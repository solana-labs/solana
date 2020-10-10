//! @brief Example Rust-based BPF program that tests call depth and stack usage

use solana_sdk::{entrypoint::SUCCESS, info};

#[inline(never)]
pub fn recurse(data: &mut [u8]) {
    if data.len() <= 1 {
        return;
    }
    recurse(&mut data[1..]);
    info!(line!(), 0, 0, 0, data[0]);
}

/// # Safety
#[inline(never)]
#[no_mangle]
pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64 {
    info!("Call depth");
    let depth = *(input.add(16) as *mut u8);
    info!(line!(), 0, 0, 0, depth);
    let mut data = Vec::with_capacity(depth as usize);
    for i in 0_u8..depth {
        data.push(i);
    }
    recurse(&mut data);
    SUCCESS
}
