use std::io::Read;

use anyhow::{Context, Result, bail};

use crate::packet::{
    ID_DIGITAL, ID_FAST_ADC, ID_PADDING, ID_SERVO, ID_SLOW_ADC, ID_TEMPERATURE, MAX_TEMP_SENSORS,
    PacketData,
};

pub struct PacketDecoder<R: Read> {
    reader: R,
    offset: u64,
    separator_byte: u8,
}

impl<R: Read> PacketDecoder<R> {
    pub fn new(reader: R, separator_byte: u8) -> Self {
        Self {
            reader,
            offset: 0,
            separator_byte,
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
                Err(e) => return Err(e).context("reading packet ID"),
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
                ID_FAST_ADC => return self.decode_fast_adc().map(Some),
                ID_SLOW_ADC => return self.decode_slow_adc().map(Some),
                ID_TEMPERATURE => {
                    return self.decode_temperature().map(Some);
                }
                ID_DIGITAL => return self.decode_digital().map(Some),
                ID_SERVO => return self.decode_servo().map(Some),
                _ => {
                    bail!("unknown packet ID {id:#04x} at offset {}", self.offset - 1,);
                }
            }
        }
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        self.reader.read_exact(buf).with_context(|| {
            format!(
                "truncated payload at offset {} \
                     (needed {} bytes)",
                self.offset,
                buf.len(),
            )
        })?;
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

    fn decode_fast_adc(&mut self) -> Result<PacketData> {
        let timestamp_ms = self.read_u32_le()?;
        let tensometer = self.read_u16_le()?;
        let tank = self.read_u16_le()?;
        let combustion = self.read_u16_le()?;
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
        Ok(PacketData::SlowAdc {
            bat_stand,
            bat_comp,
            boost,
            starter,
        })
    }

    fn decode_temperature(&mut self) -> Result<PacketData> {
        let count = self.read_u8()?;
        if count > MAX_TEMP_SENSORS {
            bail!(
                "temperature sensor count {count} exceeds \
                 maximum {MAX_TEMP_SENSORS} at offset {}",
                self.offset - 1,
            );
        }
        let mut sensors = Vec::with_capacity(count as usize);
        for _ in 0..count {
            sensors.push(self.read_u16_le()?);
        }
        Ok(PacketData::Temperature { sensors })
    }

    fn decode_digital(&mut self) -> Result<PacketData> {
        let value = self.read_u8()?;
        Ok(PacketData::Digital { value })
    }

    fn decode_servo(&mut self) -> Result<PacketData> {
        let ticks = self.read_u16_le()?;
        Ok(PacketData::Servo { ticks })
    }
}
