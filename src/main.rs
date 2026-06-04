mod cartridge;

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(version, about)]
struct Figs {
    /// Path to Game Boy ROM
    rom_path: PathBuf,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let figs = Figs::parse();
    let rom_bytes = std::fs::read(&figs.rom_path)?;
    let _header = cartridge::parse_rom_header(&rom_bytes)?;

    Ok(())
}
