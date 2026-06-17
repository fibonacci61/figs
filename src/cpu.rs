use std::{cell::RefCell, collections::VecDeque, rc::Rc};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interrupt {
    Stat,
    VBlank,
}

pub struct Cpu {
    regs: Registers,
    bus: Bus,

    // interrupt master enable
    // enables/disables all non maskable interrupts
    ime: bool,

    int_queue: Rc<RefCell<VecDeque<Interrupt>>>,
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
        // the low 4 bits of F are hardwired to 0 and can't be written
        self.f = (value as u8) & 0xF0;
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
    HlMem,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PrefixRegister {
    B,
    C,
    D,
    E,
    H,
    L,
    LdHl,
    A,
}

impl Cpu {
    pub fn new(bus: Bus, int_queue: Rc<RefCell<VecDeque<Interrupt>>>) -> Self {
        Self {
            regs: Registers::new(bus.cartridge.header_flags().header_checksum),
            bus,
            ime: false,
            int_queue,
        }
    }

    fn prefix_reg(&self, reg: PrefixRegister) -> u8 {
        match reg {
            PrefixRegister::B => self.regs.b,
            PrefixRegister::C => self.regs.c,
            PrefixRegister::D => self.regs.d,
            PrefixRegister::E => self.regs.e,
            PrefixRegister::H => self.regs.h,
            PrefixRegister::L => self.regs.l,
            PrefixRegister::LdHl => self.bus.read(self.regs.hl()),
            PrefixRegister::A => self.regs.a,
        }
    }

    fn set_prefix_reg(&mut self, reg: PrefixRegister, value: u8) {
        match reg {
            PrefixRegister::B => self.regs.b = value,
            PrefixRegister::C => self.regs.c = value,
            PrefixRegister::D => self.regs.d = value,
            PrefixRegister::E => self.regs.e = value,
            PrefixRegister::H => self.regs.h = value,
            PrefixRegister::L => self.regs.l = value,
            PrefixRegister::LdHl => self.bus.write(self.regs.hl(), value),
            PrefixRegister::A => self.regs.a = value,
        }
    }

    fn register(&self, reg: GpRegister) -> u8 {
        match reg {
            GpRegister::A => self.regs.a,
            GpRegister::B => self.regs.b,
            GpRegister::C => self.regs.c,
            GpRegister::D => self.regs.d,
            GpRegister::E => self.regs.e,
            GpRegister::H => self.regs.h,
            GpRegister::L => self.regs.l,
            GpRegister::HlMem => self.bus.read(self.regs.hl()),
        }
    }

