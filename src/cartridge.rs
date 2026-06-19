use anyhow::bail;
use bytemuck::{Pod, Zeroable};
use log::{info, warn};

const NINTENDO_LOGO: &[u8] = &[
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

pub struct Cartridge {
    mbc: Box<dyn Mbc>,
    title: String,
    header_flags: HeaderFlags,
}

pub struct Header<'a> {
    // declared separately from HeaderFlags because its length is variable
    pub title: &'a str,
    pub header_flags: &'a HeaderFlags,
}

#[derive(Debug, Pod, Zeroable, Clone, Copy)]
#[repr(C)]
pub struct HeaderFlags {
    pub new_licensee_code: [u8; 2],
    pub sgb_flag: u8,
    pub cartridge_type: u8,
    pub rom_size: u8,
    pub ram_size: u8,
    pub destination_code: u8,
    pub old_licensee_code: u8,
    pub mask_rom_version_number: u8,
    pub header_checksum: u8,
    pub global_checksum: [u8; 2],
}

pub fn parse_rom_header<'a>(rom: &'a [u8]) -> anyhow::Result<Header<'a>> {
    if &rom[0x104..0x134] != NINTENDO_LOGO {
        bail!("invalid logo in header");
    }

    // handle CGB flag
    let title_bytes = match rom[0x143] {
        // backwards compatible with DMG, OK
        0x80 => &rom[0x134..0x143],
        // CGB only, print warning
        0xC0 => {
            warn!("cartridge is marked as CGB-only, running anyway");
            &rom[0x134..0x143]
        }
        // anything else => part of the title, not a flag
        _ => &rom[0x134..0x144],
    };

    // trim title_bytes based
    let title_len = title_bytes.iter().position(|v| *v == 0);
    let trimmed_title = match title_len {
        Some(len) => &title_bytes[..len],
        _ => title_bytes,
    };
    if !trimmed_title.is_ascii() {
        bail!("ROM title contains invalid UTF-8");
    }

    // all ASCII is valid UTF-8
    let title = str::from_utf8(trimmed_title).unwrap();

    info!("parsed cartridge title: '{}'", title);

    // parse the rest of the flags instantaneously since they
    let header_flags = bytemuck::from_bytes::<HeaderFlags>(&rom[0x144..0x150]);

    let mut checksum = 0u8;
    for byte in &rom[0x134..0x14D] {
        checksum = checksum.wrapping_sub(*byte).wrapping_sub(1);
    }

    if checksum != header_flags.header_checksum {
        bail!(
            "checksum mismatch: computed checksum (0x{checksum:02X}) expected checksum (0x{:02X})",
            header_flags.header_checksum
        );
    }

    info!("header flags: {:02X?}", header_flags);

    Ok(Header {
        title,
        header_flags,
    })
}

const RAM_ENABLE_NIBBLE: u8 = 0xA;

pub trait Mbc {
    fn read(&self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, value: u8);
}

struct NoMbc {
    rom: Vec<u8>,
    ram: Option<Vec<u8>>,
}

impl NoMbc {
    const ROM_START: u16 = 0x0000;
    const ROM_END: u16 = 0x8000;
    const RAM_START: u16 = 0xA000;
    const RAM_END: u16 = 0xC000;

    fn new(rom: Vec<u8>, ram: bool) -> Self {
        Self {
            rom,
            ram: if ram { Some(Vec::new()) } else { None },
        }
    }
}

impl Mbc for NoMbc {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            Self::ROM_START..Self::ROM_END => self.rom[addr as usize],
            Self::RAM_START..Self::RAM_END if let Some(ram) = self.ram.as_ref() => {
                ram[addr as usize]
            }
            _ => panic!("out of range cartridge read (0x{addr:X})"),
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            Self::ROM_START..Self::ROM_END => {}
            Self::RAM_START..Self::RAM_END if let Some(ram) = self.ram.as_mut() => {
                ram[addr as usize] = value
            }
            _ => panic!("out of range cartridge write (0x{addr:X})"),
        }
    }
}

struct Mbc1 {
    rom: Vec<u8>,
    ram: Option<Vec<u8>>,

    // control registers
    ram_enabled: bool,
    rom_bank_number: u8,
    secondary_bank: u8,
    banking_mode: u8,

    // from HeaderFlags
    rom_size_flag: u8,
    _ram_size_flag: u8,
    // if computed rom size > 512 KiB
    is_large_rom: bool,
    // if computed ram size > 8 KiB (0x2000, same size as 0xA000..0xC000)
    is_large_ram: bool,
}

impl Mbc1 {
    const BANK_SIZE: usize = 0x4000;

