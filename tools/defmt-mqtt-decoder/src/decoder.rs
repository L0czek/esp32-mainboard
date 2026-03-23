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
}
