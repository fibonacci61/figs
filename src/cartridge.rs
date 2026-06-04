use anyhow::bail;
use bytemuck::{Pod, Zeroable};
use log::{info, warn};

const NINTENDO_LOGO: &[u8] = &[
    0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
    0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
    0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
];

pub struct Header<'a> {
    // declared separately from HeaderFlags because its length is variable
    pub title: &'a [u8],
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
    let title = match title_len {
        Some(len) => &title_bytes[..len],
        _ => title_bytes,
    };
    info!(
        "parsed cartridge title: '{}'",
        String::from_utf8_lossy(title)
    );

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
