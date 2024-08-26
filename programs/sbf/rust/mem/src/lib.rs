//! Test mem functions

use {
    solana_program::{
        account_info::AccountInfo,
        entrypoint::ProgramResult,
        program_memory::{sol_memcmp, sol_memcpy, sol_memmove, sol_memset},
        pubkey::Pubkey,
    },
    solana_sbf_rust_mem_dep::{run_mem_tests, MemOps},
};

solana_program::entrypoint_no_alloc!(process_instruction);
pub fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    // Via syscalls
    #[derive(Default)]
    struct MemOpSyscalls();
    impl MemOps for MemOpSyscalls {
        fn memcpy(&self, dst: &mut [u8], src: &[u8], n: usize) {
            sol_memcpy(dst, src, n)
        }
        unsafe fn memmove(&self, dst: *mut u8, src: *mut u8, n: usize) {
            sol_memmove(dst, src, n)
        }
        fn memset(&self, s: &mut [u8], c: u8, n: usize) {
            sol_memset(s, c, n)
        }
        fn memcmp(&self, s1: &[u8], s2: &[u8], n: usize) -> i32 {
            sol_memcmp(s1, s2, n)
        }
    }
    run_mem_tests(MemOpSyscalls::default());

    Ok(())
}
