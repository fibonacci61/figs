use crate::bus::Bus;

#[derive(Default)]
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
}

impl Cpu {
    pub fn new(bus: Bus) -> Self {
        Self {
            regs: Registers {
                pc: 0x0100,
                ..Default::default()
            },
            bus,
        }
    }

    pub fn next(&mut self) {
        let opcode = self.bus.read(self.regs.pc);
        self.regs.pc += 1;
        match opcode {
            // nop
            0x00 => {}
            _ => panic!("unimplemented opcode: 0x{opcode:02X}"),
        }
    }
}
