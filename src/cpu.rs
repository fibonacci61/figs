use crate::{
    bus::Bus,
    dma::{DEST_BASE_ADDR, DmaState, PAYLOAD_SIZE},
};

#[derive(Debug)]
pub struct Registers {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
}

pub struct Cpu {
    regs: Registers,
    bus: Bus,

    // interrupt master enable
    // enables/disables all non maskable interrupts
    ime: bool,
}

const FLAG_Z: u8 = 0x80;
const FLAG_N: u8 = 0x40;
const FLAG_H: u8 = 0x20;
const FLAG_C: u8 = 0x10;

impl Registers {
    // initialize with values from DMG ROM hand-off state
    pub fn new(header_checksum: u8) -> Self {
        Self {
            a: 0x01,
            f: if header_checksum == 0 {
                FLAG_Z
            } else {
                FLAG_Z | FLAG_H | FLAG_C
            },
            b: 0x00,
            c: 0x13,
            d: 0x00,
            e: 0xD8,
            h: 0x01,
            l: 0x4D,
            pc: 0x0100,
            sp: 0xFFFE,
        }
    }

    pub fn af(&self) -> u16 {
        ((self.a as u16) << 8) | (self.f as u16)
    }

    pub fn bc(&self) -> u16 {
        ((self.b as u16) << 8) | (self.c as u16)
    }

    pub fn de(&self) -> u16 {
        ((self.d as u16) << 8) | (self.e as u16)
    }

    pub fn hl(&self) -> u16 {
        ((self.h as u16) << 8) | (self.l as u16)
    }

    pub fn set_af(&mut self, value: u16) {
        self.a = (value >> 8) as u8;
        self.f = value as u8;
    }

    pub fn set_bc(&mut self, value: u16) {
        self.b = (value >> 8) as u8;
        self.c = value as u8;
    }

    pub fn set_de(&mut self, value: u16) {
        self.d = (value >> 8) as u8;
        self.e = value as u8;
    }

    pub fn set_hl(&mut self, value: u16) {
        self.h = (value >> 8) as u8;
        self.l = value as u8;
    }

    pub fn set_flag_z(&mut self, value: bool) {
        if value {
            self.f |= FLAG_Z;
        } else {
            self.f &= !FLAG_Z
        }
    }

    pub fn set_flag_n(&mut self, value: bool) {
        if value {
            self.f |= FLAG_N;
        } else {
            self.f &= !FLAG_N
        }
    }

    pub fn set_flag_h(&mut self, value: bool) {
        if value {
            self.f |= FLAG_H;
        } else {
            self.f &= !FLAG_H
        }
    }
    pub fn set_flag_c(&mut self, value: bool) {
        if value {
            self.f |= FLAG_C;
        } else {
            self.f &= !FLAG_C
        }
    }

    pub fn register(&self, reg: GpRegister) -> u8 {
        match reg {
            GpRegister::A => self.a,
            GpRegister::B => self.b,
            GpRegister::C => self.c,
            GpRegister::D => self.d,
            GpRegister::E => self.e,
            GpRegister::H => self.h,
            GpRegister::L => self.l,
        }
    }

    pub fn set_register(&mut self, reg: GpRegister, value: u8) {
        match reg {
            GpRegister::A => self.a = value,
            GpRegister::B => self.b = value,
            GpRegister::C => self.c = value,
            GpRegister::D => self.d = value,
            GpRegister::E => self.e = value,
            GpRegister::H => self.h = value,
            GpRegister::L => self.l = value,
        }
    }
}

#[derive(Clone, Copy)]
pub enum GpRegister {
    A,
    B,
    C,
    D,
    E,
    H,
    L,
}

impl Cpu {
    pub fn new(bus: Bus) -> Self {
        Self {
            regs: Registers::new(bus.cartridge.header_flags().header_checksum),
            bus,
            ime: false,
        }
    }

    fn fetch_byte(&mut self) -> u8 {
        let byte = self.bus.read(self.regs.pc);
        self.regs.pc += 1;
        byte
    }

    fn fetch_word(&mut self) -> u16 {
        let lo = self.fetch_byte();
        let hi = self.fetch_byte();
        ((hi as u16) << 8) | (lo as u16)
    }

    fn push_byte(&mut self, value: u8) {
        self.regs.sp -= 1;
        self.bus.write(self.regs.sp, value);
    }

    fn push_word(&mut self, value: u16) {
        self.push_byte((value >> 8) as u8);
        self.push_byte(value as u8);
    }

    fn pop_byte(&mut self) -> u8 {
        let byte = self.bus.read(self.regs.sp);
        self.regs.sp += 1;
        byte
    }

    fn pop_word(&mut self) -> u16 {
        let lo = self.pop_byte();
        let hi = self.pop_byte();
        ((hi as u16) << 8) | (lo as u16)
    }

