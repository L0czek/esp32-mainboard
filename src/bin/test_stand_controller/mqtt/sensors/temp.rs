use crate::mqtt::codec::{write_u16_le, write_u32_le, EncodeError};
use crate::mqtt::sensors::EncodablePayload;

pub const TEMP_MAX_SAMPLES: usize = 64;

#[derive(Debug, Clone)]
pub struct TempPacket {
    sensor_id: u8,
    pub first_timestamp_ms: u32,
    values: [u16; TEMP_MAX_SAMPLES],
    sample_count: u8,
}

impl TempPacket {
    pub fn new(
        sensor_id: u8,
        first_timestamp_ms: u32,
        values: [u16; TEMP_MAX_SAMPLES],
        sample_count: usize,
    ) -> Result<Self, EncodeError> {
        if sample_count == 0 || sample_count > TEMP_MAX_SAMPLES {
            return Err(EncodeError::TooManySamples);
        }

        Ok(Self {
            sensor_id,
            first_timestamp_ms,
            values,
            sample_count: sample_count as u8,
        })
    }

    pub fn from_slice(
        sensor_id: u8,
        first_timestamp_ms: u32,
        values: &[u16],
    ) -> Result<Self, EncodeError> {
        if values.is_empty() || values.len() > TEMP_MAX_SAMPLES {
            return Err(EncodeError::TooManySamples);
        }

        let mut copy = [0u16; TEMP_MAX_SAMPLES];
        copy[..values.len()].copy_from_slice(values);
        Self::new(sensor_id, first_timestamp_ms, copy, values.len())
    }

    pub const fn sensor_id(&self) -> u8 {
        self.sensor_id
    }

    pub fn values(&self) -> &[u16] {
        &self.values[..self.sample_count as usize]
    }
}

impl EncodablePayload for TempPacket {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, EncodeError> {
        let values = self.values();
        let needed = 4 + (values.len() * 2);
        if out.len() < needed {
            return Err(EncodeError::BufferTooSmall);
        }

        write_u32_le(&mut out[..4], self.first_timestamp_ms)?;

        for (index, value) in values.iter().enumerate() {
            let offset = 4 + (index * 2);
            write_u16_le(&mut out[offset..offset + 2], *value)?;
        }

        Ok(needed)
    }
}
