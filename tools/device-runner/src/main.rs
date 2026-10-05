use anyhow::{bail, Context, Result};
use object::{Object, ObjectSection};
use std::{env, fs, path::Path, process::Command};

const RELEASE_MARKER: &[u8] = b"FOR_FLASHING_V1!R";
const NON_RELEASE_MARKER: &[u8] = b"FOR_FLASHING_V1!D";

fn main() -> Result<()> {
    let firmware = env::args()
        .nth(1)
        .context("Cargo did not provide firmware ELF path")?;
    let firmware = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(firmware);

    verify_build_marker(&firmware)?;

    let status = Command::new("probe-rs")
        .args([
            "run",
            "--chip=esp32c6",
            "--preverify",
            "--always-print-stacktrace",
            "--no-location",
            "--catch-hardfault",
        ])
        .arg(&firmware)
        .status()
        .context("failed to start probe-rs")?;

    if !status.success() {
        bail!("probe-rs failed");
    }

    Ok(())
}

fn verify_build_marker(path: &Path) -> Result<()> {
    let data = fs::read(path).context("failed to read firmware ELF")?;

    let elf = object::File::parse(&*data).context("firmware is not a valid ELF file")?;

    let section = elf
        .section_by_name(".build_type")
        .context("Firmware has no .build_type section; refusing to flash")?;

    let marker = section
        .data()
        .context("failed to read .build_type section")?;

    match marker {
        RELEASE_MARKER => {}
        NON_RELEASE_MARKER => {
            eprintln!("\x1b[33mWarning: firmware was not built with the release profile\x1b[0m");
        }
        marker => {
            bail!(
                "Firmware was compiled without complete device configuration; \
                 refusing to flash. marker={}",
                String::from_utf8_lossy(marker)
            );
        }
    }

    Ok(())
}
