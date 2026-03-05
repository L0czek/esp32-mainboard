use std::io::Read;

use crate::packet::{
    ID_DIGITAL, ID_FAST_ADC, ID_PADDING, ID_SERVO, ID_SLOW_ADC, ID_TEMPERATURE, ID_TIMING_SYNC,
    PacketData,
};

pub type Result<T> = std::result::Result<T, DecodeError>;

#[derive(Debug)]
pub enum DecodeError {
    Io {
        op: &'static str,
        source: std::io::Error,
    },
    TruncatedPayload {
        offset: u64,
        needed: usize,
        source: std::io::Error,
    },
    UnknownPacketId {
        id: u8,
        offset: u64,
    },
    MissingTimeSync {
        packet_type: &'static str,
    },
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeError::Io { op, source } => {
                write!(f, "{op}: {source}")
            }
            DecodeError::TruncatedPayload { offset, needed, .. } => {
                write!(
                    f,
                    "truncated payload at offset {offset} (needed {needed} bytes)"
                )
            }
            DecodeError::UnknownPacketId { id, offset } => {
                write!(f, "unknown packet ID {id:#04x} at offset {offset}")
            }
            DecodeError::MissingTimeSync { packet_type } => {
                write!(f, "{packet_type} before timing_sync: missing timing sync")
            }
        }
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DecodeError::Io { source, .. } => Some(source),
            DecodeError::TruncatedPayload { source, .. } => Some(source),
            DecodeError::UnknownPacketId { .. } | DecodeError::MissingTimeSync { .. } => None,
        }
    }
}

pub struct PacketDecoder<R: Read> {
    reader: R,
    offset: u64,
    separator_byte: u8,
    simulated_time_ms: Option<u32>,
    fast_interval_ms: Option<u16>,
}

impl<R: Read> PacketDecoder<R> {
    pub fn new(reader: R, separator_byte: u8) -> Self {
        Self {
            reader,
            offset: 0,
            separator_byte,
            simulated_time_ms: None,
            fast_interval_ms: None,
        }
    }

    pub fn next_packet(&mut self) -> Result<Option<PacketData>> {
        loop {
            let mut id_buf = [0u8; 1];
            match self.reader.read_exact(&mut id_buf) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(None);
                }
                Err(source) => {
                    return Err(DecodeError::Io {
                        op: "reading packet ID",
                        source,
                    });
                }
            }
            let id = id_buf[0];
            self.offset += 1;

            if id == ID_PADDING {
                continue;
            }
            if id == self.separator_byte {
                return Ok(Some(PacketData::ExperimentSeparator {}));
            }

            match id {
                ID_TIMING_SYNC => return self.decode_timing_sync().map(Some),
                ID_FAST_ADC => return self.decode_fast_adc().map(Some),
                ID_SLOW_ADC => return self.decode_slow_adc().map(Some),
                ID_TEMPERATURE => {
                    return self.decode_temperature().map(Some);
                }
                ID_DIGITAL => return self.decode_digital().map(Some),
                ID_SERVO => return self.decode_servo().map(Some),
                _ => {
                    return Err(DecodeError::UnknownPacketId {
                        id,
                        offset: self.offset - 1,
                    });
                }
            }
        }
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        if let Err(source) = self.reader.read_exact(buf) {
            return Err(DecodeError::TruncatedPayload {
                offset: self.offset,
                needed: buf.len(),
                source,
            });
        }
        self.offset += buf.len() as u64;
        Ok(())
    }

    fn read_u8(&mut self) -> Result<u8> {
        let mut buf = [0u8; 1];
        self.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    fn read_u16_le(&mut self) -> Result<u16> {
        let mut buf = [0u8; 2];
        self.read_exact(&mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }

    fn read_u32_le(&mut self) -> Result<u32> {
        let mut buf = [0u8; 4];
        self.read_exact(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    fn decode_timing_sync(&mut self) -> Result<PacketData> {
        let timestamp_ms = self.read_u32_le()?;
        let fast_interval_ms = self.read_u16_le()?;
        self.simulated_time_ms = Some(timestamp_ms);
        self.fast_interval_ms = Some(fast_interval_ms);
        Ok(PacketData::TimingSync {
            timestamp_ms,
            fast_interval_ms,
        })
    }

    fn decode_fast_adc(&mut self) -> Result<PacketData> {
        let tensometer = self.read_u16_le()?;
        let tank = self.read_u16_le()?;
        let combustion = self.read_u16_le()?;

        let timestamp_ms = self.current_time_ms("fast_adc")?;
        let fast_interval_ms = self.fast_interval_ms.ok_or(DecodeError::MissingTimeSync {
            packet_type: "fast_adc",
        })?;

        self.simulated_time_ms = Some(timestamp_ms.wrapping_add(fast_interval_ms as u32));

        Ok(PacketData::FastAdc {
            timestamp_ms,
            tensometer,
            tank,
            combustion,
        })
    }

    fn decode_slow_adc(&mut self) -> Result<PacketData> {
        let bat_stand = self.read_u16_le()?;
        let bat_comp = self.read_u16_le()?;
        let boost = self.read_u16_le()?;
        let starter = self.read_u16_le()?;
        let timestamp_ms = self.current_time_ms("slow_adc")?;
        Ok(PacketData::SlowAdc {
            timestamp_ms,
            bat_stand,
            bat_comp,
            boost,
            starter,
        })
    }

    fn decode_temperature(&mut self) -> Result<PacketData> {
        let sensor_id = self.read_u8()?;
        let value = self.read_u16_le()?;
        let timestamp_ms = self.current_time_ms("temperature")?;
        Ok(PacketData::Temperature {
            timestamp_ms,
            sensor_id,
            value,
        })
    }

    fn decode_digital(&mut self) -> Result<PacketData> {
        let value = self.read_u8()?;
        let timestamp_ms = self.current_time_ms("digital")?;
        Ok(PacketData::Digital {
            timestamp_ms,
            value,
        })
    }

    fn decode_servo(&mut self) -> Result<PacketData> {
        let ticks = self.read_u16_le()?;
        let timestamp_ms = self.current_time_ms("servo")?;
        Ok(PacketData::Servo {
            timestamp_ms,
            ticks,
        })
    }

    fn current_time_ms(&self, packet_type: &'static str) -> Result<u32> {
        self.simulated_time_ms
            .ok_or(DecodeError::MissingTimeSync { packet_type })
    }
}
