//! @brief Solana Rust-based BPF program utility functions and types

use core::mem::size_of;
use core::slice::from_raw_parts;
use heapless::consts::*;
use heapless::String; // fixed capacity `std::Vec` // type level integer used to specify capacity
use process;

extern "C" {
    fn sol_log_(message: *const u8);
}
/// Helper function that prints a string to stdout
pub fn sol_log(message: &str) {
    let mut c_string: String<U256> = String::new();
    if message.len() < 256 - 1 {
        if c_string.push_str(message).is_err() {
            c_string
                .push_str("Attempted to log a malformed string\0")
                .is_ok();
        }
        if c_string.push('\0').is_err() {
            c_string.push_str("Failed to log string\0").is_ok();
        };
    } else {
        c_string
            .push_str("Attempted to log a string that is too long\0")
            .is_ok();
    }
    unsafe {
        sol_log_(c_string.as_bytes().as_ptr());
    }
}

extern "C" {
    fn sol_log_64_(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64);
}
/// Helper function that prints a 64 bit values represented in hexadecimal
/// to stdout
pub fn sol_log_64(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) {
    unsafe {
        sol_log_64_(arg1, arg2, arg3, arg4, arg5);
    }
}

/// Prints the hexadecimal representation of a public key
///
/// @param key The public key to print
pub fn sol_log_key(key: &SolPubkey) {
    for (i, k) in key.key.iter().enumerate() {
        sol_log_64(0, 0, 0, i as u64, u64::from(*k));
    }
}

/// Prints the hexadecimal representation of a slice
///
/// @param slice The array to print
pub fn sol_log_slice(slice: &[u8]) {
    for (i, s) in slice.iter().enumerate() {
        sol_log_64(0, 0, 0, i as u64, u64::from(*s));
    }
}

/// Prints the hexadecimal representation of the program's input parameters
///
/// @param ka A pointer to an array of SolKeyedAccount to print
/// @param data A pointer to the instruction data to print
pub fn sol_log_params(ka: &[SolKeyedAccount], data: &[u8]) {
    sol_log("- Number of KeyedAccounts");
    sol_log_64(0, 0, 0, 0, ka.len() as u64);
    for k in ka.iter() {
        sol_log("- Is signer");
        sol_log_64(0, 0, 0, 0, k.is_signer as u64);
        sol_log("- Key");
        sol_log_key(&k.key);
        sol_log("- Tokens");
        sol_log_64(0, 0, 0, 0, k.tokens);
        sol_log("- Userdata");
        sol_log_slice(k.userdata);
        sol_log("- Owner");
        sol_log_key(&k.owner);
    }
    sol_log("- Instruction data");
    sol_log_slice(data);
}

pub const SIZE_PUBKEY: usize = 32;

/// Public key
pub struct SolPubkey<'a> {
    pub key: &'a [u8],
}

/// Keyed Account
pub struct SolKeyedAccount<'a> {
    /// Public key of the account
    pub key: SolPubkey<'a>,
    /// Public key of the account
    pub is_signer: u64,
    /// Number of tokens owned by this account
    pub tokens: u64,
    /// On-chain data within this account
    pub userdata: &'a [u8],
    /// Program that owns this account
    pub owner: SolPubkey<'a>,
}

/// Information about the state of the cluster immediately before the program
/// started executing the current instruction
pub struct SolClusterInfo<'a> {
    /// Current ledger tick
    pub tick_height: u64,
    ///program_id of the currently executing program
    pub program_id: SolPubkey<'a>,
}

#[no_mangle]
pub extern "C" fn entrypoint(input: *mut u8) -> bool {
    const NUM_KA: usize = 1; // Number of KeyedAccounts expected
    let mut offset: usize = 0;

    // Number of KeyedAccounts present

    let num_ka = unsafe {
        #[allow(clippy::cast_ptr_alignment)]
        let num_ka_ptr: *const u64 = input.add(offset) as *const u64;
        *num_ka_ptr
    };
    offset += 8;

    if num_ka != NUM_KA as u64 {
        return false;
    }

    // KeyedAccounts

    let is_signer = unsafe {
        #[allow(clippy::cast_ptr_alignment)]
        let is_signer_ptr: *const u64 = input.add(offset) as *const u64;
        *is_signer_ptr
    };
    offset += size_of::<u64>();

    let key_slice = unsafe { from_raw_parts(input.add(offset), SIZE_PUBKEY) };
    let key = SolPubkey { key: &key_slice };
    offset += SIZE_PUBKEY;

    let tokens = unsafe {
        #[allow(clippy::cast_ptr_alignment)]
        let tokens_ptr: *const u64 = input.add(offset) as *const u64;
        *tokens_ptr
    };
    offset += size_of::<u64>();

    let userdata_length = unsafe {
        #[allow(clippy::cast_ptr_alignment)]
        let userdata_length_ptr: *const u64 = input.add(offset) as *const u64;
        *userdata_length_ptr
    } as usize;
    offset += size_of::<u64>();

    let userdata = unsafe { from_raw_parts(input.add(offset), userdata_length) };
    offset += userdata_length;

    let owner_slice = unsafe { from_raw_parts(input.add(offset), SIZE_PUBKEY) };
    let owner = SolPubkey { key: &owner_slice };
    offset += SIZE_PUBKEY;

    let mut ka = [SolKeyedAccount {
        key,
        is_signer,
        tokens,
        userdata,
        owner,
    }];

    // Instruction data

    let data_length = unsafe {
        #[allow(clippy::cast_ptr_alignment)]
        let data_length_ptr: *const u64 = input.add(offset) as *const u64;
        *data_length_ptr
    } as usize;
    offset += size_of::<u64>();

    let data = unsafe { from_raw_parts(input.add(offset), data_length) };
    offset += data_length;

    // Tick height

    let tick_height = unsafe {
        #[allow(clippy::cast_ptr_alignment)]
        let tick_height_ptr: *const u64 = input.add(offset) as *const u64;
        *tick_height_ptr
    };
    offset += size_of::<u64>();

    // Id

    let program_id_slice = unsafe { from_raw_parts(input.add(offset), SIZE_PUBKEY) };
    let program_id: SolPubkey = SolPubkey {
        key: &program_id_slice,
    };

    let info = SolClusterInfo {
        tick_height,
        program_id,
    };

    // Call user implementable function
    process(&mut ka, &data, &info)
}
