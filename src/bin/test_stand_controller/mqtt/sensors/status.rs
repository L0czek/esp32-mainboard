use embassy_time::Instant;

use crate::{
    mqtt::{codec::EncodeError, sensors::EncodableEnum},
    servo::state::ServoStatus,
};

pub const CMD_STATUS_MAX_LEN: usize = 64;
pub const CPU_IDLE_METRIC_LEN: usize = 2;
pub const WIFI_RSSI_METRIC_LEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum StateStatus {
    Armed,
    Countdown { end: Instant },
    Fire,
    PostFire,
    LampTest,
    CameraTest,
}

impl EncodableEnum for StateStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Armed => "ARMED",
            Self::Countdown { .. } => "COUNTDOWN",
            Self::Fire => "FIRE",
            Self::PostFire => "POSTFIRE",
            Self::LampTest => "LAMPTEST",
            Self::CameraTest => "CAMERATEST",
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
            Self::LampTest => "State: LAMPTEST",
            Self::CameraTest => "State: CAMERATEST",
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
    value: [u8; CPU_IDLE_METRIC_LEN],
}

impl CpuIdleMetricPacket {
    #[must_use]
    pub fn from_idle_permille(idle_permille: u16) -> Self {
        let clamped = idle_permille.min(1_000);
        Self {
            value: clamped.to_le_bytes(),
        }
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.value
    }
}

#[derive(Debug, Clone)]
pub struct WifiRssiMetricPacket {
    value: [u8; WIFI_RSSI_METRIC_LEN],
}

impl WifiRssiMetricPacket {
    #[must_use]
    pub fn from_dbm(rssi_dbm: i32) -> Self {
        Self {
            value: rssi_dbm.to_le_bytes(),
        }
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.value
    }
}
