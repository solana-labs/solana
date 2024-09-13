use {
    crate::{AccountMeta, ProcessedSiblingInstruction},
    solana_define_syscall::define_syscall,
    solana_pubkey::Pubkey,
};

define_syscall!(fn sol_get_processed_sibling_instruction(index: u64, meta: *mut ProcessedSiblingInstruction, program_id: *mut Pubkey, data: *mut u8, accounts: *mut AccountMeta) -> u64);
define_syscall!(fn sol_get_stack_height() -> u64);
