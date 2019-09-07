use std::{fmt, num::ParseIntError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeHexError {
    InvalidLength(LengthError),
    ParseInt(ParseIntError),
}

impl From<ParseIntError> for DecodeHexError {
    fn from(e: ParseIntError) -> Self {
        DecodeHexError::ParseInt(e)
    }
}

impl fmt::Display for DecodeHexError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DecodeHexError::OddLength => "input hex string length is odd ".fmt(f),
            DecodeHexError::ParseInt(e) => e.fmt(f),
        }
    }
}

pub enum LengthError {
    OddLength,
    Maximum(u32),
    Minimum(u32),
}

pub fn decode_hex(s: &str) -> Result<Vec<u8>, DecodeHexError> {
    if s.len() % 2 != 0 {
        Err(DecodeHexError::InvalidLength(LengthError(OddLength)))
    } else {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.into()))
            .collect()
    }
}

pub fn measure_variable_int(vint: Vec<u8>) -> Result<u8, DecodeHexError> {
    let ln = vint.len();
    if ln > 9 {
        return Err(DecodeHexError::InvalidLength(LengthError::Maximum(9)));
    }

    let val: usize = match vint[0] {
        0..=253 => 1,
        253 => 2,
        254 => 5,
        255 => 9,
    }
    Ok(val)
}

pub fn decode_variable_int(vint: Vec<u8>) -> Result<u64, DecodeHexError> {
    let ln = vint.len();
    if ln > 9 {
        return Err(DecodeHexError::InvalidLength(LengthError::Maximum(9)));
    }

    let val: u64 = match vint[0] {
        0..=253 => vint[0] as u64,
        253 => vint[1] as u64,
        254 => u32::from_le_bytes(vint[1..5]) as u64,
        255 => u64::from_le_bytes(vint[1..9]),
    }
    Ok(val)
}
