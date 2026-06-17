use std::{cell::RefCell, cmp::Ordering, collections::VecDeque, rc::Rc};

use bitflags::bitflags;
use bytemuck::{Pod, Zeroable};
use minifb::Window;

use crate::{bus::VRAM_LEN, cpu::Interrupt};

pub const OAM_LEN: usize = 40;
pub const SCREEN_WIDTH: usize = 160;
pub const SCREEN_HEIGHT: usize = 144;

#[derive(Debug, Clone, Copy)]
enum State {
    Mode2,
    Mode3(Mode3State),
    Mode0,
}

#[derive(Debug, Clone, Copy)]
enum FetcherState {
    FetchId,
    FetchLow { id: u8 },
    FetchHigh { id: u8, low: u8 },
    Push { low: u8, high: u8 },
}

#[derive(Debug, Clone, Copy)]
enum SpriteFetcherMode {
    FetchTile,
    FetchLow { row_addr: u16 },
    FetchHigh { row_addr: u16, low: u8 },
}

#[derive(Debug, Clone, Copy)]
struct SpriteFetcherState {
    delay: u8,
    obj: ObjectAttribute,
    mode: SpriteFetcherMode,
}

#[derive(Debug, Clone, Copy)]
struct Mode3State {
    fetcher_delay: u8,
    window_fetch: bool,
    fetcher_state: FetcherState,

    discard_counter: u8,
    sprite_fetcher: Option<SpriteFetcherState>,
}

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(C)]
struct ObjectAttribute {
    pos_y: u8,
    pos_x: u8,
    tile_index: u8,
    flags: ObjFlags,
}

#[bitfields::bitfield(u8)]
#[derive(PartialEq, Eq, Pod, Zeroable)]
struct ObjFlags {
    priority: bool,
    y_flip: bool,
    x_flip: bool,
    dmg_palette: bool,
    #[bits(4)]
    _reserved: u8,
}

#[bitfields::bitfield(u8)]
#[derive(PartialEq, Eq)]
struct Stat {
    _reserved: bool,
    lyc_int_select: bool,
    mode2_int_select: bool,
    mode1_int_select: bool,
    mode0_int_select: bool,
    lyc_eq_ly: bool,
    #[bits(2)]
    ppu_mode: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pixel {
    Zero,
    One,
    Two,
    Three,
}

impl Pixel {
    fn from_bits(value: u8) -> Self {
        match value {
            0b00 => Self::Zero,
            0b01 => Self::One,
            0b10 => Self::Two,
            0b11 => Self::Three,
            _ => panic!("invalid pixel bits"),
        }
    }

    fn apply_palette(self, palette: u8) -> Self {
        Self::from_bits(
            palette
                >> match self {
                    Self::Zero => 0,
                    Self::One => 2,
                    Self::Two => 4,
                    Self::Three => 6,
                }
                & 0b11,
        )
    }

    fn to_bgra(self) -> u32 {
        match self {
            Self::Zero => 0xFFFFFF,
            Self::One => 0xC0C0C0,
            Self::Two => 0x404040,
            Self::Three => 0x000000,
        }
    }
}

#[derive(Clone, Copy)]
struct SpritePixel {
    pixel: Pixel,
    bg_over_obj: bool,
    palette: u8,
}

pub struct Ppu {
    window: Window,
    fb: [u32; SCREEN_HEIGHT * SCREEN_WIDTH],
    int_queue: Rc<RefCell<VecDeque<Interrupt>>>,

    vram: [u8; VRAM_LEN],
    oam: [ObjectAttribute; OAM_LEN],

    dot: u32,
    mode: State,
    lcdc: Lcdc,

    pub scroll_x: u8,
    pub scroll_y: u8,
    pub wx: u8,
    pub wy: u8,

    pub bgp: u8,
    pub obp0: u8,
    pub obp1: u8,

    pub lyc: u8,
    stat: Stat,
    stat_line: bool,

    line: u8,
    window_line: u8,
    object_buffer: Vec<ObjectAttribute>,

    wy_condition: Option<bool>,

