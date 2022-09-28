pub mod prelude {
    pub use crate::big_mod_exp::{
        big_mod_exp
    };
}

/// Big integer modular exponentiation
pub fn big_mod_exp(base: &[u8], exponent: &[u8], modulus: &[u8]) -> Vec<u8>{
    #[cfg(not(target_os = "solana"))]
    {
        use num_bigint::BigUint;
        let base = BigUint::from_bytes_be(base);
        let exponent = BigUint::from_bytes_be(exponent);
        let modulus = BigUint::from_bytes_be(modulus);
        base.modpow(&exponent, &modulus)
    }

    #[cfg(target_os = "solana")]
    {
        let mut result =
    }
}