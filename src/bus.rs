use crate::{cartridge::Cartridge, dma::Dma};

pub const WRAM_LEN: usize = 0x2000;
pub const HRAM_LEN: usize = 0x7F;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaybeInitByte {
    Uninit,
    Init(u8),
}

pub struct Bus {
    pub cartridge: Cartridge,
    pub wram: [MaybeInitByte; WRAM_LEN],
    pub hram: [MaybeInitByte; HRAM_LEN],
    pub dma: Dma,
}

impl Bus {
    pub fn new(cartridge: Cartridge) -> Self {
        Self {
            cartridge,
            wram: [const { MaybeInitByte::Uninit }; WRAM_LEN],
            hram: [const { MaybeInitByte::Uninit }; HRAM_LEN],
            dma: Dma::new(),
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        // suppresses warning for hram pattern
        #[allow(non_contiguous_range_endpoints)]
        match addr {
            // cartridge ROM and RAM
            0x0000..0x8000 | 0xA000..0xC000 => self.cartridge.read(addr),
            // WRAM
            0xC000..0xE000 => match self.wram[(addr as usize) - 0xC000] {
                MaybeInitByte::Uninit => panic!("attempt to read uninit memory"),
                MaybeInitByte::Init(v) => v,
            },
            // HRAM
            0xFF80..0xFFFF => match self.hram[(addr as usize) - 0xFF80] {
                MaybeInitByte::Uninit => panic!("attempt to read uninit memory"),
                MaybeInitByte::Init(v) => v,
            },
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        // suppresses warning for hram pattern
        #[allow(non_contiguous_range_endpoints)]
        match addr {
            // cartridge ROM and RAM
            0x0000..0x8000 | 0xA000..0xC000 => self.cartridge.write(addr, value),
            // WRAM
            0xC000..0xE000 => self.wram[(addr as usize) - 0xC000] = MaybeInitByte::Init(value),
            // OAM DMA
            0xFF46 => {
                self.dma.assign_op((value as u16) * 0x100);
            }
            0xFF80..0xFFFF => self.hram[(addr as usize) - 0xFF80] = MaybeInitByte::Init(value),
            _ => {
                log::warn!("attempt to write value {value:X} to unmapped addr {addr:X}")
            }
        }
    }
}