    fn set_register(&mut self, reg: GpRegister, value: u8) {
        match reg {
            GpRegister::A => self.regs.a = value,
            GpRegister::B => self.regs.b = value,
            GpRegister::C => self.regs.c = value,
            GpRegister::D => self.regs.d = value,
            GpRegister::E => self.regs.e = value,
            GpRegister::H => self.regs.h = value,
            GpRegister::L => self.regs.l = value,
            GpRegister::HlMem => self.bus.write(self.regs.hl(), value),
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
        self.regs.set_flag_h((self.register(reg) & 0x0F) == 0x0F);

        self.set_register(reg, self.register(reg).wrapping_add(1));

        self.regs.set_flag_z(self.register(reg) == 0);
        self.regs.set_flag_n(false);
        1
    }

    fn dec(&mut self, reg: GpRegister) -> u32 {
        self.regs.set_flag_h((self.register(reg) & 0x0F) == 0);
        self.set_register(reg, self.register(reg).wrapping_sub(1));

        self.regs.set_flag_z(self.register(reg) == 0);
        self.regs.set_flag_n(true);
        1
    }

    fn add(&mut self, reg: GpRegister, rhs: u8) {
        self.regs
            .set_flag_h((self.register(reg) & 0x0F) + (rhs & 0x0F) > 0x0F);

        let (result, overflow) = self.register(reg).overflowing_add(rhs);
        self.set_register(reg, result);

        self.regs.set_flag_z(self.register(reg) == 0);
        self.regs.set_flag_n(false);
        self.regs.set_flag_c(overflow);
    }

    fn sub(&mut self, reg: GpRegister, rhs: u8) {
        self.regs
            .set_flag_h((self.register(reg) & 0x0F) < (rhs & 0x0F));

        let (result, overflow) = self.register(reg).overflowing_sub(rhs);
        self.set_register(reg, result);

        self.regs.set_flag_z(self.register(reg) == 0);
        self.regs.set_flag_n(true);
        self.regs.set_flag_c(overflow);
    }

    fn or(&mut self, value: u8) {
        self.regs.a |= value;

        self.regs.set_flag_z(self.regs.a == 0);
        self.regs.set_flag_n(false);
        self.regs.set_flag_h(false);
        self.regs.set_flag_c(false);
    }

    fn xor(&mut self, value: u8) {
        self.regs.a ^= value;

        self.regs.set_flag_z(self.regs.a == 0);
        self.regs.set_flag_n(false);
        self.regs.set_flag_h(false);
        self.regs.set_flag_c(false);
    }

    fn bit(&mut self, reg: PrefixRegister, bit: u8) -> u32 {
        self.regs.set_flag_z(self.prefix_reg(reg) & (1 << bit) == 0);
        self.regs.set_flag_n(false);
        self.regs.set_flag_h(true);
        if reg == PrefixRegister::LdHl { 3 } else { 2 }
    }

    fn res(&mut self, reg: PrefixRegister, bit: u8) -> u32 {
        self.set_prefix_reg(reg, self.prefix_reg(reg) & !(1 << bit));
        if reg == PrefixRegister::LdHl { 4 } else { 2 }
    }

    fn set(&mut self, reg: PrefixRegister, bit: u8) -> u32 {
        self.set_prefix_reg(reg, self.prefix_reg(reg) | (1 << bit));
        if reg == PrefixRegister::LdHl { 4 } else { 2 }
    }

    fn adc(&mut self, reg: GpRegister, rhs: u8) {
        let c = if self.regs.f & FLAG_C != 0 { 1 } else { 0 };
        let lhs = self.register(reg);

        let (intermediate, c_rhs) = lhs.overflowing_add(rhs);
        let (result, c_c) = intermediate.overflowing_add(c);

        self.set_register(reg, result);

        let h_rhs = (lhs & 0x0F) + (rhs & 0x0F) > 0x0F;
        let h_c = (intermediate & 0x0F) + c > 0x0F;

        self.regs.set_flag_z(result == 0);
        self.regs.set_flag_n(false);
        self.regs.set_flag_h(h_rhs | h_c);
        self.regs.set_flag_c(c_rhs | c_c);
    }

    fn add16(&mut self, lhs: u16, rhs: u16) -> u16 {
        let (result, overflow) = lhs.overflowing_add(rhs);
        self.regs.set_flag_n(false);
        self.regs
            .set_flag_h((lhs & 0x0FFF) + (rhs & 0x0FFF) > 0x0FFF);
        self.regs.set_flag_c(overflow);
        result
    }

    fn cp(&mut self, value: u8) {
        let (result, overflown) = self.regs.a.overflowing_sub(value);

        self.regs.set_flag_z(result == 0);
        self.regs.set_flag_n(true);
        self.regs.set_flag_h((self.regs.a & 0x0F) < (value & 0x0F));
        self.regs.set_flag_c(overflown);
    }

    fn ret(&mut self) {
        let addr = self.pop_word();
        self.regs.pc = addr;
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
                self.ret();
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
                self.or(self.regs.c);
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
                self.xor(self.regs.c);
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
                self.cp(value);
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
            // add a, d8
            0xC6 => {
                let rhs = self.fetch_byte();
                self.add(GpRegister::A, rhs);
                2
            }
            // sub d8
            0xD6 => {
                let rhs = self.fetch_byte();
                self.sub(GpRegister::A, rhs);
                2
            }
            // or a
            0xB7 => {
                self.or(self.regs.a);
                1
            }
            // push de
            0xD5 => {
                self.push_word(self.regs.de());
                4
            }
            // ld b, (hl)
            0x46 => {
                self.regs.b = self.bus.read(self.regs.hl());
                2
            }
            // dec l
            0x2D => self.dec(GpRegister::L),
            // ld c, (hl)
            0x4E => {
                self.regs.c = self.bus.read(self.regs.hl());
                2
            }
            // ld d, (hl)
            0x56 => {
                self.regs.d = self.bus.read(self.regs.hl());
                2
            }
            // xor (hl)
            0xAE => {
                self.xor(self.bus.read(self.regs.hl()));
                2
            }
            // ld h, d8
            0x26 => {
                self.regs.h = self.fetch_byte();
                2
            }
            // prefix
            0xCB => {
                let suffix_opcode = self.fetch_byte();

                let reg = match suffix_opcode & 0x07 {
                    0x0 => PrefixRegister::B,
                    0x1 => PrefixRegister::C,
                    0x2 => PrefixRegister::D,
                    0x3 => PrefixRegister::E,
                    0x4 => PrefixRegister::H,
                    0x5 => PrefixRegister::L,
                    0x6 => PrefixRegister::LdHl,
                    0x7 => PrefixRegister::A,
                    _ => unreachable!(),
                };

                let reg_value = self.prefix_reg(reg);

                match suffix_opcode & 0xF8 {
                    // rlc r
                    0x00 => {
                        self.set_prefix_reg(reg, reg_value.rotate_left(1));

                        // if reg_value = 0, reg_value.rotate_left(1) is also zero
                        self.regs.set_flag_z(reg_value == 0);
                        self.regs.set_flag_n(false);
                        self.regs.set_flag_h(false);
                        // bit 7
                        self.regs.set_flag_c(reg_value & (1 << 7) != 0);
                        if reg == PrefixRegister::LdHl { 4 } else { 2 }
                    }
                    // rrc r
                    0x08 => {
                        self.set_prefix_reg(reg, reg_value.rotate_right(1));

                        // if reg_value = 0, reg_value.rotate_right(1) is also zero
                        self.regs.set_flag_z(reg_value == 0);
                        self.regs.set_flag_n(false);
                        self.regs.set_flag_h(false);
                        // bit 0
                        self.regs.set_flag_c(reg_value & 0x01 != 0);
                        if reg == PrefixRegister::LdHl { 4 } else { 2 }
                    }
                    // rl r
                    0x10 => {
                        let result = (reg_value << 1) & 0xFE | (self.regs.f & FLAG_C >> 4);
                        self.set_prefix_reg(reg, result);

                        self.regs.set_flag_z(result == 0);
                        self.regs.set_flag_n(false);
                        self.regs.set_flag_h(false);
                        // bit 7
                        self.regs.set_flag_c(reg_value & (1 << 7) != 0);
                        if reg == PrefixRegister::LdHl { 4 } else { 2 }
                    }
                    // rr r
                    0x18 => {
                        let result = (reg_value >> 1) | ((self.regs.f & FLAG_C) << 3);
                        self.set_prefix_reg(reg, result);

                        self.regs.set_flag_z(result == 0);
                        self.regs.set_flag_n(false);
                        self.regs.set_flag_h(false);
                        // bit 0
                        self.regs.set_flag_c(reg_value & 0x01 != 0);
                        if reg == PrefixRegister::LdHl { 4 } else { 2 }
                    }
                    // sla r
                    0x20 => {
                        self.set_prefix_reg(reg, reg_value << 1);

                        self.regs.set_flag_n(reg_value << 1 == 0);
                        self.regs.set_flag_n(false);
                        self.regs.set_flag_h(false);
                        // bit 7
                        self.regs.set_flag_c(reg_value & (1 << 7) != 0);
                        if reg == PrefixRegister::LdHl { 4 } else { 2 }
                    }
                    // sra r
                    0x28 => {
                        // arithmetic right shift
                        let result = (reg_value as i8 >> 1) as u8;
                        self.set_prefix_reg(reg, result);

                        self.regs.set_flag_z(result == 0);
                        self.regs.set_flag_n(false);
                        self.regs.set_flag_h(false);
                        // bit 0
                        self.regs.set_flag_c(reg_value & 0x01 != 0);
                        if reg == PrefixRegister::LdHl { 4 } else { 2 }
                    }
                    // swap r
                    0x30 => {
                        let high_nibble = reg_value & 0xF0;
                        let result = (reg_value << 4) | (high_nibble >> 4);
                        self.set_prefix_reg(reg, result);

                        self.regs.set_flag_z(result == 0);
                        self.regs.set_flag_n(false);
                        self.regs.set_flag_h(false);
                        // bit 0
                        self.regs.set_flag_c(false);
                        if reg == PrefixRegister::LdHl { 4 } else { 2 }
                    }
                    // srl r
                    0x38 => {
                        self.set_prefix_reg(reg, reg_value >> 1);

                        self.regs.set_flag_z(reg_value >> 1 == 0);
                        self.regs.set_flag_n(false);
                        self.regs.set_flag_h(false);
                        // bit 0
                        self.regs.set_flag_c(reg_value & 0x01 != 0);
                        if reg == PrefixRegister::LdHl { 4 } else { 2 }
                    }
                    // bit 0, r
                    0x40 => self.bit(reg, 0),
                    // bit 1, r
                    0x48 => self.bit(reg, 1),
                    // bit 2, r
                    0x50 => self.bit(reg, 2),
                    // bit 3, r
                    0x58 => self.bit(reg, 3),
                    // bit 4, r
                    0x60 => self.bit(reg, 4),
                    // bit 5, r
                    0x68 => self.bit(reg, 5),
                    // bit 6, r
                    0x70 => self.bit(reg, 6),
                    // bit 7, r
                    0x78 => self.bit(reg, 7),
                    // res 0, r
                    0x80 => self.res(reg, 0),
                    // res 1, r
                    0x88 => self.res(reg, 1),
                    // res 2, r
                    0x90 => self.res(reg, 2),
                    // res 3, r
                    0x98 => self.res(reg, 3),
                    // res 4, r
                    0xA0 => self.res(reg, 4),
                    // res 5, r
                    0xA8 => self.res(reg, 5),
                    // res 7, r
                    0xB0 => self.res(reg, 6),
                    // res 7, r
                    0xB8 => self.res(reg, 7),
                    // set 0, r
                    0xC0 => self.set(reg, 0),
                    // set 1, r
                    0xC8 => self.set(reg, 1),
                    // set 0, r
                    0xD0 => self.set(reg, 2),
                    // set 1, r
                    0xD8 => self.set(reg, 3),
                    // set 0, r
                    0xE0 => self.set(reg, 4),
                    // set 1, r
                    0xE8 => self.set(reg, 5),
                    // set 0, r
                    0xF0 => self.set(reg, 6),
                    // set 1, r
                    0xF8 => self.set(reg, 7),
                    _ => unreachable!(),
                }
            }
            // rra
            0x1F => {
                let prev_c = self.regs.f & FLAG_C;

                // bit 0
                self.regs.set_flag_c(self.regs.a & 0x01 != 0);

                self.regs.a = (self.regs.a >> 1) | (prev_c << 3);

                self.regs.set_flag_z(false);
                self.regs.set_flag_n(false);
                self.regs.set_flag_h(false);

                1
            }
            // jr nc, s8
            0x30 => {
                let offset = self.fetch_byte() as i8 as i16;
                if (self.regs.f & FLAG_C) == 0 {
                    self.regs.pc = self.regs.pc.wrapping_add_signed(offset);
                    3
                } else {
                    2
                }
            }
            // ld e, a
            0x5F => {
                self.regs.e = self.regs.a;
                1
            }
            // xor d8
            0xEE => {
                let value = self.fetch_byte();
                self.xor(value);
                2
            }
            // ld a, c
            0x79 => {
                self.regs.a = self.regs.c;
                1
            }
            // ld c, a
            0x4F => {
                self.regs.c = self.regs.a;
                1
            }
            // ld a, d
            0x7A => {
                self.regs.a = self.regs.d;
                1
            }
            // ld d, a
            0x57 => {
                self.regs.d = self.regs.a;
                1
            }
            // ld a, e
            0x7B => {
                self.regs.a = self.regs.e;
                1
            }
            // dec h
            0x25 => {
                self.dec(GpRegister::H);
                1
            }
            // ld (hl), d
            0x72 => {
                self.bus.write(self.regs.hl(), self.regs.d);
                2
            }
            // ld (hl), c
            0x71 => {
                self.bus.write(self.regs.hl(), self.regs.c);
                2
            }
            // ld (hl), b
            0x70 => {
                self.bus.write(self.regs.hl(), self.regs.b);
                2
            }
            // pop de
            0xD1 => {
                let value = self.pop_word();
                self.regs.set_de(value);
                3
            }
            // adc a, d8
            0xCE => {
                let value = self.fetch_byte();
                self.adc(GpRegister::A, value);
                2
            }
            // ret nc
            0xD0 => {
                if self.regs.f & FLAG_C == 0 {
                    self.ret();
                    5
                } else {
                    2
                }
            }
            // ret z
            0xC8 => {
                if self.regs.f & FLAG_Z != 0 {
                    self.ret();
                    5
                } else {
                    2
                }
            }
            // dec a
            0x3D => {
                self.dec(GpRegister::A);
                1
            }
            // or (hl)
            0xB6 => {
                self.or(self.bus.read(self.regs.hl()));
                2
            }
            // dec (hl)
            0x35 => {
                self.dec(GpRegister::HlMem);
                3
            }
            // ld l, (hl)
            0x6E => {
                self.regs.l = self.bus.read(self.regs.hl());
                2
            }
            // ld l, a
            0x6F => {
                self.regs.l = self.regs.a;
                1
            }
            // add hl, hl
            0x29 => {
                let hl = self.regs.hl();
                let result = self.add16(hl, hl);
                self.regs.set_hl(result);
                2
            }
            // dec e
            0x1D => self.dec(GpRegister::E),
            // jp hl
            0xE9 => {
                self.regs.pc = self.regs.hl();
                1
            }
            // jp nz, a16
            0xC2 => {
                let addr = self.fetch_word();
                if self.regs.f & FLAG_Z == 0 {
                    self.regs.pc = addr;
                    4
                } else {
                    3
                }
            }
            // cp e
            0xBB => {
                self.cp(self.regs.e);
                1
            }
            // inc b
            0x04 => self.inc(GpRegister::B),
            // inc c
            0x0C => self.inc(GpRegister::C),
            // ld h, a
            0x67 => {
                self.regs.h = self.regs.a;
                1
            }
            // daa
            0x27 => {
                if self.regs.f & FLAG_N != 0 {
                    let mut adjustment = 0;
                    if self.regs.f & FLAG_H != 0 {
                        adjustment += 0x06;
                    }
                    if self.regs.f & FLAG_C != 0 {
                        adjustment += 0x60;
                    }

                    self.regs.a = self.regs.a.wrapping_sub(adjustment);
                } else {
                    let mut adjustment = 0;
                    if self.regs.f & FLAG_H != 0 || self.regs.a & 0x0F > 0x09 {
                        adjustment += 0x06;
                    }
                    if self.regs.f & FLAG_C != 0 || self.regs.a > 0x99 {
                        adjustment += 0x60;
                        self.regs.set_flag_c(true);
                    }

                    self.regs.a = self.regs.a.wrapping_add(adjustment);
                }

                self.regs.set_flag_z(self.regs.a == 0);
                self.regs.set_flag_h(false);
                1
            }
            // cp d
            0xBA => {
                self.cp(self.regs.d);
                1
            }
            // cp c
            0xB9 => {
                self.cp(self.regs.c);
                1
            }
            // cp b
            0xB8 => {
                self.cp(self.regs.b);
                1
            }
            _ => panic!(
                "unimplemented opcode: 0x{opcode:02X} at addr {:X}",
                self.regs.pc
            ),
        }
    }

    pub fn step_ppu(&mut self) {
        self.bus.ppu.step();
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
