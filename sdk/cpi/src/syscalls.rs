/// Syscall definitions used by `solana_cpi`.
pub use solana_define_syscall::definitions::{
    sol_invoke_signed_c, sol_invoke_signed_rust, sol_set_return_data,
};
use {solana_define_syscall::define_syscall, solana_pubkey::Pubkey};

define_syscall!(fn sol_get_return_data(data: *mut u8, length: u64, program_id: *mut Pubkey) -> u64);