    tile_x: u8,
    screen_x: u8,
    bg_fifo: VecDeque<Pixel>,
    obj_fifo: VecDeque<SpritePixel>,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Lcdc: u8 {
        const LCD_ENABLE = 1 << 7;
        const WINDOW_TILEMAP_AREA = 1 << 6;
        const WINDOW_ENABLE = 1 << 5;
        const BG_WINDOW_TILE_AREA = 1 << 4;
        const BG_TILEMAP_AREA = 1 << 3;
        const OBJ_SIZE = 1 << 2;
        const OBJ_ENABLE = 1 << 1;
        const BG_WINDOW_ENABLE = 1 << 0;
    }
}

// width of tile in pixels
const TILE_WIDTH: u16 = 8;
// width/height of tilemap in tiles
const TILEMAP_WIDTH: usize = 32;
// length of tile data in bytes
const TILE_LEN: usize = 16;
const OBJ_Y_OFFSET: u8 = 16;

impl Ppu {
    pub fn new(mut window: Window, int_queue: Rc<RefCell<VecDeque<Interrupt>>>) -> Self {
        let fb = [0; SCREEN_WIDTH * SCREEN_HEIGHT];
        window
            .update_with_buffer(&fb, SCREEN_WIDTH, SCREEN_HEIGHT)
            .unwrap();
        Self {
            window,
            fb,
            int_queue,
            vram: [0; VRAM_LEN],
            oam: [ObjectAttribute::zeroed(); OAM_LEN],
            // can be thought of as representing how many dots been completed
            dot: 0,
            mode: State::Mode2,
            scroll_x: 0,
            scroll_y: 0,
            wx: 0,
            wy: 0,
            lyc: 0,
            stat: Stat::new(),
            stat_line: false,
            line: 0,
            window_line: 0,
            // DMG post-boot LCDC = 0x91
            lcdc: Lcdc::LCD_ENABLE | Lcdc::BG_WINDOW_TILE_AREA | Lcdc::BG_WINDOW_ENABLE,
            wy_condition: None,
            // DMG post-boot BGP = 0xFC
            bgp: 0xFC,
            // OBP0/OBP1 are left uninitialized by the boot ROM; 0xFF by convention
            obp0: 0xFF,
            obp1: 0xFF,
            object_buffer: Vec::with_capacity(10),
            tile_x: 0,
            screen_x: 0,
            bg_fifo: VecDeque::new(),
            obj_fifo: VecDeque::new(),
        }
    }

    // VRAM is only locked during mode 3, freely accessible while the LCD is off.
    fn vram_accessible(&self) -> bool {
        !self.lcdc.contains(Lcdc::LCD_ENABLE) || self.mode_number() != 3
    }

    // OAM is locked during modes 2 and 3, freely accessible while the LCD is off.
    fn oam_accessible(&self) -> bool {
        !self.lcdc.contains(Lcdc::LCD_ENABLE) || !matches!(self.mode_number(), 2 | 3)
    }

    pub fn vram(&self) -> Option<&[u8; VRAM_LEN]> {
        self.vram_accessible().then_some(&self.vram)
    }

    pub fn vram_mut(&mut self) -> Option<&mut [u8; VRAM_LEN]> {
        self.vram_accessible().then(|| &mut self.vram)
    }

    pub fn oam(&self) -> Option<&[u8]> {
        self.oam_accessible()
            .then(|| bytemuck::cast_slice(self.oam.as_slice()))
    }

    pub fn oam_mut(&mut self) -> Option<&mut [u8]> {
        self.oam_accessible()
            .then(|| bytemuck::cast_slice_mut(self.oam.as_mut_slice()))
    }

    pub fn ly(&self) -> u8 {
        self.line
    }

    fn mode_number(&self) -> u8 {
        if self.line >= 144 {
            1
        } else {
            match self.mode {
                State::Mode2 => 2,
                State::Mode3(_) => 3,
                State::Mode0 => 0,
            }
        }
    }

    pub fn stat(&self) -> u8 {
        let mut stat = self.stat;

        stat.set_lyc_eq_ly(self.lyc == self.line);
        stat.set_ppu_mode(if !self.lcdc.contains(Lcdc::LCD_ENABLE) {
            0
        } else {
            self.mode_number()
        });

        stat.into_bits()
    }

