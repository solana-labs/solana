//! Example Rust-based SBF realloc test program

use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

pub const REALLOC: u8 = 1;
pub const REALLOC_EXTEND: u8 = 2;
pub const REALLOC_EXTEND_AND_FILL: u8 = 3;
pub const REALLOC_AND_ASSIGN: u8 = 4;
pub const REALLOC_AND_ASSIGN_TO_SELF_VIA_SYSTEM_PROGRAM: u8 = 5;
pub const ASSIGN_TO_SELF_VIA_SYSTEM_PROGRAM_AND_REALLOC: u8 = 6;
pub const DEALLOC_AND_ASSIGN_TO_CALLER: u8 = 7;
pub const CHECK: u8 = 8;
pub const ZERO_INIT: u8 = 9;
pub const REALLOC_EXTEND_AND_UNDO: u8 = 10;

pub fn realloc(program_id: &Pubkey, address: &Pubkey, size: usize, bump: &mut u8) -> Instruction {
    let mut instruction_data = vec![REALLOC, *bump];
    instruction_data.extend_from_slice(&size.to_le_bytes());

    *bump = bump.saturating_add(1);

    Instruction::new_with_bytes(
        *program_id,
        &instruction_data,
        vec![AccountMeta::new(*address, false)],
    )
}

pub fn realloc_extend(
    program_id: &Pubkey,
    address: &Pubkey,
    size: usize,
    bump: &mut u8,
) -> Instruction {
    let mut instruction_data = vec![REALLOC_EXTEND, *bump];
    instruction_data.extend_from_slice(&size.to_le_bytes());

    *bump = bump.saturating_add(1);

    Instruction::new_with_bytes(
        *program_id,
        &instruction_data,
        vec![AccountMeta::new(*address, false)],
    )
}

pub fn realloc_extend_and_fill(
    program_id: &Pubkey,
    address: &Pubkey,
    size: usize,
    fill: u8,
    bump: &mut u64,
) -> Instruction {
    let mut instruction_data = vec![
        REALLOC_EXTEND_AND_FILL,
        fill,
        *bump as u8,
        (*bump / 255) as u8,
    ];
    instruction_data.extend_from_slice(&size.to_le_bytes());

    *bump = bump.saturating_add(1);

    Instruction::new_with_bytes(
        *program_id,
        &instruction_data,
        vec![AccountMeta::new(*address, false)],
    )
}

pub fn realloc_extend_and_undo(
    program_id: &Pubkey,
    address: &Pubkey,
    size: usize,
    bump: &mut u8,
) -> Instruction {
    let mut instruction_data = vec![REALLOC_EXTEND_AND_UNDO, *bump];
    instruction_data.extend_from_slice(&size.to_le_bytes());

    *bump = bump.saturating_add(1);

    Instruction::new_with_bytes(
        *program_id,
        &instruction_data,
        vec![AccountMeta::new(*address, false)],
    )
}
