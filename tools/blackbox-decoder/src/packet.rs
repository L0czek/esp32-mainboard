use serde::Serialize;

pub const ID_FAST_ADC: u8 = 0x01;
pub const ID_SLOW_ADC: u8 = 0x02;
pub const ID_TEMPERATURE: u8 = 0x03;
pub const ID_DIGITAL: u8 = 0x04;
pub const ID_SERVO: u8 = 0x05;

pub const ID_PADDING: u8 = 0x00;
pub const MAX_TEMP_SENSORS: u8 = 32;

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PacketData {
    FastAdc {
        timestamp_ms: u32,
        tensometer: u16,
        tank: u16,
        combustion: u16,
    },
    SlowAdc {
        bat_stand: u16,
        bat_comp: u16,
        boost: u16,
        starter: u16,
    },
    Temperature {
        sensors: Vec<u16>,
    },
    Digital {
        value: u8,
    },
    Servo {
        ticks: u16,
    },
    ExperimentSeparator {},
}
