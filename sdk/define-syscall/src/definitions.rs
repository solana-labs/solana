//! This module is only for syscall definitions that bring in no extra dependencies.
use crate::define_syscall;

define_syscall!(fn sol_secp256k1_recover(hash: *const u8, recovery_id: u64, signature: *const u8, result: *mut u8) -> u64);
define_syscall!(fn sol_poseidon(parameters: u64, endianness: u64, vals: *const u8, val_len: u64, hash_result: *mut u8) -> u64);
define_syscall!(fn sol_invoke_signed_c(instruction_addr: *const u8, account_infos_addr: *const u8, account_infos_len: u64, signers_seeds_addr: *const u8, signers_seeds_len: u64) -> u64);
define_syscall!(fn sol_invoke_signed_rust(instruction_addr: *const u8, account_infos_addr: *const u8, account_infos_len: u64, signers_seeds_addr: *const u8, signers_seeds_len: u64) -> u64);
define_syscall!(fn sol_set_return_data(data: *const u8, length: u64));
define_syscall!(fn sol_get_stack_height() -> u64);
define_syscall!(fn sol_log_(message: *const u8, len: u64));
define_syscall!(fn sol_log_64_(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64));
define_syscall!(fn sol_log_compute_units_());
define_syscall!(fn sol_log_data(data: *const u8, data_len: u64));
define_syscall!(fn sol_memcpy_(dst: *mut u8, src: *const u8, n: u64));
define_syscall!(fn sol_memmove_(dst: *mut u8, src: *const u8, n: u64));
define_syscall!(fn sol_memcmp_(s1: *const u8, s2: *const u8, n: u64, result: *mut i32));
define_syscall!(fn sol_memset_(s: *mut u8, c: u8, n: u64));
define_syscall!(fn sol_log_pubkey(pubkey_addr: *const u8));
define_syscall!(fn sol_create_program_address(seeds_addr: *const u8, seeds_len: u64, program_id_addr: *const u8, address_bytes_addr: *const u8) -> u64);
define_syscall!(fn sol_try_find_program_address(seeds_addr: *const u8, seeds_len: u64, program_id_addr: *const u8, address_bytes_addr: *const u8, bump_seed_addr: *const u8) -> u64);
define_syscall!(fn sol_sha256(vals: *const u8, val_len: u64, hash_result: *mut u8) -> u64);
define_syscall!(fn sol_keccak256(vals: *const u8, val_len: u64, hash_result: *mut u8) -> u64);
define_syscall!(fn sol_blake3(vals: *const u8, val_len: u64, hash_result: *mut u8) -> u64);
define_syscall!(fn sol_curve_validate_point(curve_id: u64, point_addr: *const u8, result: *mut u8) -> u64);
define_syscall!(fn sol_curve_group_op(curve_id: u64, group_op: u64, left_input_addr: *const u8, right_input_addr: *const u8, result_point_addr: *mut u8) -> u64);
define_syscall!(fn sol_curve_multiscalar_mul(curve_id: u64, scalars_addr: *const u8, points_addr: *const u8, points_len: u64, result_point_addr: *mut u8) -> u64);
define_syscall!(fn sol_curve_pairing_map(curve_id: u64, point: *const u8, result: *mut u8) -> u64);
define_syscall!(fn sol_alt_bn128_group_op(group_op: u64, input: *const u8, input_size: u64, result: *mut u8) -> u64);
define_syscall!(fn sol_big_mod_exp(params: *const u8, result: *mut u8) -> u64);
define_syscall!(fn sol_remaining_compute_units() -> u64);
define_syscall!(fn sol_alt_bn128_compression(op: u64, input: *const u8, input_size: u64, result: *mut u8) -> u64);
define_syscall!(fn sol_get_sysvar(sysvar_id_addr: *const u8, result: *mut u8, offset: u64, length: u64) -> u64);
define_syscall!(fn sol_get_epoch_stake(vote_address: *const u8) -> u64);

// these are to be deprecated once they are superceded by sol_get_sysvar
define_syscall!(fn sol_get_clock_sysvar(addr: *mut u8) -> u64);
define_syscall!(fn sol_get_epoch_schedule_sysvar(addr: *mut u8) -> u64);
define_syscall!(fn sol_get_rent_sysvar(addr: *mut u8) -> u64);
define_syscall!(fn sol_get_last_restart_slot(addr: *mut u8) -> u64);
define_syscall!(fn sol_get_epoch_rewards_sysvar(addr: *mut u8) -> u64);

// this cannot go through sol_get_sysvar but can be removed once no longer in use
define_syscall!(fn sol_get_fees_sysvar(addr: *mut u8) -> u64);