    fn inc(&mut self, reg: GpRegister) -> u32 {
        self.regs
            .set_flag_h((self.regs.register(reg) & 0x0F) == 0x0F);

        self.regs
            .set_register(reg, self.regs.register(reg).wrapping_add(1));

        self.regs.set_flag_z(self.regs.register(reg) == 0);
        self.regs.set_flag_n(false);
        1
    }

    fn dec(&mut self, reg: GpRegister) -> u32 {
        self.regs.set_flag_h((self.regs.register(reg) & 0x0F) == 0);
        self.regs
            .set_register(reg, self.regs.register(reg).wrapping_sub(1));

        self.regs.set_flag_z(self.regs.register(reg) == 0);
        self.regs.set_flag_n(true);
        1
    }

    pub fn step(&mut self) -> u32 {
        log::trace!("executing instruction at 0x{:04X}", self.regs.pc);
        println!(
            "A:{:02X} F:{:02X} B:{:02X} C:{:02X} D:{:02X} E:{:02X} H:{:02X} L:{:02X} SP:{:04X} PC:{:04X} PCMEM:{:02X},{:02X},{:02X},{:02X}",
            self.regs.a,
            self.regs.f,
            self.regs.b,
            self.regs.c,
            self.regs.d,
            self.regs.e,
            self.regs.h,
            self.regs.l,
            self.regs.sp,
            self.regs.pc,
            self.bus.read(self.regs.pc),
            self.bus.read(self.regs.pc + 1),
            self.bus.read(self.regs.pc + 2),
            self.bus.read(self.regs.pc + 3),
        );
        let opcode = self.fetch_byte();
        match opcode {
            // nop
            0x00 => 1,
            // jp a16
            0xC3 => {
                self.regs.pc = self.fetch_word();
                log::trace!("jumped to addr 0x{:X}", self.regs.pc);
                4
            }
            // di
            0xF3 => {
                self.ime = false;
                1
            }
            // ld sp, d16
            0x31 => {
                self.regs.sp = self.fetch_word();
                log::trace!("set sp to 0x{:X}", self.regs.sp);
                3
            }
            // ld (a16), a
            0xEA => {
                let addr = self.fetch_word();
                self.bus.write(addr, self.regs.a);
                4
            }
            // ld a, d8
            0x3E => {
                self.regs.a = self.fetch_byte();
                2
            }
            // ld (a8), a
            0xE0 => {
                let addr = (self.fetch_byte() as u16) | 0xFF00;
                self.bus.write(addr, self.regs.a);
                3
            }
            // ld hl, d16
            0x21 => {
                let value = self.fetch_word();
                self.regs.set_hl(value);
                3
            }
            // call a16
            0xCD => {
                let addr = self.fetch_word();
                self.push_word(self.regs.pc);
                self.regs.pc = addr;
                log::trace!("called routine at addr 0x{addr:X}");
                6
            }
            // ld a, l
            0x7D => {
                self.regs.a = self.regs.l;
                1
            }
            // ld a, h
            0x7C => {
                self.regs.a = self.regs.h;
                1
            }
            // jr s8
            0x18 => {
                let offset = self.fetch_byte() as i8 as i16;
                self.regs.pc = self.regs.pc.wrapping_add_signed(offset);
                3
            }
            // ret
            0xC9 => {
                let addr = self.pop_word();
                self.regs.pc = addr;
                log::trace!("returning to addr {addr:X}");
                4
            }
            // push hl
            0xE5 => {
                self.push_word(self.regs.hl());
                4
            }
            // pop hl
            0xE1 => {
                let value = self.pop_word();
                self.regs.set_hl(value);
                3
            }
            // push af
            0xF5 => {
                self.push_word(self.regs.af());
                4
            }
            // inc hl
            0x23 => {
                self.regs.set_hl(self.regs.hl().wrapping_add(1));
                2
            }
            // ld a, (hl+)
            0x2A => {
                let value = self.bus.read(self.regs.hl());
                self.regs.a = value;
                self.regs.set_hl(self.regs.hl().wrapping_add(1));
                2
            }
            // pop af
            0xF1 => {
                let value = self.pop_word();
                self.regs.set_af(value);
                3
            }
            // push bc
            0xC5 => {
                self.push_word(self.regs.bc());
                4
            }
            // ld bc, d16
            0x01 => {
                let value = self.fetch_word();
                self.regs.set_bc(value);
                3
            }
            // inc bc
            0x03 => {
                self.regs.set_bc(self.regs.bc().wrapping_add(1));
                2
            }
            // ld a, b
            0x78 => {
                self.regs.a = self.regs.b;
                1
            }
            // or c
            0xB1 => {
                self.regs.a |= self.regs.c;

                self.regs.set_flag_z(self.regs.a == 0);
                self.regs.set_flag_n(false);
                self.regs.set_flag_h(false);
                self.regs.set_flag_c(false);
                1
            }
            // jr z, s8
            0x28 => {
                let offset = self.fetch_byte() as i8 as i16;
                if (self.regs.f & FLAG_Z) != 0 {
                    self.regs.pc = self.regs.pc.wrapping_add_signed(offset);
                    3
                } else {
                    2
                }
            }
            // pop bc
            0xC1 => {
                let value = self.pop_word();
                self.regs.set_bc(value);
                3
            }
            // ld a, (a16)
            0xFA => {
                let addr = self.fetch_word();
                self.regs.a = self.bus.read(addr);
                4
            }
            // and d8
            0xE6 => {
                let value = self.fetch_byte();
                self.regs.a &= value;

                self.regs.set_flag_z(self.regs.a == 0);
                self.regs.set_flag_n(false);
                self.regs.set_flag_h(true);
                self.regs.set_flag_c(false);
                2
            }
            // call nz, a16
            0xC4 => {
                let addr = self.fetch_word();
                if (self.regs.f & FLAG_Z) == 0 {
                    self.push_word(self.regs.pc);
                    self.regs.pc = addr;
                    6
                } else {
                    3
                }
            }
            // ld b, d8
            0x06 => {
                self.regs.b = self.fetch_byte();
                2
            }
            // ld (hl), a
            0x77 => {
                self.bus.write(self.regs.hl(), self.regs.a);
                2
            }
            // inc l
            0x2C => self.inc(GpRegister::L),
            // jr nz, s8
            0x20 => {
                let offset = self.fetch_byte() as i8 as i16;
                if (self.regs.f & FLAG_Z) == 0 {
                    self.regs.pc = self.regs.pc.wrapping_add_signed(offset);
                    3
                } else {
                    2
                }
            }
            // inc h
            0x24 => self.inc(GpRegister::H),
            // dec b
            0x05 => self.dec(GpRegister::B),
            // ld c, d8
            0x0E => {
                self.regs.c = self.fetch_byte();
                2
            }
            // ld de, d16
            0x11 => {
                let value = self.fetch_word();
                self.regs.set_de(value);
                3
            }
            // ld a, (de)
            0x1A => {
                self.regs.a = self.bus.read(self.regs.de());
                2
            }
            // inc de
            0x13 => {
                self.regs.set_de(self.regs.de().wrapping_add(1));
                2
            }
            // xor c
            0xA9 => {
                self.regs.a ^= self.regs.c;

                self.regs.set_flag_z(self.regs.a == 0);
                self.regs.set_flag_n(false);
                self.regs.set_flag_h(false);
                self.regs.set_flag_c(false);
                1
            }
            // ld a, (a8)
            0xF0 => {
                let addr = (self.fetch_byte() as u16) | 0xFF00;
                self.regs.a = self.bus.read(addr);
                3
            }
            // cp d8
            0xFE => {
                let value = self.fetch_byte();
                let (result, overflown) = self.regs.a.overflowing_sub(value);

                self.regs.set_flag_z(result == 0);
                self.regs.set_flag_n(true);
                self.regs.set_flag_h((self.regs.a & 0x0F) < (value & 0x0F));
                self.regs.set_flag_c(overflown);
                2
            }
            // ld (hl-), a
            0x32 => {
                self.bus.write(self.regs.hl(), self.regs.a);
                self.regs.set_hl(self.regs.hl().wrapping_sub(1));
                2
            }
            // jr c, s8
            0x38 => {
                let offset = self.fetch_byte() as i8 as i16;
                if (self.regs.f & FLAG_C) != 0 {
                    self.regs.pc = self.regs.pc.wrapping_add_signed(offset);
                    3
                } else {
                    2
                }
            }
            // ld a, (hl)
            0x7E => {
                println!("{:#X?}", self.regs);
                self.regs.a = self.bus.read(self.regs.hl());
                2
            }
            // ld b, a
            0x47 => {
                self.regs.b = self.regs.a;
                1
            }
            // ld (de), a
            0x12 => {
                self.bus.write(self.regs.de(), self.regs.a);
                3
            }
            // inc e
            0x1C => self.inc(GpRegister::E),
            // inc d
            0x14 => self.inc(GpRegister::D),
            // dec c
            0x0D => self.dec(GpRegister::C),
            // ld (hl+), a
            0x22 => {
                self.bus.write(self.regs.hl(), self.regs.a);
                self.regs.set_hl(self.regs.hl().wrapping_add(1));
                2
            }
            // inc a
            0x3C => self.inc(GpRegister::A),
            _ => panic!(
                "unimplemented opcode: 0x{opcode:02X} at addr {:X}",
                self.regs.pc
            ),
        }
    }

    pub fn step_dma(&mut self, cycles: u32) {
        match self.bus.dma.step(cycles) {
            // maybe merge these two states together?
            DmaState::Free | DmaState::Working => {}
            DmaState::Done { src_addr } => {
                for i in 0..PAYLOAD_SIZE {
                    let value = self.bus.read(src_addr + i);
                    self.bus.write(DEST_BASE_ADDR + i, value);
                }
            }
        }
    }
}
