use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use defmt_decoder::{DecodeError, StreamDecoder, Table};

use crate::mqtt::PayloadHandler;

pub struct DefmtStreamDecoder {
    can_recover: bool,
    stream: Box<dyn StreamDecoder + Send + Sync + 'static>,
}

impl DefmtStreamDecoder {
    pub fn from_elf(elf: &Path) -> Result<Self> {
        let bytes = fs::read(elf).with_context(|| format!("failed to read {}", elf.display()))?;
        let table = Table::parse(&bytes)?.ok_or_else(|| anyhow!(".defmt data not found"))?;
        let can_recover = table.encoding().can_recover();
        let leaked = Box::leak(Box::new(table));
        let stream = leaked.new_stream_decoder();

        Ok(Self {
            can_recover,
            stream,
        })
    }

    pub fn process_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.stream.received(bytes);

        loop {
            match self.stream.decode() {
                Ok(frame) => println!("{}", frame.display(false)),
                Err(DecodeError::UnexpectedEof) => break,
                Err(DecodeError::Malformed) if self.can_recover => {
                    eprintln!("warning: malformed frame skipped");
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
