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

    use object::write::{Object as WriteObject, Symbol, SymbolSection};
    use object::{
        Architecture, BinaryFormat, Endianness, Object, ObjectSymbol, SectionKind, SymbolFlags,
        SymbolKind, SymbolScope,
    };

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

        let stdout = String::from_utf8(stdout).unwrap();
        let stderr = String::from_utf8(stderr).unwrap();

        assert!(
            stdout.contains("fixture=42"),
            "decoded output missing fixture payload: {stdout:?}"
        );
        assert!(
            stdout.contains("INFO"),
            "decoded output missing log level: {stdout:?}"
        );
        assert_eq!(
            stdout.lines().count(),
            1,
            "expected one decoded log line, got: {stdout:?}"
        );
        assert!(stderr.is_empty(), "unexpected decoder warnings: {stderr:?}");
    }

    fn build_fixture_stream() -> (PathBuf, Vec<u8>) {
        let manifest_path = fixture_manifest_path();
        run_fixture_command(&manifest_path, ["build", "--quiet"]);
        let fixture_dir = manifest_path.parent().unwrap();
        let binary = fixture_binary_path(&manifest_path);

        let output = Command::new(&binary)
            .current_dir(fixture_dir)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");

        let elf = build_decoder_fixture_object(&manifest_path, &output.stdout);
        assert!(elf.exists(), "missing fixture ELF at {}", elf.display());
        assert!(!output.stdout.is_empty(), "fixture produced no defmt bytes");

        (elf, output.stdout)
    }

    fn build_decoder_fixture_object(manifest_path: &Path, stream_bytes: &[u8]) -> PathBuf {
        let fixture_elf = fixture_binary_path(manifest_path);
        let fixture_dir = manifest_path.parent().unwrap();
        let build_dir = fixture_dir.join("target").join("decoder-test");
        let object_path = build_dir.join("defmt-fixture-table.o");

        fs::create_dir_all(&build_dir).unwrap();

        let fixture_bytes = fs::read(&fixture_elf).unwrap();
        let fixture = object::File::parse(&*fixture_bytes).unwrap();
        let symbols = collect_fixture_symbols(&fixture, decode_fixture_frame_index(stream_bytes));
        let section_size = symbols
            .iter()
            .map(|symbol| symbol.value + symbol.size.max(1))
            .max()
            .unwrap() as usize;

        let mut object =
            WriteObject::new(BinaryFormat::Elf, Architecture::X86_64, Endianness::Little);
        let section = object.add_section(Vec::new(), b".defmt".to_vec(), SectionKind::ReadOnlyData);
        object.append_section_data(section, &vec![0; section_size], 1);

        for symbol in symbols {
            object.add_symbol(Symbol {
                name: symbol.name.into_bytes(),
                value: symbol.value,
                size: symbol.size.max(1),
                kind: SymbolKind::Data,
                scope: SymbolScope::Linkage,
                weak: false,
                section: SymbolSection::Section(section),
                flags: SymbolFlags::None,
            });
        }

        fs::write(&object_path, object.write().unwrap()).unwrap();
        object_path
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

    fn collect_fixture_symbols(file: &object::File<'_>, frame_index: u64) -> Vec<FixtureSymbol> {
        file.symbols()
            .filter_map(|symbol| {
                let name = symbol.name().ok()?;
                if !is_required_fixture_symbol(name) {
                    return None;
                }

                Some(FixtureSymbol {
                    name: name.to_owned(),
                    value: if name.contains("\"tag\":\"defmt_info\"") {
                        frame_index
                    } else {
                        symbol.address()
                    },
                    size: symbol.size(),
                })
            })
            .collect()
    }

    fn is_required_fixture_symbol(name: &str) -> bool {
        name.contains("\"tag\":\"defmt_info\"")
            || name.starts_with("_defmt_encoding_ = ")
            || name.starts_with("_defmt_version_ = ")
    }

    fn decode_fixture_frame_index(stream_bytes: &[u8]) -> u64 {
        let frame = decode_rzcobs_frame(stream_bytes);
        u16::from_le_bytes([frame[0], frame[1]]) as u64
    }

    fn decode_rzcobs_frame(stream_bytes: &[u8]) -> Vec<u8> {
        let start = stream_bytes
            .iter()
            .position(|byte| *byte != 0)
            .expect("fixture stream missing frame data");
        let end = stream_bytes[start..]
            .iter()
            .position(|byte| *byte == 0)
            .map(|offset| start + offset)
            .expect("fixture stream missing frame terminator");

        let mut decoded = Vec::new();
        let mut encoded = stream_bytes[start..end].iter().rev().copied();

        while let Some(byte) = encoded.next() {
            if byte == 0 {
                panic!("fixture rzCOBS frame contained an unexpected zero byte");
            }

            if byte <= 0x7f {
                for bit in 0..7 {
                    if byte & (1 << (6 - bit)) == 0 {
                        decoded.push(encoded.next().expect("fixture rzCOBS frame truncated"));
                    } else {
                        decoded.push(0);
                    }
                }
                continue;
            }

            if byte < 0xff {
                let count = (byte & 0x7f) + 7;
                decoded.push(0);
                for _ in 0..count {
                    decoded.push(encoded.next().expect("fixture rzCOBS frame truncated"));
                }
                continue;
            }

            for _ in 0..134 {
                decoded.push(encoded.next().expect("fixture rzCOBS frame truncated"));
            }
        }

        decoded.reverse();
        assert!(
            decoded.len() >= 2,
            "fixture rzCOBS frame too short to contain a defmt index"
        );
        decoded
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

    struct FixtureSymbol {
        name: String,
        value: u64,
        size: u64,
    }
}
