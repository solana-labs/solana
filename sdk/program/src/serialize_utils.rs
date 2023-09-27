//! Helpers for reading and writing bytes.

#![allow(clippy::arithmetic_side_effects)]
use crate::{pubkey::Pubkey, sanitize::SanitizeError};

pub fn append_u16(buf: &mut Vec<u8>, data: u16) {
    let start = buf.len();
    buf.resize(buf.len() + 2, 0);
    let end = buf.len();
    buf[start..end].copy_from_slice(&data.to_le_bytes());
}

pub fn append_u8(buf: &mut Vec<u8>, data: u8) {
    let start = buf.len();
    buf.resize(buf.len() + 1, 0);
    buf[start] = data;
}

pub fn append_slice(buf: &mut Vec<u8>, data: &[u8]) {
    let start = buf.len();
    buf.resize(buf.len() + data.len(), 0);
    let end = buf.len();
    buf[start..end].copy_from_slice(data);
}

pub fn read_u8(current: &mut usize, data: &[u8]) -> Result<u8, SanitizeError> {
    if data.len() < *current + 1 {
        return Err(SanitizeError::IndexOutOfBounds);
    }
    let e = data[*current];
    *current += 1;
    Ok(e)
}

pub fn read_pubkey(current: &mut usize, data: &[u8]) -> Result<Pubkey, SanitizeError> {
    let len = std::mem::size_of::<Pubkey>();
    if data.len() < *current + len {
        return Err(SanitizeError::IndexOutOfBounds);
    }
    let e = Pubkey::try_from(&data[*current..*current + len])
        .map_err(|_| SanitizeError::ValueOutOfBounds)?;
    *current += len;
    Ok(e)
}

pub fn read_u16(current: &mut usize, data: &[u8]) -> Result<u16, SanitizeError> {
    if data.len() < *current + 2 {
        return Err(SanitizeError::IndexOutOfBounds);
    }
    let mut fixed_data = [0u8; 2];
    fixed_data.copy_from_slice(&data[*current..*current + 2]);
    let e = u16::from_le_bytes(fixed_data);
    *current += 2;
    Ok(e)
}

pub fn read_slice(
    current: &mut usize,
    data: &[u8],
    data_len: usize,
) -> Result<Vec<u8>, SanitizeError> {
    if data.len() < *current + data_len {
        return Err(SanitizeError::IndexOutOfBounds);
    }
    let e = data[*current..*current + data_len].to_vec();
    *current += data_len;
    Ok(e)
}