    fn new(rom: Vec<u8>, ram: bool, rom_size_flag: u8, _ram_size_flag: u8) -> Self {
        Self {
            rom,
            ram: if ram { Some(Vec::new()) } else { None },

            ram_enabled: false,
            rom_bank_number: 1,
            secondary_bank: 0,
            banking_mode: 0,

            rom_size_flag,
            _ram_size_flag,
            is_large_rom: rom_size_flag >= 5,
            is_large_ram: _ram_size_flag >= 2,
        }
    }

    fn rom_bank_base_addr(&self) -> usize {
        (self.rom_bank_number as usize) * Self::BANK_SIZE
    }

    fn rom0_effective_addr(&self, addr: u16) -> usize {
        debug_assert!((0x0000..0x4000).contains(&addr));
        if self.banking_mode == 1 && self.is_large_rom {
            ((self.secondary_bank as usize) << 19) | addr as usize
        } else {
            addr as usize
        }
    }

    fn rom1_effective_addr(&self, addr: u16) -> usize {
        debug_assert!((0x4000..0x8000).contains(&addr));
        self.rom_bank_base_addr() + (addr as usize - 0x4000)
    }

    fn ram_effective_addr(&self, addr: u16) -> usize {
        debug_assert!((0xA000..0xC000).contains(&addr));
        if self.banking_mode == 1 && self.is_large_ram {
            ((self.secondary_bank as usize) << 13) | addr as usize
        } else {
            addr as usize
        }
    }
}

impl Mbc for Mbc1 {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..0x4000 => self.rom[self.rom0_effective_addr(addr)],
            0x4000..0x8000 => self.rom[self.rom1_effective_addr(addr)],
            0xA000..0xC000 if let Some(ram) = self.ram.as_ref() => {
                if self.ram_enabled {
                    ram[self.ram_effective_addr(addr)]
                } else {
                    panic!("attempt to read from cartridge ram before enable")
                }
            }
            _ => panic!("out of range cartridge read (0x{addr:X})"),
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        // needs to be computed in advance for the 0xA000-0xC000 case because it mutably borrows
        // self in the binding to `ram`, while this method requires an immutable reference
        let effective_addr = self.ram_effective_addr(addr);
        match addr {
            // RAM enable register
            0x0000..0x2000 => {
                // check if the lower nibble is equal to `RAM_ENABLE_NIBBLE`
                if (value & 0x0F) == RAM_ENABLE_NIBBLE {
                    self.ram_enabled = true;
                } else {
                    self.ram_enabled = false;
                }
            }
            // ROM bank number register, 5 bits
            0x2000..0x4000 => {
                // first 5 bits only
                let mut bank_number = value & 0x1F;
                if bank_number == 0 {
                    bank_number = 1;
                }

                // if the rom is too small to need these bits, they will be masked out in the next
                // step
                bank_number |= self.secondary_bank << 5;

                // mask bank number to only the amount of bits required to represent every possible
                // bank number within the limits of the rom size flag
                let required_bits = self.rom_size_flag + 1;
                let mask = 0xFF >> (8 - required_bits);
                bank_number &= mask;

                self.rom_bank_number = bank_number;
            }
            // Secondary ROM bank register, 3 bits
            0x4000..0x6000 => {
                self.secondary_bank = value & 0b111;
            }
            // Banking mode register, 1 bit
            0x6000..0x8000 => {
                // is this mask actually correct??
                self.banking_mode = value & 0b1;
            }
            // RAM write
            0xA000..0xC000 if let Some(ram) = self.ram.as_mut() => {
                if self.ram_enabled {
                    ram[effective_addr] = value;
                } else {
                    panic!("attempt to write to cartridge while ram is disabled")
                }
            }
            _ => panic!("address (0x{addr:04X}) out of range for cartridge write"),
        }
    }
}

impl Cartridge {
    pub fn new(rom: Vec<u8>) -> anyhow::Result<Self> {
        let header = parse_rom_header(&rom)?;

        let title = header.title.to_string();
        let header_flags = *header.header_flags;

        let mbc: Box<dyn Mbc> = match header.header_flags.cartridge_type {
            // ROM ONLY
            0x00 => Box::new(NoMbc::new(rom, false)),
            // MBC1
            0x01 => Box::new(Mbc1::new(
                rom,
                false,
                header_flags.rom_size,
                header_flags.ram_size,
            )),
            // MBC1+RAM
            0x02 => Box::new(Mbc1::new(
                rom,
                true,
                header_flags.rom_size,
                header_flags.ram_size,
            )),
            // ROM+RAM
            0x08 => Box::new(NoMbc::new(rom, true)),
            v => bail!("unsupported cartridge type 0x{v:X}"),
        };

        Ok(Self {
            mbc,
            title,
            header_flags,
        })
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn header_flags(&self) -> &HeaderFlags {
        &self.header_flags
    }

    pub fn read(&self, addr: u16) -> u8 {
        self.mbc.read(addr)
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        self.mbc.write(addr, value);
    }
}
