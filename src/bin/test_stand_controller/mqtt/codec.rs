pub const U12_MAX: u16 = 0x0FFF;

#[derive(Debug, Clone, Copy, defmt::Format)]
pub enum EncodeError {
    EmptySamples,
    BufferTooSmall,
    TooManySamples,
    ValueOutOfRange,
}

pub fn write_u32_le(out: &mut [u8], value: u32) -> Result<(), EncodeError> {
    if out.len() < 4 {
        return Err(EncodeError::BufferTooSmall);
    }
    out[..4].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

pub fn write_u16_le(out: &mut [u8], value: u16) -> Result<(), EncodeError> {
    if out.len() < 2 {
        return Err(EncodeError::BufferTooSmall);
    }
    out[..2].copy_from_slice(&value.to_le_bytes());
    Ok(())
}

pub fn validate_u12_samples(samples: &[u16], max_count: usize) -> Result<(), EncodeError> {
    if samples.is_empty() {
        return Err(EncodeError::EmptySamples);
    }
    if samples.len() > max_count {
        return Err(EncodeError::TooManySamples);
    }
    if samples.iter().any(|value| *value > U12_MAX) {
        return Err(EncodeError::ValueOutOfRange);
    }
    Ok(())
}

pub fn encoded_u12_len(sample_count: usize) -> Result<usize, EncodeError> {
    if sample_count == 0 {
        return Err(EncodeError::EmptySamples);
    }

    let padded_count = if sample_count.is_multiple_of(2) {
        sample_count + 1
    } else {
        sample_count
    };

    let pair_count = padded_count / 2;
    Ok((pair_count * 3) + 2)
}

pub fn pack_u12_with_padding(samples: &[u16], out: &mut [u8]) -> Result<usize, EncodeError> {
    let needed = encoded_u12_len(samples.len())?;
    if out.len() < needed {
        return Err(EncodeError::BufferTooSmall);
    }

    let mut src_index = 0usize;
    let mut out_index = 0usize;

    while src_index + 1 < samples.len() {
        let first = samples[src_index];
        let second = samples[src_index + 1];
        if first > U12_MAX || second > U12_MAX {
            return Err(EncodeError::ValueOutOfRange);
        }

        out[out_index] = (first & 0x00FF) as u8;
        out[out_index + 1] = ((first >> 8) as u8 & 0x0F) | (((second as u8) & 0x0F) << 4);
        out[out_index + 2] = (second >> 4) as u8;

        src_index += 2;
        out_index += 3;
    }

    if src_index < samples.len() {
        let sample = samples[src_index];
        if sample > U12_MAX {
            return Err(EncodeError::ValueOutOfRange);
        }
        out[out_index] = (sample & 0x00FF) as u8;
        out[out_index + 1] = ((sample >> 8) as u8) & 0x0F;
    } else {
        out[out_index] = 0;
        out[out_index + 1] = 0;
    }

    Ok(needed)
}
