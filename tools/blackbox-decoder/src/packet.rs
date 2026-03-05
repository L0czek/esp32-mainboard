use serde::Serialize;

pub const ID_FAST_ADC: u8 = 0x01;
pub const ID_SLOW_ADC: u8 = 0x02;
pub const ID_TEMPERATURE: u8 = 0x03;
pub const ID_DIGITAL: u8 = 0x04;
pub const ID_SERVO: u8 = 0x05;
pub const ID_TIMING_SYNC: u8 = 0x06;

pub const ID_PADDING: u8 = 0x00;
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PacketData {
    TimingSync {
        timestamp_ms: u32,
        fast_interval_ms: u16,
    },
    FastAdc {
        timestamp_ms: u32,
        tensometer: u16,
        tank: u16,
        combustion: u16,
    },
    SlowAdc {
        timestamp_ms: u32,
        bat_stand: u16,
        bat_comp: u16,
        boost: u16,
        starter: u16,
    },
    Temperature {
        timestamp_ms: u32,
        sensor_id: u8,
        value: u16,
    },
    Digital {
        timestamp_ms: u32,
        value: u8,
    },
    Servo {
        timestamp_ms: u32,
        ticks: u16,
    },
    ExperimentSeparator {},
}
