#[repr(C)]
pub struct BigModExpParams{
    pub base: *const u8,
    pub base_len: u64,
    pub exponent: *const u8,
    pub exponent_len: u64,
    pub modulus: *const u8,
    pub modulus_len: u64,
}

/// Big integer modular exponentiation
pub fn big_mod_exp(base: &[u8], exponent: &[u8], modulus: &[u8]) -> Vec<u8>{
    #[cfg(not(target_os = "solana"))]
    {
        use num_bigint::BigUint;
        let modulus_len = modulus.len();
        let base = BigUint::from_bytes_be(base);
        let exponent = BigUint::from_bytes_be(exponent);
        let modulus = BigUint::from_bytes_be(modulus);
        let ret_int = base.modpow(&exponent, &modulus);
        let ret_int = ret_int.to_bytes_be();
        let mut return_value = vec![0_u8; modulus_len - ret_int.len()];
        return_value.extend(ret_int);
        return_value
    }

    #[cfg(target_os = "solana")]
    {
        let mut return_value = vec![0_u8; modulus.len()];

        let param = BigModExpParams{
            base: base as *const _ as *const u8,
            base_len: base.len() as u64,
            exponent: exponent as *const _ as *const u8,
            exponent_len: exponent.len() as u64,
            modulus: modulus as *const _ as *const u8,
            modulus_len: modulus.len() as u64,
        };
        // let result = unsafe {
        unsafe {
            crate::syscalls::sol_big_mod_exp(
                &param as *const _ as *const u8,
                return_value.as_mut_slice() as *mut _ as *mut u8,
            )
        };

        // match result {
        //     0 => Ok(Secp256k1Pubkey::new(&pubkey_buffer)),
        //     error => Err(Secp256k1RecoverError::from(error)),
        // }

        return_value
    }
}