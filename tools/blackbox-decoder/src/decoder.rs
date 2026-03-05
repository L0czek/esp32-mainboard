use std::io::Read;
use std::{collections::VecDeque, io::ErrorKind};

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

const TIMING_SYNC_SIGNATURE: [u8; 8] = [
    ID_TIMING_SYNC,
    b'T',
    b'I',
    b'M',
    b'E',
    b'S',
    b'Y',
    b'N',
];
const WINDOW_SIZE: usize = TIMING_SYNC_SIGNATURE.len();

pub struct PacketDecoder<R: Read> {
    reader: R,
    offset: u64,
    separator_byte: u8,
    simulated_time_ms: Option<u32>,
    fast_interval_ms: Option<u16>,
    fast_packets_since_timing_sync: u32,
    window: VecDeque<u8>,
    end_of_stream: bool,
}

impl<R: Read> PacketDecoder<R> {
    pub fn new(reader: R, separator_byte: u8) -> Self {
        Self {
            reader,
            offset: 0,
            separator_byte,
            simulated_time_ms: None,
            fast_interval_ms: None,
            fast_packets_since_timing_sync: 0,
            window: VecDeque::with_capacity(WINDOW_SIZE),
            end_of_stream: false,
        }
    }

    pub fn next_packet(&mut self) -> Result<Option<PacketData>> {
        loop {
            let Some(id) = self.try_read_byte_raw()? else {
                return Ok(None);
            };

            if id == ID_PADDING {
                continue;
            }
            if id == self.separator_byte {
                return Ok(Some(PacketData::ExperimentSeparator {}));
            }

            return match id {
                ID_TIMING_SYNC => self.decode_timing_sync().map(Some),
                ID_FAST_ADC => self.decode_fast_adc().map(Some),
                ID_SLOW_ADC => self.decode_slow_adc().map(Some),
                ID_TEMPERATURE => self.decode_temperature().map(Some),
                ID_DIGITAL => self.decode_digital().map(Some),
                ID_SERVO => self.decode_servo().map(Some),
                _ => Err(DecodeError::UnknownPacketId {
                    id,
                    offset: self.offset - 1,
                }),
            };
        }
    }

    fn read_byte_from_input_stream(&mut self) -> Result<Option<u8>> {
        let mut buf = [0u8; 1];
        match self.reader.read(&mut buf) {
            Ok(0) => Ok(None),
            Ok(_) => Ok(Some(buf[0])),
            Err(source) => Err(DecodeError::Io {
                op: "reading input stream",
                source,
            }),
        }
    }

    fn refill_read_buffer(&mut self) -> Result<()> {
        while !self.end_of_stream && self.window.len() < WINDOW_SIZE {
            match self.read_byte_from_input_stream()? {
                Some(byte) => self.window.push_back(byte),
                None => self.end_of_stream = true,
            }
        }
        Ok(())
    }

    fn read_buffer_matches_timing_sync_signature(&self) -> bool {
        self.window.len() == TIMING_SYNC_SIGNATURE.len()
            && self.window.iter().copied().eq(TIMING_SYNC_SIGNATURE)
    }

    // Reads one byte from the sliding read buffer without checking for timing sync match.
    // Returns Ok(None) when the underlying stream reached EOF.
    fn try_read_byte_raw(&mut self) -> Result<Option<u8>> {
        self.refill_read_buffer()?;
        let Some(byte) = self.window.pop_front() else {
            return Ok(None);
        };
        self.offset += 1;

        if !self.end_of_stream {
            match self.read_byte_from_input_stream()? {
                Some(next) => self.window.push_back(next),
                None => self.end_of_stream = true,
            }
        }

        Ok(Some(byte))
    }

    // Reads one byte from the sliding read buffer.
    // Returns Ok(None) when the underlying stream reached EOF.
    fn try_read_byte(&mut self) -> Result<Option<u8>> {
        if self.read_buffer_matches_timing_sync_signature() {
            return Err(DecodeError::TruncatedPayload {
                offset: self.offset,
                needed: 1,
                source: std::io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "timing sync signature detected inside packet",
                ),
            });
        }

