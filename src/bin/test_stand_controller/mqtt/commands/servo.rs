use super::{trim_ascii, Command};
use crate::servo::command::ServoCommand;

impl Command for ServoCommand {
    fn try_decode(payload: &[u8]) -> Option<Self> {
        match trim_ascii(payload) {
            b"OPEN" => Some(Self::Open),
            b"CLOSE" => Some(Self::Close),
            _ => None,
        }
    }
}
