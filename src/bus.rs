use crate::cartridge::Cartridge;

pub struct Bus {
    pub cartridge: Cartridge,
}

impl Bus {
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..0x8000 | 0xA000..0xC000 => self.cartridge.read(addr),
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..0x8000 | 0xA000..0xC000 => self.cartridge.write(addr, value),
            _ => {}
        }
    }
}