    pub fn set_stat(&mut self, value: u8) {
        self.stat = Stat::from_bits(value);
    }

    fn compute_stat_line(&self) -> bool {
        (self.stat.mode2_int_select()
            && (self.mode_number() == 2 || (self.line == 144 && self.dot == 0)))
            || (self.stat.mode1_int_select() && self.mode_number() == 1)
            || (self.stat.mode0_int_select() && self.mode_number() == 0)
            || (self.stat.lyc_int_select() && self.line == self.lyc)
    }

    pub fn lcdc(&self) -> u8 {
        self.lcdc.bits()
    }

    pub fn set_lcdc(&mut self, value: u8) {
        self.lcdc = Lcdc::from_bits_retain(value);
        if !self.lcdc.contains(Lcdc::LCD_ENABLE) {
            // this bit should only be cleared during VBlank on real hardware
            // we allow it in emulation, but a warning is printed out
            if self.line >= 144 {
                log::warn!("LCD_ENABLE set while not in VBlank");
            }
            // reset everything
            self.line = 0;
            self.window_line = 0;
            self.dot = 0;
            self.tile_x = 0;
            self.screen_x = 0;
            self.wy_condition = None;
            self.mode = State::Mode2;
            // draw black screen
            self.fb.fill(0);
            self.window
                .update_with_buffer(&self.fb, SCREEN_WIDTH, SCREEN_HEIGHT)
                .expect("drawing failed");
        }
    }

    fn bg_map_addr(&self) -> usize {
        if self.lcdc.contains(Lcdc::BG_TILEMAP_AREA) {
            // if set, BG uses tilemap 0x9C00
            0x1C00
        } else {
            // if clear, tilemap 0x9800 is used
            0x1800
        }
    }

    fn window_map_addr(&self) -> usize {
        if self.lcdc.contains(Lcdc::WINDOW_TILEMAP_AREA) {
            0x1C00
        } else {
            0x1800
        }
    }

    fn bg_tile_addr(&self, id: u8) -> usize {
        if self.lcdc.contains(Lcdc::BG_WINDOW_TILE_AREA) {
            // 0x8000 mode, unsigned addressing
            let offset = (id as usize) * TILE_LEN;
            0x0000 + offset
        } else {
            // 0x9000 mode, signed addressing
            let offset = (id as i8 as isize) * TILE_LEN as isize;
            (0x1000 + offset) as usize
        }
    }

    fn sprite_height(&self) -> u8 {
        if self.lcdc.contains(Lcdc::OBJ_SIZE) {
            16
        } else {
            8
        }
    }

    fn step_mode2(&mut self) {
        if self.dot == 0 {
            self.object_buffer.clear();
            self.bg_fifo.clear();
            self.obj_fifo.clear();
        }

        if self.wy_condition.is_none() {
            self.wy_condition = Some(self.wy == self.line);
        }

        // 80 dots, 40 entries, 2 dots per entry
        // trigger only on odd numbers
        if self.dot.is_multiple_of(2) {
            return;
        }

        // 79 dots have been completed, so this is the 80th
        if self.dot == 79 {
            self.mode = State::Mode3(Mode3State {
                // 4 dot penalty on first fetch
                fetcher_delay: 4,
                window_fetch: false,
                fetcher_state: FetcherState::FetchId,

                discard_counter: self.scroll_x % (TILE_WIDTH as u8),
                sprite_fetcher: None,
            });
            self.screen_x = 0;
            self.object_buffer
                .sort_by(|a, b| match a.pos_x.cmp(&b.pos_x) {
                    Ordering::Equal => a.tile_index.cmp(&b.tile_index),
                    x => x,
                });
        }

        if self.object_buffer.len() >= 10 {
            return;
        }

        // dot 1 => idx 0
        // dot 3 => idx 1
        // ...
        // dot 79 => idx 39
        let entry_idx = self.dot / 2;
        let entry = self.oam[entry_idx as usize];

        // line needs to be in this range for sprite to be visible
        let visible_range = entry.pos_y..entry.pos_y.saturating_add(self.sprite_height());
        if visible_range.contains(&(self.line + OBJ_Y_OFFSET)) {
            self.object_buffer.push(entry);
        }
    }

