use crate::cartridge::Cartridge;

pub const WRAM_LEN: usize = 0x2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaybeInitByte {
    Uninit,
    Init(u8),
}

pub struct Bus {
    pub cartridge: Cartridge,
    pub wram: [MaybeInitByte; WRAM_LEN],
}

impl Bus {
    pub fn new(cartridge: Cartridge) -> Self {
        Self {
            cartridge,
            wram: [const { MaybeInitByte::Uninit }; WRAM_LEN],
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..0x8000 | 0xA000..0xC000 => self.cartridge.read(addr),
            0xC000..0xE000 => match self.wram[(addr as usize) - 0xC000] {
                MaybeInitByte::Uninit => panic!("attempt to read uninit memory"),
                MaybeInitByte::Init(v) => v,
            },
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..0x8000 | 0xA000..0xC000 => self.cartridge.write(addr, value),
            0xC000..0xE000 => self.wram[(addr as usize) - 0xC000] = MaybeInitByte::Init(value),
            _ => {
                log::warn!("attempt to write value {value:X} to unmapped addr {addr:X}")
            }
        }
    }
}
