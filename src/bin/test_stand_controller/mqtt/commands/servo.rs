#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum ServoCommand {
    Open,
    Close,
}

impl ServoCommand {
    pub fn decode(payload: &[u8]) -> Option<Self> {
        match trim_ascii(payload) {
            b"OPEN" => Some(Self::Open),
            b"CLOSE" => Some(Self::Close),
            _ => None,
        }
    }
}

fn trim_ascii(input: &[u8]) -> &[u8] {
    let start = input
        .iter()
        .position(|value| !value.is_ascii_whitespace())
        .unwrap_or(input.len());

    let end = input
        .iter()
        .rposition(|value| !value.is_ascii_whitespace())
        .map(|index| index + 1)
        .unwrap_or(start);

    &input[start..end]
}