    fn step_fetcher(
        &mut self,
        state: FetcherState,
        first_fetch_counter: &mut u8,
        window_fetch: bool,
    ) -> FetcherState {
        // 4 dot penalty on first fetch
        if *first_fetch_counter > 0 {
            *first_fetch_counter -= 1;
            return state;
        }

        // mode 3 starts on dot 80, so operations that take two dots should work on the second
        // dot, i.e. on dot 81, 83, 85, not 80, 82, 84
        // thus we require that dot is odd for these operations
        match state {
            FetcherState::FetchId => {
                if self.dot.is_multiple_of(2) {
                    return state;
                }

                let (map_col, map_row, base) = if window_fetch {
                    let map_col = self.tile_x % (TILEMAP_WIDTH as u8);
                    let map_row = (self.window_line / 8) % TILEMAP_WIDTH as u8;

                    (map_col, map_row, self.window_map_addr())
                } else {
                    let map_row_px = self.line as u16 + self.scroll_y as u16;
                    let map_row = map_row_px / TILE_WIDTH;
                    // wrap
                    let map_row = map_row % (TILEMAP_WIDTH as u16);

                    let map_col = self.tile_x as u16 + (self.scroll_x as u16) / TILE_WIDTH;
                    // wrap
                    let map_col = map_col % (TILEMAP_WIDTH as u16);

                    (map_col as u8, map_row as u8, self.bg_map_addr())
                };

                let id_offset = (map_row as usize) * TILEMAP_WIDTH + (map_col as usize);

                let id = self.vram[base + id_offset];

                self.tile_x += 1;

                FetcherState::FetchLow { id }
            }
            FetcherState::FetchLow { id } => {
                if self.dot.is_multiple_of(2) {
                    return state;
                }

                let tile_row = if window_fetch {
                    self.window_line % 8
                } else {
                    self.scroll_y.wrapping_add(self.line) % 8
                };

                FetcherState::FetchHigh {
                    id,
                    low: self.vram[self.bg_tile_addr(id) + (tile_row as usize) * 2],
                }
            }
            FetcherState::FetchHigh { id, low } => {
                if self.dot.is_multiple_of(2) {
                    return state;
                }

                let tile_row = if window_fetch {
                    self.window_line % 8
                } else {
                    self.scroll_y.wrapping_add(self.line) % 8
                };

                FetcherState::Push {
                    low,
                    high: self.vram[self.bg_tile_addr(id) + (tile_row as usize) * 2 + 1],
                }
            }
            FetcherState::Push { low, high } => {
                if self.dot.is_multiple_of(2) {
                    return state;
                }

                // fifo must have less than 8 pixels inside already
                // if this condition is not met, then pushing will be reattempted in 2 dots
                if self.bg_fifo.len() > 8 {
                    return state;
                }

                for i in (0..8).rev() {
                    let low_bit = (low >> i) & 0x01;
                    let high_bit = (high >> i) & 0x01;
                    let pixel_bits = low_bit | (high_bit << 1);
                    let pixel = Pixel::from_bits(pixel_bits);
                    self.bg_fifo.push_back(pixel);
                }

                FetcherState::FetchId
            }
        }
    }

