use crate::mqtt::commands::{trim_ascii, Command};

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum StateCommand {
    Fire,
    Abort,
    /// triggered automatically when safety button transitions to safe state
    SafetySafe,
    FireEnd,
    FireReset,
}

impl Command for StateCommand {
    fn try_decode(payload: &[u8]) -> Option<Self> {
        match trim_ascii(payload) {
            b"FIRE" => Some(Self::Fire),
            b"ABORT" => Some(Self::Abort),
            b"FIRE_END" => Some(Self::FireEnd),
            b"FIRE_RESET" => Some(Self::FireReset),
            _ => None,
        }
    }
}
