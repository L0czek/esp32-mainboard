use crate::mqtt::codec::EncodeError;

pub const CMD_STATUS_MAX_LEN: usize = 64;

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
