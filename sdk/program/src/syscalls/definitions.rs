use crate::{
    instruction::{AccountMeta, ProcessedSiblingInstruction},
    pubkey::Pubkey,
};

#[cfg(target_feature = "static-syscalls")]
macro_rules! define_syscall {
    (fn $name:ident($($arg:ident: $typ:ty),*) -> $ret:ty) => {
		#[inline]
        pub unsafe fn $name($($arg: $typ),*) -> $ret {
			// this enum is used to force the hash to be computed in a const context
			#[repr(usize)]
			enum Syscall {
				Code = sys_hash(stringify!($name)),
			}

            let syscall: extern "C" fn($($arg: $typ),*) -> $ret = core::mem::transmute(Syscall::Code);
            syscall($($arg),*)
        }

    };
    (fn $name:ident($($arg:ident: $typ:ty),*)) => {
        define_syscall!(fn $name($($arg: $typ),*) -> ());
    }
}

#[cfg(not(target_feature = "static-syscalls"))]
macro_rules! define_syscall {
	(fn $name:ident($($arg:ident: $typ:ty),*) -> $ret:ty) => {
		extern "C" {
			pub fn $name($($arg: $typ),*) -> $ret;
		}
	};
	(fn $name:ident($($arg:ident: $typ:ty),*)) => {
		define_syscall!(fn $name($($arg: $typ),*) -> ());
	}
}

define_syscall!(fn sol_log_(message: *const u8, len: u64));
define_syscall!(fn sol_log_64_(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64));
define_syscall!(fn sol_log_compute_units_());
define_syscall!(fn sol_log_pubkey(pubkey_addr: *const u8));
define_syscall!(fn sol_create_program_address(seeds_addr: *const u8, seeds_len: u64, program_id_addr: *const u8, address_bytes_addr: *const u8) -> u64);
define_syscall!(fn sol_try_find_program_address(seeds_addr: *const u8, seeds_len: u64, program_id_addr: *const u8, address_bytes_addr: *const u8, bump_seed_addr: *const u8) -> u64);
define_syscall!(fn sol_sha256(vals: *const u8, val_len: u64, hash_result: *mut u8) -> u64);
define_syscall!(fn sol_keccak256(vals: *const u8, val_len: u64, hash_result: *mut u8) -> u64);
define_syscall!(fn sol_secp256k1_recover(hash: *const u8, recovery_id: u64, signature: *const u8, result: *mut u8) -> u64);
define_syscall!(fn sol_blake3(vals: *const u8, val_len: u64, hash_result: *mut u8) -> u64);
define_syscall!(fn sol_get_clock_sysvar(addr: *mut u8) -> u64);
define_syscall!(fn sol_get_epoch_schedule_sysvar(addr: *mut u8) -> u64);
define_syscall!(fn sol_get_fees_sysvar(addr: *mut u8) -> u64);
define_syscall!(fn sol_get_rent_sysvar(addr: *mut u8) -> u64);
define_syscall!(fn sol_get_last_restart_slot(addr: *mut u8) -> u64);
define_syscall!(fn sol_memcpy_(dst: *mut u8, src: *const u8, n: u64));
define_syscall!(fn sol_memmove_(dst: *mut u8, src: *const u8, n: u64));
define_syscall!(fn sol_memcmp_(s1: *const u8, s2: *const u8, n: u64, result: *mut i32));
define_syscall!(fn sol_memset_(s: *mut u8, c: u8, n: u64));
define_syscall!(fn sol_invoke_signed_c(instruction_addr: *const u8, account_infos_addr: *const u8, account_infos_len: u64, signers_seeds_addr: *const u8, signers_seeds_len: u64) -> u64);
define_syscall!(fn sol_invoke_signed_rust(instruction_addr: *const u8, account_infos_addr: *const u8, account_infos_len: u64, signers_seeds_addr: *const u8, signers_seeds_len: u64) -> u64);
define_syscall!(fn sol_set_return_data(data: *const u8, length: u64));
define_syscall!(fn sol_get_return_data(data: *mut u8, length: u64, program_id: *mut Pubkey) -> u64);
define_syscall!(fn sol_log_data(data: *const u8, data_len: u64));
define_syscall!(fn sol_get_processed_sibling_instruction(index: u64, meta: *mut ProcessedSiblingInstruction, program_id: *mut Pubkey, data: *mut u8, accounts: *mut AccountMeta) -> u64);
define_syscall!(fn sol_get_stack_height() -> u64);
define_syscall!(fn sol_curve_validate_point(curve_id: u64, point_addr: *const u8, result: *mut u8) -> u64);
define_syscall!(fn sol_curve_group_op(curve_id: u64, group_op: u64, left_input_addr: *const u8, right_input_addr: *const u8, result_point_addr: *mut u8) -> u64);
define_syscall!(fn sol_curve_multiscalar_mul(curve_id: u64, scalars_addr: *const u8, points_addr: *const u8, points_len: u64, result_point_addr: *mut u8) -> u64);
define_syscall!(fn sol_curve_pairing_map(curve_id: u64, point: *const u8, result: *mut u8) -> u64);
define_syscall!(fn sol_alt_bn128_group_op(group_op: u64, input: *const u8, input_size: u64, result: *mut u8) -> u64);
define_syscall!(fn sol_big_mod_exp(params: *const u8, result: *mut u8) -> u64);
define_syscall!(fn sol_get_epoch_rewards_sysvar(addr: *mut u8) -> u64);
define_syscall!(fn sol_poseidon(parameters: u64, endianness: u64, vals: *const u8, val_len: u64, hash_result: *mut u8) -> u64);
define_syscall!(fn sol_remaining_compute_units() -> u64);
define_syscall!(fn sol_alt_bn128_compression(op: u64, input: *const u8, input_size: u64, result: *mut u8) -> u64);

#[cfg(target_feature = "static-syscalls")]
pub const fn sys_hash(name: &str) -> usize {
    murmur3_32(name.as_bytes(), 0) as usize
}

#[cfg(target_feature = "static-syscalls")]
const fn murmur3_32(buf: &[u8], seed: u32) -> u32 {
    const fn pre_mix(buf: [u8; 4]) -> u32 {
        u32::from_le_bytes(buf)
            .wrapping_mul(0xcc9e2d51)
            .rotate_left(15)
            .wrapping_mul(0x1b873593)
    }

    let mut hash = seed;

    let mut i = 0;
    while i < buf.len() / 4 {
        let buf = [buf[i * 4], buf[i * 4 + 1], buf[i * 4 + 2], buf[i * 4 + 3]];
        hash ^= pre_mix(buf);
        hash = hash.rotate_left(13);
        hash = hash.wrapping_mul(5).wrapping_add(0xe6546b64);

        i += 1;
    }

    match buf.len() % 4 {
        0 => {}
        1 => {
            hash = hash ^ pre_mix([buf[i * 4], 0, 0, 0]);
        }
        2 => {
            hash = hash ^ pre_mix([buf[i * 4], buf[i * 4 + 1], 0, 0]);
        }
        3 => {
            hash = hash ^ pre_mix([buf[i * 4], buf[i * 4 + 1], buf[i * 4 + 2], 0]);
        }
        _ => { /* unreachable!() */ }
    }

    hash = hash ^ buf.len() as u32;
    hash = hash ^ (hash.wrapping_shr(16));
    hash = hash.wrapping_mul(0x85ebca6b);
    hash = hash ^ (hash.wrapping_shr(13));
    hash = hash.wrapping_mul(0xc2b2ae35);
    hash = hash ^ (hash.wrapping_shr(16));

    hash
}
