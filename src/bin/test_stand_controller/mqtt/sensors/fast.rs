use crate::mqtt::codec::{pack_u12_with_padding, validate_u12_samples, write_u32_le, EncodeError};
use crate::mqtt::sensors::EncodablePayload;
use crate::mqtt::topics::{
    TOPIC_SENSOR_ADC_FAST_PRESSURE_COMBUSTION, TOPIC_SENSOR_ADC_FAST_PRESSURE_TANK,
    TOPIC_SENSOR_ADC_FAST_TENSOMETER,
};

pub const FAST_MAX_SAMPLES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum FastAdcChannel {
    Tensometer,
    PressureTank,
    PressureCombustion,
}

impl FastAdcChannel {
    pub const fn topic(self) -> &'static str {
        match self {
            Self::Tensometer => TOPIC_SENSOR_ADC_FAST_TENSOMETER,
            Self::PressureTank => TOPIC_SENSOR_ADC_FAST_PRESSURE_TANK,
            Self::PressureCombustion => TOPIC_SENSOR_ADC_FAST_PRESSURE_COMBUSTION,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FastAdcPacket {
    channel: FastAdcChannel,
    pub first_timestamp_ms: u32,
    pub last_timestamp_ms: u32,
    samples: [u16; FAST_MAX_SAMPLES],
    sample_count: u8,
}

impl FastAdcPacket {
    pub fn new(
        channel: FastAdcChannel,
        first_timestamp_ms: u32,
        last_timestamp_ms: u32,
        samples: [u16; FAST_MAX_SAMPLES],
        sample_count: usize,
    ) -> Result<Self, EncodeError> {
        if sample_count == 0 || sample_count > FAST_MAX_SAMPLES {
            return Err(EncodeError::TooManySamples);
        }

        validate_u12_samples(&samples[..sample_count], FAST_MAX_SAMPLES)?;

        Ok(Self {
            channel,
            first_timestamp_ms,
            last_timestamp_ms,
            samples,
            sample_count: sample_count as u8,
        })
    }

    pub fn from_slice(
        channel: FastAdcChannel,
        first_timestamp_ms: u32,
        last_timestamp_ms: u32,
        samples: &[u16],
    ) -> Result<Self, EncodeError> {
        validate_u12_samples(samples, FAST_MAX_SAMPLES)?;

        let mut copy = [0u16; FAST_MAX_SAMPLES];
        copy[..samples.len()].copy_from_slice(samples);

        Self::new(
            channel,
            first_timestamp_ms,
            last_timestamp_ms,
            copy,
            samples.len(),
        )
    }

    pub const fn channel(&self) -> FastAdcChannel {
        self.channel
    }

    pub fn set_channel(&mut self, channel: FastAdcChannel) {
        self.channel = channel;
    }

    pub fn topic(&self) -> &'static str {
        self.channel.topic()
    }

    pub fn samples(&self) -> &[u16] {
        &self.samples[..self.sample_count as usize]
    }
}

impl EncodablePayload for FastAdcPacket {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, EncodeError> {
        if out.len() < 8 {
            return Err(EncodeError::BufferTooSmall);
        }

        write_u32_le(&mut out[..4], self.first_timestamp_ms)?;
        write_u32_le(&mut out[4..8], self.last_timestamp_ms)?;

        let written = pack_u12_with_padding(self.samples(), &mut out[8..])?;
        Ok(8 + written)
    }
}
