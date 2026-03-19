use embassy_time::Instant;

use crate::{
    mqtt::{codec::EncodeError, sensors::EncodableEnum},
    servo::state::ServoStatus,
};

pub const CMD_STATUS_MAX_LEN: usize = 64;
pub const CPU_IDLE_METRIC_MAX_LEN: usize = 8;
pub const WIFI_RSSI_METRIC_MAX_LEN: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum StateStatus {
    Armed,
    Countdown { end: Instant },
    Fire,
    PostFire,
}

impl EncodableEnum for StateStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Armed => "ARMED",
            Self::Countdown { .. } => "COUNTDOWN",
            Self::Fire => "FIRE",
            Self::PostFire => "POSTFIRE",
        }
    }
}

impl StateStatus {
    pub const fn as_log(self) -> &'static str {
        match self {
            Self::Armed => "State: ARMED",
            Self::Countdown { .. } => "State: COUNTDOWN",
            Self::Fire => "State: FIRE",
            Self::PostFire => "State: POSTFIRE",
        }
    }
}

impl EncodableEnum for ServoStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Closed => "CLOSED",
            Self::Opening => "OPENING",
            Self::Open => "OPEN",
            Self::Closing => "CLOSING",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommandStatusPacket {
    value: [u8; CMD_STATUS_MAX_LEN],
    len: u8,
}

impl CommandStatusPacket {
    pub fn from_bytes(value: &[u8]) -> Result<Self, EncodeError> {
        if value.is_empty() || value.len() > CMD_STATUS_MAX_LEN {
            return Err(EncodeError::TooManySamples);
        }

        let mut copy = [0u8; CMD_STATUS_MAX_LEN];
        copy[..value.len()].copy_from_slice(value);

        Ok(Self {
            value: copy,
            len: value.len() as u8,
        })
    }

    pub fn from_str(value: &str) -> Result<Self, EncodeError> {
        Self::from_bytes(value.as_bytes())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.value[..self.len as usize]
    }
}

#[derive(Debug, Clone)]
pub struct CpuIdleMetricPacket {
    value: [u8; CPU_IDLE_METRIC_MAX_LEN],
    len: u8,
}

impl CpuIdleMetricPacket {
    #[must_use]
    pub fn from_idle_permille(idle_permille: u16) -> Self {
        let clamped = idle_permille.min(1_000);
        let whole_percent = clamped / 10;
        let decimal = clamped % 10;

        let mut value = [0u8; CPU_IDLE_METRIC_MAX_LEN];
        let mut len = write_u16_decimal(whole_percent, &mut value);
        value[len] = b'.';
        len += 1;
        value[len] = b'0' + (decimal as u8);
        len += 1;
        value[len] = b'%';
        len += 1;

        Self {
            value,
            len: len as u8,
        }
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.value[..self.len as usize]
    }
}

#[derive(Debug, Clone)]
pub struct WifiRssiMetricPacket {
    value: [u8; WIFI_RSSI_METRIC_MAX_LEN],
    len: u8,
}

impl WifiRssiMetricPacket {
    #[must_use]
    pub fn from_dbm(rssi_dbm: i32) -> Self {
        let mut value = [0u8; WIFI_RSSI_METRIC_MAX_LEN];
        let len = write_i32_decimal(rssi_dbm, &mut value);

        Self {
            value,
            len: len as u8,
        }
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.value[..self.len as usize]
    }
}

fn write_u16_decimal(value: u16, out: &mut [u8; CPU_IDLE_METRIC_MAX_LEN]) -> usize {
    if value >= 100 {
        out[0] = b'1';
        out[1] = b'0';
        out[2] = b'0';
        return 3;
    }

    if value >= 10 {
        out[0] = b'0' + ((value / 10) as u8);
        out[1] = b'0' + ((value % 10) as u8);
        return 2;
    }

    out[0] = b'0' + (value as u8);
    1
}

fn write_i32_decimal(value: i32, out: &mut [u8; WIFI_RSSI_METRIC_MAX_LEN]) -> usize {
    let mut digits = [0u8; 10];
    let mut digits_len = 0usize;
    let mut magnitude = value.unsigned_abs();

    loop {
        digits[digits_len] = b'0' + (magnitude % 10) as u8;
        digits_len += 1;
        magnitude /= 10;
        if magnitude == 0 {
            break;
        }
    }

    let mut len = 0usize;
    if value < 0 {
        out[len] = b'-';
        len += 1;
    }

    for idx in (0..digits_len).rev() {
        out[len] = digits[idx];
        len += 1;
    }

    len
}
