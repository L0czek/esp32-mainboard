use crate::mqtt::codec::EncodeError;

pub const CMD_STATUS_MAX_LEN: usize = 64;
pub const CPU_IDLE_METRIC_MAX_LEN: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum StateStatus {
    Armed,
    Fire,
    PostFire,
}

impl StateStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Armed => "ARMED",
            Self::Fire => "FIRE",
            Self::PostFire => "POSTFIRE",
        }
    }

    pub const fn as_bytes(self) -> &'static [u8] {
        self.as_str().as_bytes()
    }

    pub const fn as_log(self) -> &'static str {
        match self {
            Self::Armed => "State: ARMED",
            Self::Fire => "State: FIRE",
            Self::PostFire => "State: POSTFIRE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum ServoStatus {
    Closed,
    Opening,
    Open,
    Closing,
}

impl ServoStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "CLOSED",
            Self::Opening => "OPENING",
            Self::Open => "OPEN",
            Self::Closing => "CLOSING",
        }
    }

    pub const fn as_bytes(self) -> &'static [u8] {
        self.as_str().as_bytes()
    }

    pub const fn as_log(self) -> &'static str {
        match self {
            Self::Closed => "Servo closed",
            Self::Opening => "Servo opening",
            Self::Open => "Servo open",
            Self::Closing => "Servo closing",
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
