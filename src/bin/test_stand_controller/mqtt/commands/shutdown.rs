use crate::mqtt::commands::{trim_ascii, Command};

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum ShutdownCommand {
    Shutdown,
}

impl Command for ShutdownCommand {
    fn try_decode(payload: &[u8]) -> Option<Self> {
        match trim_ascii(payload) {
            b"SHUTDOWN" => Some(Self::Shutdown),
            _ => None,
        }
    }
}
