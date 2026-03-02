mod decoder;
mod packet;

use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write, stdout};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use crate::decoder::PacketDecoder;

/// Blackbox SD card tool — decode binary data or format cards.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Decode raw blackbox binary data into NDJSON.
    Decode {
        /// Path to the raw SD card image or binary file.
        input: PathBuf,

        /// Experiment separator byte in hex (e.g. "aa" or "0xAA").
        #[arg(long, default_value = "aa", value_parser = parse_hex_byte)]
        separator: u8,
    },
    /// Zero-fill an SD card or image file for a new experiment.
    Format {
        /// Path to the SD card block device or image file.
        device: PathBuf,

        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,

        /// Stop at the first chunk that is already all zeros.
        #[arg(long)]
        quick: bool,
    },
}

fn parse_hex_byte(s: &str) -> Result<u8, String> {
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u8::from_str_radix(s, 16).map_err(|e| format!("invalid hex byte: {e}"))
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Decode { input, separator } => cmd_decode(&input, separator),
        Command::Format { device, yes, quick } => cmd_format(&device, yes, quick),
    }
}

fn cmd_decode(input: &PathBuf, separator: u8) -> Result<()> {
    if separator == 0x00 || (0x01..=0x05).contains(&separator) {
        bail!(
            "separator byte {separator:#04x} conflicts with padding (0x00) \
             or packet IDs (0x01-0x05)",
        );
    }

    let file = File::open(input).with_context(|| format!("failed to open {input:?}"))?;
    let reader = BufReader::with_capacity(64 * 1024, file);
    let mut decoder = PacketDecoder::new(reader, separator);

    let out = stdout().lock();
    let mut writer = BufWriter::new(out);
    let mut count: u64 = 0;

    while let Some(packet) = decoder.next_packet()? {
        serde_json::to_writer(&mut writer, &packet)?;
        writer.write_all(b"\n")?;
        count += 1;
    }

    writer.flush()?;
    eprintln!("decoded {count} packets");
    Ok(())
}

const CHUNK_SIZE: usize = 1024 * 1024;

fn cmd_format(device: &PathBuf, yes: bool, quick: bool) -> Result<()> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(device)
        .with_context(|| format!("failed to open {device:?} for read/write"))?;

    let size = file
        .seek(SeekFrom::End(0))
        .with_context(|| format!("failed to determine size of {device:?}"))?;
    if size == 0 {
        bail!("{device:?} reports size 0 — is this the right device?");
    }
    file.seek(SeekFrom::Start(0))?;

    let size_mb = size / (1024 * 1024);
    let mode = if quick { "quick-format" } else { "full format" };
    eprintln!("device: {device:?} ({size_mb} MB, {mode})");

    if !yes {
        eprint!("this will zero-fill the device. continue? [y/N] ");
        std::io::stderr().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !answer.trim().eq_ignore_ascii_case("y") {
            bail!("aborted");
        }
    }

    let zeros = vec![0u8; CHUNK_SIZE];
    let mut written: u64 = 0;

    while written < size {
        let remaining = (size - written) as usize;
        let n = remaining.min(CHUNK_SIZE);

        if quick {
            let mut buf = vec![0u8; n];
            file.read_exact(&mut buf)
                .with_context(|| format!("read failed at offset {written}"))?;
            if buf.iter().all(|&b| b == 0) {
                break;
            }
            file.seek(SeekFrom::Start(written))?;
        }

        file.write_all(&zeros[..n])
            .with_context(|| format!("write failed at offset {written}"))?;
        written += n as u64;

        let pct = written * 100 / size;
        let written_mb = written / (1024 * 1024);
        eprint!("\r{written_mb}/{size_mb} MB ({pct}%)");
    }

    file.flush()?;
    file.sync_all()?;
    let written_mb = written / (1024 * 1024);
    eprintln!("\rformatted {written_mb} MB          ");
    Ok(())
}
