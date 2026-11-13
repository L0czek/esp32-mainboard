use anyhow::{bail, Context, Result};
use object::{Object, ObjectSection};
use std::{env, fs, path::Path, process::Command};

// The firmware stores a 23-byte marker in `.build_type` (see `build_type_marker` in
// `src/config.rs`): "FOR_FLASHING_V1!RELEASE", "FOR_FLASHING_V1!DEBUG..", or the same with
// an "UNCONFIGURED_V1!" prefix. Only the start is compared here.
const RELEASE_MARKER: &[u8] = b"FOR_FLASHING_V1!R";
const NON_RELEASE_MARKER: &[u8] = b"FOR_FLASHING_V1!D";

#[derive(Debug, PartialEq, Eq)]
enum BuildKind {
    Release,
    NonRelease,
    Unconfigured,
}

fn classify_build_marker(marker: &[u8]) -> BuildKind {
    if marker.starts_with(RELEASE_MARKER) {
        BuildKind::Release
    } else if marker.starts_with(NON_RELEASE_MARKER) {
        BuildKind::NonRelease
    } else {
        BuildKind::Unconfigured
    }
}

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

    match classify_build_marker(marker) {
        BuildKind::Release => {}
        BuildKind::NonRelease => {
            eprintln!("\x1b[33mWarning: firmware was not built with the release profile\x1b[0m");
        }
        BuildKind::Unconfigured => {
            bail!(
                "Firmware was compiled without complete device configuration; \
                 refusing to flash. marker={}",
                String::from_utf8_lossy(marker)
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BuildKind, classify_build_marker};

    #[test]
    fn accepts_the_markers_the_firmware_embeds() {
        assert_eq!(
            classify_build_marker(b"FOR_FLASHING_V1!RELEASE"),
            BuildKind::Release
        );
        assert_eq!(
            classify_build_marker(b"FOR_FLASHING_V1!DEBUG.."),
            BuildKind::NonRelease
        );
    }

    #[test]
    fn rejects_unconfigured_and_unknown_markers() {
        assert_eq!(
            classify_build_marker(b"UNCONFIGURED_V1!RELEASE"),
            BuildKind::Unconfigured
        );
        assert_eq!(
            classify_build_marker(b"UNCONFIGURED_V1!DEBUG.."),
            BuildKind::Unconfigured
        );
        assert_eq!(classify_build_marker(b""), BuildKind::Unconfigured);
    }
}
