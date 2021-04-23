#![allow(clippy::integer_arithmetic)]
//! This account contains the serialized transaction instructions

use crate::{instruction::Instruction, sanitize::SanitizeError, sysvar::Sysvar};

pub type Instructions = Vec<Instruction>;

crate::declare_sysvar_id!("Sysvar1nstructions1111111111111111111111111", Instructions);

impl Sysvar for Instructions {}

pub fn load_current_index(data: &[u8]) -> u16 {
    let mut instr_fixed_data = [0u8; 2];
    let len = data.len();
    instr_fixed_data.copy_from_slice(&data[len - 2..len]);
    u16::from_le_bytes(instr_fixed_data)
}

pub fn store_current_index(data: &mut [u8], instruction_index: u16) {
    let last_index = data.len() - 2;
    data[last_index..last_index + 2].copy_from_slice(&instruction_index.to_le_bytes());
}

pub fn load_instruction_at(index: usize, data: &[u8]) -> Result<Instruction, SanitizeError> {
    crate::message::Message::deserialize_instruction(index, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_store_instruction() {
        let mut data = [4u8; 10];
        store_current_index(&mut data, 3);
        assert_eq!(load_current_index(&data), 3);
        assert_eq!([4u8; 8], data[0..8]);
    }
}
