#[cfg(not(target_arch = "wasm32"))]
use std::fs;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
use anyhow::Context;
use anyhow::{Result, anyhow};
use defmt_decoder::{DecodeError, StreamDecoder, Table};

const MALFORMED_FRAME_WARNING: &str = "warning: malformed frame skipped";

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

/// Output produced after feeding one MQTT payload into the decoder.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DecodedChunk {
    /// Formatted log lines that became complete after this payload.
    pub lines: Vec<String>,
    /// Recoverable decoder warnings emitted while skipping malformed frames.
    pub warnings: Vec<String>,
}

impl DecodedChunk {
    /// Returns whether this payload completed no lines and emitted no warnings.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.warnings.is_empty()
    }
}

/// Incremental `defmt` stream decoder for raw MQTT payload bytes.
pub struct DefmtStreamDecoder {
    can_recover: bool,
    stream: Box<dyn DecoderBackend + Send + Sync + 'static>,
}

impl DefmtStreamDecoder {
    /// Builds a decoder from firmware ELF bytes.
    ///
    /// Args:
    /// - `elf`: Full firmware ELF contents containing the `.defmt` section.
    ///
    /// Returns:
    /// - A decoder that can accept MQTT payload chunks in stream order.
    ///
    /// Errors:
    /// - Returns an error when the ELF cannot be parsed or does not contain `.defmt` metadata.
    pub fn from_elf_bytes(elf: &[u8]) -> Result<Self> {
        let table = Table::parse(elf)?.ok_or_else(|| anyhow!(".defmt data not found"))?;
        Ok(Self::from_table(table))
    }

    #[cfg(not(target_arch = "wasm32"))]
    /// Builds a decoder from a firmware ELF on disk.
    ///
    /// Args:
    /// - `elf`: Path to the firmware ELF that matches the incoming `defmt` stream.
    ///
    /// Errors:
    /// - Returns an error when the file cannot be read or does not contain `.defmt` metadata.
    pub fn from_elf(elf: &Path) -> Result<Self> {
        let bytes = fs::read(elf).with_context(|| format!("failed to read {}", elf.display()))?;
        Self::from_elf_bytes(&bytes)
    }

    /// Feeds one raw MQTT payload chunk into the decoder.
    ///
    /// Args:
    /// - `bytes`: Raw `defmt` stream bytes taken from one MQTT publish payload.
    ///
    /// Returns:
    /// - The decoded lines and recoverable warnings produced by this chunk.
    ///
    /// Errors:
    /// - Returns an error when the stream is malformed and the encoding cannot recover.
    pub fn decode_chunk(&mut self, bytes: &[u8]) -> Result<DecodedChunk> {
        let mut output = DecodedChunk::default();
        self.stream.received(bytes);

        loop {
            match self.stream.decode_frame() {
                Ok(Some(frame)) => output.lines.push(frame),
                Ok(None) => return Ok(output),
                Err(DecodeError::Malformed) if self.can_recover => {
                    output.warnings.push(String::from(MALFORMED_FRAME_WARNING));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn from_table(table: Table) -> Self {
        let can_recover = table.encoding().can_recover();
        let leaked = Box::leak(Box::new(table));
        let stream = Box::new(LiveDecoderBackend {
            stream: leaked.new_stream_decoder(),
        });

        Self {
            can_recover,
            stream,
        }
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
        frames: VecDeque<Result<Option<String>, DecodeError>>,
    }

    impl FakeDecoderBackend {
        fn new(frames: impl IntoIterator<Item = Result<Option<String>, DecodeError>>) -> Self {
            Self {
                frames: frames.into_iter().collect(),
            }
        }
    }

    impl DecoderBackend for FakeDecoderBackend {
        fn received(&mut self, _bytes: &[u8]) {}

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

        let first = decoder.decode_chunk(&[1, 2]).unwrap();
        let second = decoder.decode_chunk(&[3, 4]).unwrap();

        assert!(first.is_empty());
        assert_eq!(second.lines, vec![String::from("frame 1")]);
        assert!(second.warnings.is_empty());
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

        let chunk = decoder.decode_chunk(&[9, 9]).unwrap();

        assert_eq!(chunk.lines, vec![String::from("frame 2")]);
        assert_eq!(chunk.warnings, vec![String::from(MALFORMED_FRAME_WARNING)]);
    }

    #[test]
    fn returns_malformed_error_when_stream_cannot_recover() {
        let backend = FakeDecoderBackend::new([Err(DecodeError::Malformed)]);
        let mut decoder = DefmtStreamDecoder {
            can_recover: false,
            stream: Box::new(backend),
        };

        let error = decoder.decode_chunk(&[7]).unwrap_err();
        let decode_error = error.downcast_ref::<DecodeError>();

        assert!(matches!(decode_error, Some(DecodeError::Malformed)));
    }

    #[test]
    fn accepts_real_fixture_stream_from_matching_elf_without_decode_errors() {
        let (elf, bytes) = build_fixture_stream();
        let elf_bytes = fs::read(&elf).unwrap();
        let mut decoder = DefmtStreamDecoder::from_elf_bytes(&elf_bytes).unwrap();
        let split = bytes.len() / 2;

        let first = decoder.decode_chunk(&bytes[..split]).unwrap();
        let second = decoder.decode_chunk(&bytes[split..]).unwrap();

        let lines = first
            .lines
            .into_iter()
            .chain(second.lines)
            .collect::<Vec<_>>();
        let rendered = lines.join("\n");

        assert!(
            rendered.contains("fixture=42"),
            "decoded output missing fixture payload: {rendered:?}"
        );
        assert!(
            rendered.contains("INFO"),
            "decoded output missing log level: {rendered:?}"
        );
        assert_eq!(
            lines.len(),
            1,
            "expected one decoded log line, got: {lines:?}"
        );
        assert!(first.warnings.is_empty());
        assert!(second.warnings.is_empty());
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