    fn step_sprite_fetcher(&mut self, state: &mut SpriteFetcherState) -> bool {
        if state.delay > 0 {
            state.delay -= 1;
            return false;
        }

        if self.dot.is_multiple_of(2) {
            return false;
        }

        match state.mode {
            SpriteFetcherMode::FetchTile => {
                let line_obj_distance = (self.line + OBJ_Y_OFFSET) - state.obj.pos_y;

                let row_addr = if self.lcdc.contains(Lcdc::OBJ_SIZE) {
                    // if the scanline is drawing the bottom tile
                    let mut is_bottom_tile = line_obj_distance >= 8;
                    // invert if y flip is set
                    is_bottom_tile = is_bottom_tile != state.obj.flags.y_flip();

                    let tile_index = if is_bottom_tile {
                        state.obj.tile_index | 0x01
                    } else {
                        state.obj.tile_index & 0xFE
                    };

                    let mut tile_row = line_obj_distance % 8;
                    if state.obj.flags.y_flip() {
                        tile_row = 7 - tile_row;
                    }

                    (tile_index as u16) * (TILE_LEN as u16) + (tile_row as u16) * 2
                } else {
                    let tile_row = if state.obj.flags.y_flip() {
                        7 - line_obj_distance
                    } else {
                        line_obj_distance
                    };

                    (state.obj.tile_index as u16) * (TILE_LEN as u16) + (tile_row as u16) * 2
                };

                state.mode = SpriteFetcherMode::FetchLow { row_addr };
                false
            }
            SpriteFetcherMode::FetchLow { row_addr } => {
                state.mode = SpriteFetcherMode::FetchHigh {
                    row_addr,
                    low: self.vram[row_addr as usize],
                };
                false
            }
            SpriteFetcherMode::FetchHigh { row_addr, low } => {
                let high = self.vram[(row_addr as usize) + 1];

                for i in (0..8).rev() {
                    // if x flip is set, read the bits in reverse order
                    let bit = if state.obj.flags.x_flip() { 7 - i } else { i };

                    let low_bit = (low >> bit) & 0x01;
                    let high_bit = (high >> bit) & 0x01;
                    let pixel_bits = low_bit | (high_bit << 1);
                    let pixel = Pixel::from_bits(pixel_bits);

                    let sprite_pixel = SpritePixel {
                        pixel,
                        bg_over_obj: state.obj.flags.priority(),
                        // TODO: should palette be read at composite time or at fetch time?
                        palette: if state.obj.flags.dmg_palette() {
                            self.obp1
                        } else {
                            self.obp0
                        },
                    };

                    if let Some(a) = self.obj_fifo.get_mut(7 - i) {
                        if a.pixel == Pixel::Zero {
                            *a = sprite_pixel
                        }
                    } else {
                        self.obj_fifo.push_back(sprite_pixel);
                    }
                }

                true
            }
        }
    }

    // ugly ass code lmao
    fn composite_pixel(&mut self, bg_pixel: Pixel, sprite_pixel: SpritePixel) -> Pixel {
        if !self.lcdc.contains(Lcdc::BG_WINDOW_ENABLE) {
            return if sprite_pixel.pixel == Pixel::Zero {
                Pixel::Zero.apply_palette(self.bgp)
            } else {
                sprite_pixel.pixel.apply_palette(sprite_pixel.palette)
            };
        }

        if sprite_pixel.pixel == Pixel::Zero {
            return bg_pixel.apply_palette(self.bgp);
        }

        if sprite_pixel.bg_over_obj {
            return if bg_pixel == Pixel::Zero {
                sprite_pixel.pixel.apply_palette(sprite_pixel.palette)
            } else {
                bg_pixel.apply_palette(self.bgp)
            };
        }

        sprite_pixel.pixel.apply_palette(sprite_pixel.palette)
    }

    fn step_shifter(
        &mut self,
        discard_counter: &mut u8,
        sprite_fetcher: &mut Option<SpriteFetcherState>,
    ) {
        let Some(bg_pixel) = self.bg_fifo.pop_front() else {
            return;
        };

        if *discard_counter > 0 {
            *discard_counter -= 1;
            return;
        }

        // search for sprites
        let mut selected_object_i = None;
        for (i, object) in self.object_buffer.iter().enumerate() {
            if !self.lcdc.contains(Lcdc::OBJ_ENABLE) {
                continue;
            }
            if object.pos_x.wrapping_sub(8) == self.screen_x {
                self.bg_fifo.push_front(bg_pixel);
                selected_object_i = Some(i);
                break;
            }
        }

        if let Some(i) = selected_object_i {
            let mut alignment_penalty =
                5 - u8::min(5, self.screen_x.wrapping_add(self.scroll_x) % 8);
            if !alignment_penalty.is_multiple_of(2) {
                // preserve `is_multiple_of(2)` timing gates
                alignment_penalty += 1;
            }

            *sprite_fetcher = Some(SpriteFetcherState {
                delay: 6 + alignment_penalty,
                mode: SpriteFetcherMode::FetchTile,
                // remove so it can't be drawn during this scanline again
                obj: self.object_buffer.remove(i),
            });
            return;
        }

        let sprite_pixel = self.obj_fifo.pop_front().unwrap_or(SpritePixel {
            pixel: Pixel::Zero,
            bg_over_obj: false,
            palette: self.obp0,
        });

        self.fb[(self.screen_x as usize) + (self.line as usize) * SCREEN_WIDTH] =
            self.composite_pixel(bg_pixel, sprite_pixel).to_bgra();
        self.screen_x += 1;

        if self.screen_x == SCREEN_WIDTH as u8 {
            self.mode = State::Mode0;
        }
    }

