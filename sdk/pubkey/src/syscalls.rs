/// Syscall definitions used by `solana_pubkey`.
use solana_define_syscall::define_syscall;

define_syscall!(fn sol_log_pubkey(pubkey_addr: *const u8));
define_syscall!(fn sol_create_program_address(seeds_addr: *const u8, seeds_len: u64, program_id_addr: *const u8, address_bytes_addr: *const u8) -> u64);
define_syscall!(fn sol_try_find_program_address(seeds_addr: *const u8, seeds_len: u64, program_id_addr: *const u8, address_bytes_addr: *const u8, bump_seed_addr: *const u8) -> u64);
