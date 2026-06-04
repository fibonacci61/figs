mod cartridge;

use std::path::PathBuf;

use clap::Parser;

use crate::cartridge::Cartridge;

#[derive(Parser)]
#[command(version, about)]
struct Figs {
    /// Path to Game Boy ROM
    rom_path: PathBuf,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let figs = Figs::parse();
    let rom = std::fs::read(&figs.rom_path)?;
    let _cart = Cartridge::new(rom)?;

    Ok(())
}
