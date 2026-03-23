use std::fs;
use std::io::{self, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use defmt_decoder::{DecodeError, StreamDecoder, Table};

use crate::mqtt::PayloadHandler;

trait DecoderBackend {
    fn received(&mut self, bytes: &[u8]);
    fn decode_frame(&mut self) -> Result<Option<String>, DecodeError>;
}

struct LiveDecoderBackend {
    stream: Box<dyn StreamDecoder + Send + Sync + 'static>,
}

impl DecoderBackend for LiveDecoderBackend {
    fn received(&mut self, bytes: &[u8]) {
        self.stream.received(bytes);
    }

    fn decode_frame(&mut self) -> Result<Option<String>, DecodeError> {
        match self.stream.decode() {
            Ok(frame) => Ok(Some(frame.display(false).to_string())),
            Err(DecodeError::UnexpectedEof) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

pub struct DefmtStreamDecoder {
    can_recover: bool,
    stream: Box<dyn DecoderBackend + Send + Sync + 'static>,
}

impl DefmtStreamDecoder {
    pub fn from_elf(elf: &Path) -> Result<Self> {
        let bytes = fs::read(elf).with_context(|| format!("failed to read {}", elf.display()))?;
        let table = Table::parse(&bytes)?.ok_or_else(|| anyhow!(".defmt data not found"))?;
        let can_recover = table.encoding().can_recover();
        let leaked = Box::leak(Box::new(table));
        let stream = Box::new(LiveDecoderBackend {
            stream: leaked.new_stream_decoder(),
        });

        Ok(Self {
            can_recover,
            stream,
        })
    }

    pub fn process_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        let mut stdout = io::stdout();
        let mut stderr = io::stderr();
        self.process_bytes_with(bytes, &mut stdout, &mut stderr)
    }

    fn process_bytes_with(
        &mut self,
        bytes: &[u8],
        stdout: &mut impl Write,
        stderr: &mut impl Write,
    ) -> Result<()> {
        self.stream.received(bytes);

        loop {
            match self.stream.decode_frame() {
                Ok(Some(frame)) => writeln!(stdout, "{frame}")?,
                Ok(None) => break,
                Err(DecodeError::Malformed) if self.can_recover => {
                    writeln!(stderr, "warning: malformed frame skipped")?;
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
        }

        Ok(())
    }
}

impl PayloadHandler for DefmtStreamDecoder {
    fn handle_payload(&mut self, payload: &[u8]) -> Result<()> {
        self.process_bytes(payload)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use object::{Object, ObjectSection};

    use super::*;

    struct FakeDecoderBackend {
        received_payloads: Vec<Vec<u8>>,
        frames: VecDeque<Result<Option<String>, DecodeError>>,
    }

    impl FakeDecoderBackend {
        fn new(frames: impl IntoIterator<Item = Result<Option<String>, DecodeError>>) -> Self {
            Self {
                received_payloads: Vec::new(),
                frames: frames.into_iter().collect(),
            }
        }
    }

    impl DecoderBackend for FakeDecoderBackend {
        fn received(&mut self, bytes: &[u8]) {
            self.received_payloads.push(bytes.to_vec());
        }

        fn decode_frame(&mut self) -> Result<Option<String>, DecodeError> {
            self.frames.pop_front().unwrap_or(Ok(None))
        }
    }

    #[test]
    fn waits_for_more_bytes_until_frame_is_complete() {
        let backend =
            FakeDecoderBackend::new([Ok(None), Ok(Some(String::from("frame 1"))), Ok(None)]);
        let mut decoder = DefmtStreamDecoder {
            can_recover: true,
            stream: Box::new(backend),
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        decoder
            .process_bytes_with(&[1, 2], &mut stdout, &mut stderr)
            .unwrap();
        decoder
            .process_bytes_with(&[3, 4], &mut stdout, &mut stderr)
            .unwrap();

        assert_eq!(String::from_utf8(stdout).unwrap(), "frame 1\n");
        assert!(stderr.is_empty());
    }

    #[test]
    fn skips_malformed_frames_when_stream_can_recover() {
        let backend = FakeDecoderBackend::new([
            Err(DecodeError::Malformed),
            Ok(Some(String::from("frame 2"))),
            Ok(None),
        ]);
        let mut decoder = DefmtStreamDecoder {
            can_recover: true,
            stream: Box::new(backend),
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        decoder
            .process_bytes_with(&[9, 9], &mut stdout, &mut stderr)
            .unwrap();

        assert_eq!(String::from_utf8(stdout).unwrap(), "frame 2\n");
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "warning: malformed frame skipped\n"
        );
    }

    #[test]
    fn returns_malformed_error_when_stream_cannot_recover() {
        let backend = FakeDecoderBackend::new([Err(DecodeError::Malformed)]);
        let mut decoder = DefmtStreamDecoder {
            can_recover: false,
            stream: Box::new(backend),
        };

        let error = decoder
            .process_bytes_with(&[7], &mut Vec::new(), &mut Vec::new())
            .unwrap_err();

        let decode_error = error.downcast_ref::<DecodeError>();
        assert!(matches!(decode_error, Some(DecodeError::Malformed)));
    }

    #[test]
    fn accepts_real_fixture_stream_from_matching_elf_without_decode_errors() {
        let (elf, bytes) = build_fixture_stream();
        let mut decoder = DefmtStreamDecoder::from_elf(&elf).unwrap();
        let split = bytes.len() / 2;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        decoder
            .process_bytes_with(&bytes[..split], &mut stdout, &mut stderr)
            .unwrap();
        decoder
            .process_bytes_with(&bytes[split..], &mut stdout, &mut stderr)
            .unwrap();
    }

    fn build_fixture_stream() -> (PathBuf, Vec<u8>) {
        let manifest_path = fixture_manifest_path();
        run_fixture_command(&manifest_path, ["build", "--quiet"]);
        let fixture_dir = manifest_path.parent().unwrap();

        let output = Command::new(cargo_bin())
            .args(["run", "--quiet", "--manifest-path"])
            .arg(&manifest_path)
            .current_dir(fixture_dir)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");

        let elf = build_decoder_fixture_elf(&manifest_path);
        assert!(elf.exists(), "missing fixture ELF at {}", elf.display());
        assert!(!output.stdout.is_empty(), "fixture produced no defmt bytes");

        (elf, output.stdout)
    }

    fn build_decoder_fixture_elf(manifest_path: &Path) -> PathBuf {
        let fixture_elf = fixture_binary_path(manifest_path);
        let fixture_dir = manifest_path.parent().unwrap();
        let build_dir = fixture_dir.join("target").join("decoder-test");
        let merged_elf = build_dir.join("defmt-fixture-with-section");
        let defmt_path = build_dir.join("defmt.bin");

        fs::create_dir_all(&build_dir).unwrap();

        let fixture_bytes = fs::read(&fixture_elf).unwrap();
        let defmt_object = extract_defmt_object(&build_dir);
        let defmt_bytes = fs::read(&defmt_object).unwrap();
        let mut merged = Vec::new();
        append_matching_sections(&defmt_bytes, &mut merged, |name| {
            name.starts_with(".defmt.prim.")
        });
        append_matching_sections(&fixture_bytes, &mut merged, |name| {
            name.starts_with(".defmt.info.")
        });
        append_matching_sections(&defmt_bytes, &mut merged, |name| {
            name.starts_with(".defmt.") && !name.starts_with(".defmt.prim.") && name != ".defmt.end"
        });
        append_matching_sections(&defmt_bytes, &mut merged, |name| name == ".defmt.end");
        append_matching_sections(&fixture_bytes, &mut merged, |name| name == ".defmt.end");
        fs::write(&defmt_path, merged).unwrap();
        fs::copy(&fixture_elf, &merged_elf).unwrap();

        let status = Command::new("objcopy")
            .args(["--add-section", ".defmt=target/decoder-test/defmt.bin"])
            .args(["--set-section-flags", ".defmt=alloc,readonly,contents"])
            .arg("target/decoder-test/defmt-fixture-with-section")
            .current_dir(fixture_dir)
            .status()
            .unwrap();
        assert!(status.success(), "objcopy failed to add .defmt section");

        merged_elf
    }

    fn run_fixture_command(manifest_path: &Path, args: [&str; 2]) {
        let fixture_dir = manifest_path.parent().unwrap();
        let status = Command::new(cargo_bin())
            .args(args)
            .arg("--manifest-path")
            .arg(manifest_path)
            .current_dir(fixture_dir)
            .status()
            .unwrap();
        assert!(status.success(), "fixture cargo command failed");
    }

    fn cargo_bin() -> String {
        env::var("CARGO").unwrap_or_else(|_| String::from("cargo"))
    }

    fn extract_defmt_object(build_dir: &Path) -> PathBuf {
        let defmt_rlib = build_dir
            .parent()
            .unwrap()
            .join("x86_64-unknown-linux-gnu")
            .join("debug")
            .join("deps");
        let defmt_rlib = fs::read_dir(defmt_rlib)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.starts_with("libdefmt-") && name.ends_with(".rlib"))
                    .unwrap_or(false)
            })
            .unwrap();
        let member_name = String::from_utf8(
            Command::new("ar")
                .args(["t", defmt_rlib.to_str().unwrap()])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .lines()
        .nth(1)
        .unwrap()
        .to_owned();
        let object_path = build_dir.join("defmt-object.o");
        let object_bytes = Command::new("ar")
            .args(["p", defmt_rlib.to_str().unwrap(), &member_name])
            .output()
            .unwrap()
            .stdout;
        fs::write(&object_path, object_bytes).unwrap();

        object_path
    }

    fn append_matching_sections(
        object_bytes: &[u8],
        out: &mut Vec<u8>,
        mut predicate: impl FnMut(&str) -> bool,
    ) {
        let file = object::File::parse(object_bytes).unwrap();

        for section in file.sections() {
            let Ok(name) = section.name() else {
                continue;
            };
            if !predicate(name) {
                continue;
            }

            out.extend(section.data().unwrap());
        }
    }

    fn fixture_manifest_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("defmt-fixture")
            .join("Cargo.toml")
    }

    fn fixture_binary_path(manifest_path: &Path) -> PathBuf {
        manifest_path
            .parent()
            .unwrap()
            .join("target")
            .join("x86_64-unknown-linux-gnu")
            .join("debug")
            .join("defmt-fixture")
    }
}