    fn step_mode3(&mut self, state: Mode3State) {
        let Mode3State {
            mut fetcher_delay,
            mut window_fetch,
            fetcher_state,
            mut discard_counter,
            mut sprite_fetcher,
        } = state;

        if let Some(sf) = sprite_fetcher.as_mut() {
            if self.step_sprite_fetcher(sf) {
                self.mode = State::Mode3(Mode3State {
                    fetcher_delay: state.fetcher_delay,
                    window_fetch,
                    fetcher_state: state.fetcher_state,
                    discard_counter: state.discard_counter,
                    sprite_fetcher: None,
                })
            }
        } else {
            let mut fetcher_state =
                self.step_fetcher(fetcher_state, &mut fetcher_delay, window_fetch);

            if self.wy_condition == Some(true)
                && self.lcdc.contains(Lcdc::WINDOW_ENABLE)
                && self.screen_x + 7 == self.wx
                && !window_fetch
            {
                window_fetch = true;
                fetcher_delay += 6;
                self.bg_fifo.clear();
                self.tile_x = 0;
                fetcher_state = FetcherState::FetchId;
            } else {
                // shifter will not do anything if the above branch is run because bg_fifo is
                // cleared
                self.step_shifter(&mut discard_counter, &mut sprite_fetcher);
            }

            // step_shifter might have set the mode to Mode0
            if !matches!(self.mode, State::Mode0) {
                self.mode = State::Mode3(Mode3State {
                    fetcher_state,
                    window_fetch,
                    fetcher_delay,
                    discard_counter,
                    sprite_fetcher,
                });
            }
        }
    }

    pub fn step(&mut self) {
        if !self.lcdc.contains(Lcdc::LCD_ENABLE) {
            return;
        }

        let new_stat_line = self.compute_stat_line();
        // rising edge detection
        if new_stat_line && new_stat_line != self.stat_line {
            self.int_queue.borrow_mut().push_back(Interrupt::Stat);
        }
        self.stat_line = new_stat_line;

        if self.line >= 144 {
            // wait for 10 lines (4560 dots)

            // fire vblank interrupt if we just entered vblank
            if self.dot == 0 {
                self.int_queue.borrow_mut().push_back(Interrupt::VBlank);
            }
            // we can't set dot=0 since the conditional above relies on it corresponding to the
            // start of vblank, so we use modular arithmetic to check for the line ending instead
            if (self.dot % 456) == 455 {
                self.line += 1;
            }
            // 4559 dots have been completed, so this is the 4560th
            if self.dot == 4559 {
                // restart the process
                self.dot = 0;
                self.tile_x = 0;
                self.screen_x = 0;
                self.line = 0;
                self.window_line = 0;
                self.wy_condition = None;
                return;
            }
            self.dot += 1;
            return;
        }

        match self.mode {
            State::Mode2 => self.step_mode2(),
            State::Mode3(state) => self.step_mode3(state),
            State::Mode0 => {
                // 455 dots have been completed, so this is the 456th
                if self.dot == 455 {
                    self.window
                        .update_with_buffer(&self.fb, SCREEN_WIDTH, SCREEN_HEIGHT)
                        .unwrap();
                    self.line += 1;
                    if self.wy_condition == Some(true) {
                        self.window_line += 1;
                    }
                    self.dot = 0;
                    self.tile_x = 0;
                    self.screen_x = 0;
                    if self.wy_condition == Some(false) {
                        self.wy_condition = None;
                    }
                    self.mode = State::Mode2;
                }
            }
        }

        self.dot += 1;
    }
}
