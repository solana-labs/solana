//! @brief BigNumber Syscall test

extern crate solana_program;
use solana_program::{custom_panic_default, msg};


#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u64 {
    msg!("bignum");


    0
}

custom_panic_default!();

#[cfg(test)]
mod test {
    use super::*;
}
