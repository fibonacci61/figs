use crate::{cartridge::Cartridge, dma::Dma, ppu::Ppu};

pub const VRAM_LEN: usize = 0x2000;
pub const WRAM_LEN: usize = 0x2000;
pub const HRAM_LEN: usize = 0x7F;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaybeInitByte {
    Uninit,
    Init(u8),
}

pub struct Bus {
    pub cartridge: Cartridge,
    pub ppu: Ppu,
    pub wram: [MaybeInitByte; WRAM_LEN],
    pub hram: [MaybeInitByte; HRAM_LEN],
    pub dma: Dma,
}

impl Bus {
    pub fn new(cartridge: Cartridge, ppu: Ppu) -> Self {
        Self {
            cartridge,
            wram: [const { MaybeInitByte::Uninit }; WRAM_LEN],
            // This is a simplification, a real Game Boy would display the Nintendo logo and scroll
            // it downwards. However most ROMs should initialize VRAM themselves so it shouldn't
            // matter.
            ppu,
            hram: [const { MaybeInitByte::Uninit }; HRAM_LEN],
            dma: Dma::new(),
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        // reject any non-hram reads/writes while dma is working
        if self.dma.is_working() && !(0xFF80..0xFFFF).contains(&addr) {
            return 0xFF;
        }

        // suppresses warning for hram pattern
        #[allow(non_contiguous_range_endpoints)]
        match addr {
            // cartridge ROM and RAM
            0x0000..0x8000 | 0xA000..0xC000 => self.cartridge.read(addr),
            // VRAM
            0x8000..0xA000 => self
                .ppu
                .vram()
                .map(|vram| vram[(addr as usize) - 0x8000])
                .unwrap_or(0xFF),
            // WRAM
            0xC000..0xE000 => match self.wram[(addr as usize) - 0xC000] {
                MaybeInitByte::Uninit => panic!("attempt to read uninit memory"),
                MaybeInitByte::Init(v) => v,
            },
            // OAM
            0xFE00..0xFEA0 => self
                .ppu
                .oam()
                .map(|oam| oam[(addr as usize) - 0xFE00])
                .unwrap_or(0xFF),
            // LCDC
            0xFF40 => self.ppu.lcdc(),
            // STAT
            0xFF41 => self.ppu.stat(),
            // SCY
            0xFF42 => self.ppu.scroll_y,
            // SCX
            0xFF43 => self.ppu.scroll_x,
            // LY
            0xFF44 => self.ppu.ly(),
            // LYC
            0xFF45 => self.ppu.lyc,
            // OAM DMA
            0xFF46 => todo!(),
            // BGP
            0xFF47 => self.ppu.bgp,
            // OBP0
            0xFF48 => self.ppu.obp0,
            // OBP1
            0xFF49 => self.ppu.obp1,
            // WX
            0xFF4A => self.ppu.wx,
            // WY
            0xFF4B => self.ppu.wy,
            // HRAM
            0xFF80..0xFFFF => match self.hram[(addr as usize) - 0xFF80] {
                MaybeInitByte::Uninit => panic!("attempt to read uninit memory"),
                MaybeInitByte::Init(v) => v,
            },
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        // reject any non-hram reads/writes while dma is working
        if self.dma.is_working() && !(0xFF80..0xFFFF).contains(&addr) {
            return;
        }

        // suppresses warning for hram pattern
        #[allow(non_contiguous_range_endpoints)]
        match addr {
            // cartridge ROM and RAM
            0x0000..0x8000 | 0xA000..0xC000 => self.cartridge.write(addr, value),
            // VRAM
            0x8000..0xA000 => {
                if let Some(vram) = self.ppu.vram_mut() {
                    vram[(addr as usize) - 0x8000] = value
                }
            }
            // WRAM
            0xC000..0xE000 => self.wram[(addr as usize) - 0xC000] = MaybeInitByte::Init(value),
            // OAM
            0xFE00..0xFEA0 => {
                if let Some(oam) = self.ppu.oam_mut() {
                    oam[(addr as usize) - 0xFE00] = value;
                }
            }
            // LCDC
            0xFF40 => self.ppu.set_lcdc(value),
            // STAT
            0xFF41 => self.ppu.set_stat(value),
            // SCY
            0xFF42 => self.ppu.scroll_y = value,
            // SCX
            0xFF43 => self.ppu.scroll_x = value,
            // LYC
            0xFF45 => self.ppu.lyc = value,
            // OAM DMA
            0xFF46 => {
                self.dma.assign_op((value as u16) * 0x100);
            }
            // BGP
            0xFF47 => self.ppu.bgp = value,
            // OBP0
            0xFF48 => self.ppu.obp0 = value,
            // OBP1
            0xFF49 => self.ppu.obp1 = value,
            // WX
            0xFF4A => self.ppu.wx = value,
            // WY
            0xFF4B => self.ppu.wy = value,
            // HRAM
            0xFF80..0xFFFF => self.hram[(addr as usize) - 0xFF80] = MaybeInitByte::Init(value),
            _ => {
                log::warn!("attempt to write value {value:X} to unmapped addr {addr:X}")
            }
        }
    }
}