        self.try_read_byte_raw()
    }

    // Reads one byte from the sliding read buffer.
    // Returns DecodeError::TruncatedPayload when the underlying stream reached EOF.
    fn read_byte(&mut self) -> Result<u8> {
        let Some(byte) = self.try_read_byte()? else {
            return Err(DecodeError::TruncatedPayload {
                offset: self.offset,
                needed: 1,
                source: std::io::Error::new(ErrorKind::UnexpectedEof, "unexpected end of stream"),
            });
        };

        Ok(byte)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        for item in buf {
            *item = self.read_byte()?;
        }
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
        for expected in TIMING_SYNC_SIGNATURE.iter().copied().skip(1) {
            let value = self.read_u8()?;
            if value != expected {
                return Err(DecodeError::TruncatedPayload {
                    offset: self.offset - 1,
                    needed: 1,
                    source: std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "timing sync marker mismatch",
                    ),
                });
            }
        }

        let timestamp_ms = self.read_u32_le()?;
        let fast_interval_ms = self.read_u16_le()?;
        self.simulated_time_ms = Some(timestamp_ms);
        self.fast_interval_ms = Some(fast_interval_ms);
        self.fast_packets_since_timing_sync = 0;
        Ok(PacketData::TimingSync {
            timestamp_ms,
            fast_interval_ms,
        })
    }

    fn decode_fast_adc(&mut self) -> Result<PacketData> {
        let tensometer = self.read_u16_le()?;
        let tank = self.read_u16_le()?;
        let combustion = self.read_u16_le()?;

        let mut timestamp_ms = self.current_time_ms("fast_adc")?;
        let fast_interval_ms = self.fast_interval_ms.ok_or(DecodeError::MissingTimeSync {
            packet_type: "fast_adc",
        })?;

        if self.fast_packets_since_timing_sync > 0 {
            timestamp_ms = timestamp_ms.wrapping_add(fast_interval_ms as u32);
            self.simulated_time_ms = Some(timestamp_ms);
        }
        self.fast_packets_since_timing_sync = self.fast_packets_since_timing_sync.wrapping_add(1);

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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::packet::{ID_DIGITAL, ID_FAST_ADC, ID_SLOW_ADC, ID_TEMPERATURE, PacketData};

    use super::{DecodeError, PacketDecoder, TIMING_SYNC_SIGNATURE};

    fn push_u16_le(buf: &mut Vec<u8>, value: u16) {
        buf.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32_le(buf: &mut Vec<u8>, value: u32) {
        buf.extend_from_slice(&value.to_le_bytes());
    }

    fn push_timing_sync(buf: &mut Vec<u8>, timestamp_ms: u32, fast_interval_ms: u16) {
        buf.extend_from_slice(&TIMING_SYNC_SIGNATURE);
        push_u32_le(buf, timestamp_ms);
        push_u16_le(buf, fast_interval_ms);
    }

    #[test]
    fn decodes_timing_sync_and_fast_adc_with_interpolated_timestamp() {
        let mut bytes = Vec::new();
        push_timing_sync(&mut bytes, 1_000, 5);
        bytes.push(ID_FAST_ADC);
        push_u16_le(&mut bytes, 11);
        push_u16_le(&mut bytes, 22);
        push_u16_le(&mut bytes, 33);

        let mut decoder = PacketDecoder::new(Cursor::new(bytes), 0xAA);

        let first = decoder.next_packet().expect("decode timing sync");
        match first {
            Some(PacketData::TimingSync {
                timestamp_ms,
                fast_interval_ms,
            }) => {
                assert_eq!(timestamp_ms, 1_000);
                assert_eq!(fast_interval_ms, 5);
            }
            other => panic!("unexpected first packet: {:?}", other),
        }

        let second = decoder.next_packet().expect("decode fast adc");
        match second {
            Some(PacketData::FastAdc {
                timestamp_ms,
                tensometer,
                tank,
                combustion,
            }) => {
                assert_eq!(timestamp_ms, 1_000);
                assert_eq!(tensometer, 11);
                assert_eq!(tank, 22);
                assert_eq!(combustion, 33);
            }
            other => panic!("unexpected second packet: {:?}", other),
        }

        let end = decoder.next_packet().expect("decode eof");
        assert!(end.is_none());
    }

    #[test]
    fn detects_timing_sync_signature_mid_packet_and_resynchronizes() {
        let mut bytes = Vec::new();
        push_timing_sync(&mut bytes, 10_000, 2);

        // Start a slow packet, then inject a timing sync signature in place of payload bytes.
        bytes.push(ID_SLOW_ADC);
        bytes.extend_from_slice(&TIMING_SYNC_SIGNATURE);
        push_u32_le(&mut bytes, 20_000);
        push_u16_le(&mut bytes, 4);

        // Add one packet after resync to verify stream recovery.
        bytes.push(ID_DIGITAL);
        bytes.push(1);

        let mut decoder = PacketDecoder::new(Cursor::new(bytes), 0xAA);

        let first = decoder.next_packet().expect("decode first timing sync");
        match first {
            Some(PacketData::TimingSync {
                timestamp_ms,
                fast_interval_ms,
            }) => {
                assert_eq!(timestamp_ms, 10_000);
                assert_eq!(fast_interval_ms, 2);
            }
            other => panic!("unexpected first packet: {:?}", other),
        }

        let second = decoder.next_packet();
        match second {
            Err(DecodeError::TruncatedPayload { .. }) => {}
            other => panic!("expected TruncatedPayload, got {:?}", other),
        }

        let third = decoder.next_packet().expect("decode resynced timing sync");
        match third {
            Some(PacketData::TimingSync {
                timestamp_ms,
                fast_interval_ms,
            }) => {
                assert_eq!(timestamp_ms, 20_000);
                assert_eq!(fast_interval_ms, 4);
            }
            other => panic!("unexpected third packet: {:?}", other),
        }

        let fourth = decoder.next_packet().expect("decode digital after resync");
        match fourth {
            Some(PacketData::Digital {
                timestamp_ms,
                value,
            }) => {
                assert_eq!(timestamp_ms, 20_000);
                assert_eq!(value, 1);
            }
            other => panic!("unexpected fourth packet: {:?}", other),
        }

        let end = decoder.next_packet().expect("decode eof");
        assert!(end.is_none());
    }

    #[test]
    fn fast_adc_only_advances_time_from_second_sample_after_sync() {
        let mut bytes = Vec::new();
        push_timing_sync(&mut bytes, 1_000, 5);

        bytes.push(ID_FAST_ADC);
        push_u16_le(&mut bytes, 11);
        push_u16_le(&mut bytes, 22);
        push_u16_le(&mut bytes, 33);

        bytes.push(ID_TEMPERATURE);
        bytes.push(7);
        push_u16_le(&mut bytes, 444);

        bytes.push(ID_FAST_ADC);
        push_u16_le(&mut bytes, 44);
        push_u16_le(&mut bytes, 55);
        push_u16_le(&mut bytes, 66);

        bytes.push(ID_DIGITAL);
        bytes.push(1);

        let mut decoder = PacketDecoder::new(Cursor::new(bytes), 0xAA);

        let first = decoder.next_packet().expect("decode timing sync");
        match first {
            Some(PacketData::TimingSync {
                timestamp_ms,
                fast_interval_ms,
            }) => {
                assert_eq!(timestamp_ms, 1_000);
                assert_eq!(fast_interval_ms, 5);
            }
            other => panic!("unexpected first packet: {:?}", other),
        }

        let second = decoder.next_packet().expect("decode first fast adc");
        match second {
            Some(PacketData::FastAdc { timestamp_ms, .. }) => {
                assert_eq!(timestamp_ms, 1_000);
            }
            other => panic!("unexpected second packet: {:?}", other),
        }

        let third = decoder.next_packet().expect("decode temperature");
        match third {
            Some(PacketData::Temperature { timestamp_ms, .. }) => {
                assert_eq!(timestamp_ms, 1_000);
            }
            other => panic!("unexpected third packet: {:?}", other),
        }

        let fourth = decoder.next_packet().expect("decode second fast adc");
        match fourth {
            Some(PacketData::FastAdc { timestamp_ms, .. }) => {
                assert_eq!(timestamp_ms, 1_005);
            }
            other => panic!("unexpected fourth packet: {:?}", other),
        }

        let fifth = decoder.next_packet().expect("decode digital");
        match fifth {
            Some(PacketData::Digital { timestamp_ms, value }) => {
                assert_eq!(timestamp_ms, 1_005);
                assert_eq!(value, 1);
            }
            other => panic!("unexpected fifth packet: {:?}", other),
        }

        let end = decoder.next_packet().expect("decode eof");
        assert!(end.is_none());
    }
}
