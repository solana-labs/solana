//! @brief Solana Rust-based BigNumber for bpf-programs

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use std::fmt;

#[repr(transparent)]
#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    // Clone,
    Default,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    AbiExample,
)]

pub struct BigNumber(u64);

impl BigNumber {
    /// Returns a BigNumber with initial value of 0
    pub fn new() -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::BigNum;
            let bbox = Box::new(BigNum::new().unwrap());
            let rwptr = Box::into_raw(bbox);
            let bignum_ptr = rwptr as u64;
            Self(bignum_ptr)
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_new(bignum_ptr: *mut u64) -> u64;
            }
            let mut bignum_ptr = 0u64;
            unsafe {
                sol_bignum_new(&mut bignum_ptr as *mut _ as *mut u64);
            }
            BigNumber::from(bignum_ptr)
        }
    }

    /// Returns a BigNumber with initial value set to a u32 value
    pub fn from_u32(val: u32) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::BigNum;
            let bbox = Box::new(BigNum::from_u32(val).unwrap());
            let rwptr = Box::into_raw(bbox);
            let bignum_ptr = rwptr as u64;
            Self(bignum_ptr)
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_from_u32(bignum_ptr: *mut u64, val: u64) -> u64;
            }
            let mut bignum_ptr = 0u64;
            unsafe {
                sol_bignum_from_u32(&mut bignum_ptr as *mut _ as *mut u64, val as u64);
            }
            BigNumber::from(bignum_ptr)
        }
    }

    /// Returns a BigNumber with value set to big endian array of bytes
    pub fn from_bytes(val: &[u8]) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::BigNum;
            let bbox = Box::new(BigNum::from_slice(val).unwrap());
            let rwptr = Box::into_raw(bbox);
            let bignum_ptr = rwptr as u64;
            Self(bignum_ptr)
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_from_bytes(
                    bignum_ptr: *mut u64,
                    bytes_ptr: *const u64,
                    bytes_len: u64,
                ) -> u64;
            }
            let mut bignum_ptr = 0u64;
            unsafe {
                sol_bignum_from_bytes(
                    &mut bignum_ptr as *mut _ as *mut u64,
                    val.as_ptr() as *const _ as *const u64,
                    val.len() as u64,
                );
            }
            BigNumber::from(bignum_ptr)
        }
    }

    /// Returns an array of bytes (big endian) of self
    pub fn to_bytes(&self) -> Vec<u8> {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::BigNum;
            let braw: &BigNum = unsafe { &*(self.0 as *const BigNum) };
            braw.as_ref().to_vec()
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_to_bytes(
                    bignum_ptr: *const u64,
                    bytes_ptr: *mut u8,
                    bytes_len: *mut u64,
                ) -> u64;
            }
            let mut my_buffer = [0u8; 256];
            let mut my_buffer_len = my_buffer.len();
            unsafe {
                sol_bignum_to_bytes(
                    &self.0 as *const _ as *const u64,
                    &mut my_buffer as *mut _ as *mut u8,
                    &mut my_buffer_len as *mut _ as *mut u64,
                );
            };
            my_buffer[0..my_buffer_len].to_vec()
        }
    }

    /// Add BigNumbers
    pub fn add(&self, rhs: &BigNumber) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::{BigNum, BigNumRef};
            let my_raw: &BigNum = unsafe { &*(self.0 as *const BigNum) };
            let mut bbox = Box::new(BigNum::new().unwrap());
            let rhs_ptr: &BigNum = unsafe { &*(rhs.0 as *const BigNum) };
            BigNumRef::checked_add(&mut bbox, my_raw, rhs_ptr).unwrap();
            let rwptr = Box::into_raw(bbox);
            Self(rwptr as u64)
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_add(
                    self_ptr: *const u64,
                    rhs_ptr: *const u64,
                    return_ptr: *mut u64,
                ) -> u64;
            }
            let mut bignum_ptr = 0u64;
            unsafe {
                sol_bignum_add(
                    &self.0 as *const _ as *const u64,
                    &rhs.0 as *const _ as *const u64,
                    &mut bignum_ptr as *mut _ as *mut u64,
                );
            }
            Self(bignum_ptr)
        }
    }

    /// Subtract BigNumbers
    pub fn sub(&self, rhs: &BigNumber) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::{BigNum, BigNumRef};
            let my_raw: &BigNum = unsafe { &*(self.0 as *const BigNum) };
            let mut bbox = Box::new(BigNum::new().unwrap());
            let rhs_ptr: &BigNum = unsafe { &*(rhs.0 as *const BigNum) };
            BigNumRef::checked_sub(&mut *bbox, my_raw, rhs_ptr).unwrap();
            let rwptr = Box::into_raw(bbox);
            Self(rwptr as u64)
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_sub(
                    self_ptr: *const u64,
                    rhs_ptr: *const u64,
                    return_ptr: *mut u64,
                ) -> u64;
            }
            let mut bignum_ptr = 0u64;
            unsafe {
                sol_bignum_sub(
                    &self.0 as *const _ as *const u64,
                    &rhs.0 as *const _ as *const u64,
                    &mut bignum_ptr as *mut _ as *mut u64,
                );
            }
            Self(bignum_ptr)
        }
    }

    /// Multiple BigNumbers
    pub fn mul(&self, rhs: &BigNumber) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::{BigNum, BigNumContext, BigNumRef};
            let my_raw: &BigNum = unsafe { &*(self.0 as *const BigNum) };
            let mut bbox = Box::new(BigNum::new().unwrap());
            let rhs_ptr: &BigNum = unsafe { &*(rhs.0 as *const BigNum) };
            BigNumRef::checked_mul(
                &mut *bbox,
                my_raw,
                rhs_ptr,
                &mut BigNumContext::new().unwrap(),
            )
            .unwrap();
            let rwptr = Box::into_raw(bbox);
            Self(rwptr as u64)
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_mul(
                    self_ptr: *const u64,
                    rhs_ptr: *const u64,
                    return_ptr: *mut u64,
                ) -> u64;
            }
            let mut bignum_ptr = 0u64;
            unsafe {
                sol_bignum_mul(
                    &self.0 as *const _ as *const u64,
                    &rhs.0 as *const _ as *const u64,
                    &mut bignum_ptr as *mut _ as *mut u64,
                );
            }
            Self(bignum_ptr)
        }
    }

    /// Mod multiplier (self * multiplier) % modulus
    pub fn mod_mul(&self, multiplier: &BigNumber, modulus: &BigNumber) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::{BigNum, BigNumContext};
            let mut new_self = Box::new(BigNum::new().unwrap());
            let myself_ptr: &BigNum = unsafe { &*(self.0 as *const BigNum) };
            let multiplier_ptr: &BigNum = unsafe { &*(multiplier.0 as *const BigNum) };
            let modulus_ptr: &BigNum = unsafe { &*(modulus.0 as *const BigNum) };
            new_self
                .mod_mul(
                    myself_ptr,
                    multiplier_ptr,
                    modulus_ptr,
                    &mut BigNumContext::new().unwrap(),
                )
                .unwrap();
            let rwptr = Box::into_raw(new_self);
            Self(rwptr as u64)
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_mod_mul(
                    self_ptr: *const u64,
                    multiplier_ptr: *const u64,
                    modulus_ptr: *const u64,
                    bignum_ptr: *mut u64,
                ) -> u64;
            }
            let mut bignum_ptr = 0u64;
            unsafe {
                sol_bignum_mod_mul(
                    &self.0 as *const _ as *const u64,
                    &multiplier.0 as *const _ as *const u64,
                    &modulus.0 as *const _ as *const u64,
                    &mut bignum_ptr as *mut _ as *mut u64,
                );
            }
            Self(bignum_ptr)
        }
    }

    /// Finds the inverse of modulus on self
    pub fn mod_inv(&self, modulus: &BigNumber) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::{BigNum, BigNumContext};
            let mut new_self = Box::new(BigNum::new().unwrap());
            let myself_ptr: &BigNum = unsafe { &*(self.0 as *const BigNum) };
            let modulus_ptr: &BigNum = unsafe { &*(modulus.0 as *const BigNum) };
            new_self
                .mod_inverse(myself_ptr, modulus_ptr, &mut BigNumContext::new().unwrap())
                .unwrap();
            let rwptr = Box::into_raw(new_self);
            Self(rwptr as u64)
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_mod_inv(
                    self_ptr: *const u64,
                    modulus_ptr: *const u64,
                    bignum_ptr: *mut u64,
                ) -> u64;
            }
            let mut bignum_ptr = 0u64;
            unsafe {
                sol_bignum_mod_inv(
                    &self.0 as *const _ as *const u64,
                    &modulus.0 as *const _ as *const u64,
                    &mut bignum_ptr as *mut _ as *mut u64,
                );
            }
            Self(bignum_ptr)
        }
    }

    /// Divide BigNumbers
    pub fn div(&self, rhs: &BigNumber) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::{BigNum, BigNumContext, BigNumRef};
            let my_raw: &BigNum = unsafe { &*(self.0 as *const BigNum) };
            let mut bbox = Box::new(BigNum::new().unwrap());
            let rhs_ptr: &BigNum = unsafe { &*(rhs.0 as *const BigNum) };
            BigNumRef::checked_div(
                &mut *bbox,
                my_raw,
                rhs_ptr,
                &mut BigNumContext::new().unwrap(),
            )
            .unwrap();
            let rwptr = Box::into_raw(bbox);
            Self(rwptr as u64)
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_div(
                    self_ptr: *const u64,
                    rhs_ptr: *const u64,
                    return_ptr: *mut u64,
                ) -> u64;
            }
            let mut bignum_ptr = 0u64;
            unsafe {
                sol_bignum_div(
                    &self.0 as *const _ as *const u64,
                    &rhs.0 as *const _ as *const u64,
                    &mut bignum_ptr as *mut _ as *mut u64,
                );
            }
            Self(bignum_ptr)
        }
    }

    /// Square BigNumbers
    pub fn sqr(&self) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::{BigNum, BigNumContext, BigNumRef};
            let my_raw: &BigNum = unsafe { &*(self.0 as *const BigNum) };
            let mut bbox = Box::new(BigNum::new().unwrap());
            BigNumRef::sqr(&mut *bbox, my_raw, &mut BigNumContext::new().unwrap()).unwrap();
            let rwptr = Box::into_raw(bbox);
            Self(rwptr as u64)
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_sqr(
                    self_ptr: *const u64,
                    modulus_ptr: *const u64,
                ) -> u64;
            }
            let mut bignum_ptr = 0u64;
            unsafe {
                sol_bignum_sqr(
                    &self.0 as *const _ as *const u64,
                    &mut bignum_ptr as *mut _ as *mut u64,
                );
            }
            Self(bignum_ptr)
        }
    }

    /// Square BigNumbers
    pub fn mod_sqr(&self, modulus: &BigNumber) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::{BigNum, BigNumContext, BigNumRef};
            let my_raw: &BigNum = unsafe { &*(self.0 as *const BigNum) };
            let my_mod: &BigNum = unsafe { &*(modulus.0 as *const BigNum) };
            let mut bbox = Box::new(BigNum::new().unwrap());
            BigNumRef::mod_sqr(
                &mut *bbox,
                my_raw,
                my_mod,
                &mut BigNumContext::new().unwrap(),
            )
            .unwrap();
            let rwptr = Box::into_raw(bbox);
            Self(rwptr as u64)
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_mod_sqr(
                    self_ptr: *const u64,
                    modulus_ptr: *const u64,
                    bignum_ptr: *mut u64,
                ) -> u64;
            }
            let mut bignum_ptr = 0u64;
            unsafe {
                sol_bignum_mod_sqr(
                    &self.0 as *const _ as *const u64,
                    &modulus.0 as *const _ as *const u64,
                    &mut bignum_ptr as *mut _ as *mut u64,
                );
            }
            Self(bignum_ptr)
        }
    }

    /// Square BigNumbers
    pub fn exp(&self, exponent: &BigNumber) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::{BigNum, BigNumContext, BigNumRef};
            let my_raw: &BigNum = unsafe { &*(self.0 as *const BigNum) };
            let exponent_ptr: &BigNum = unsafe { &*(exponent.0 as *const BigNum) };
            let mut bbox = Box::new(BigNum::new().unwrap());
            BigNumRef::exp(
                &mut *bbox,
                my_raw,
                exponent_ptr,
                &mut BigNumContext::new().unwrap(),
            )
            .unwrap();
            let rwptr = Box::into_raw(bbox);
            Self(rwptr as u64)
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_exp(
                    self_ptr: *const u64,
                    exponent_ptr: *const u64,
                    bignum_ptr: *mut u64,
                ) -> u64;
            }
            let mut bignum_ptr = 0u64;
            unsafe {
                sol_bignum_exp(
                    &self.0 as *const _ as *const u64,
                    &exponent.0 as *const _ as *const u64,
                    &mut bignum_ptr as *mut _ as *mut u64,
                );
            }
            Self(bignum_ptr)
        }
    }

    /// Compute modular exponentiation (self ^ rhs mod order) and return the result
    pub fn mod_exp(&self, exponent: &BigNumber, modulus: &BigNumber) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::{BigNum, BigNumContext};
            let mut new_self = Box::new(BigNum::new().unwrap());
            let myself_ptr: &BigNum = unsafe { &*(self.0 as *const BigNum) };
            let exponent_ptr: &BigNum = unsafe { &*(exponent.0 as *const BigNum) };
            let modulus_ptr: &BigNum = unsafe { &*(modulus.0 as *const BigNum) };
            new_self
                .mod_exp(
                    myself_ptr,
                    exponent_ptr,
                    modulus_ptr,
                    &mut BigNumContext::new().unwrap(),
                )
                .unwrap();
            let rwptr = Box::into_raw(new_self);
            Self(rwptr as u64)
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_mod_exp(
                    bignum_ptr: *mut u64,
                    self_ptr: *const u64,
                    exponent_ptr: *const u64,
                    modulus_ptr: *const u64,
                ) -> u64;
            }
            let bignum_ptr = &mut 0u64;
            unsafe {
                sol_bignum_mod_exp(
                    &mut *bignum_ptr as *mut _ as *mut u64,
                    &self.0 as *const _ as *const u64,
                    &exponent.0 as *const _ as *const u64,
                    &modulus.0 as *const _ as *const u64,
                );
            }
            Self(*bignum_ptr)
        }
    }
    /// Hashed Generator
    pub fn hashed_generator(&self, a: &BigNumber, n: &BigNumber, nonce: &[u8]) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use hkdf::Hkdf;
            use openssl::bn::{BigNum, BigNumContext};

            let u_bn = unsafe { &*(self.0 as *mut BigNum) };
            let a_bn = unsafe { &*(a.0 as *mut BigNum) };
            let n_bn = unsafe { &*(n.0 as *mut BigNum) };
            println!("u:{}", u_bn);
            println!("a:{}", a_bn);
            println!("n:{}", n_bn);
            // hash_generator
            let mut transcript = u_bn.as_ref().to_vec();
            transcript.append(&mut a_bn.as_ref().to_vec());
            transcript.extend_from_slice(nonce);
            println!("t:{:?}", transcript);
            let length = n_bn.num_bytes();
            println!("len:{}", length);
            let h = Hkdf::<blake3::Hasher>::new(
                Some(b"SST_SALT_HASH_TO_GENERATOR_"),
                transcript.as_slice().as_ref(),
            );
            let mut okm = vec![0u8; length as usize];
            h.expand(b"", &mut okm).unwrap();
            println!("okm:{:?}", okm);
            let okm_bn = BigNum::from_slice(okm.as_slice()).unwrap();

            let mut res_bn = Box::new(BigNum::new().unwrap());
            res_bn
                .mod_sqr(
                    okm_bn.as_ref(),
                    n_bn.as_ref(),
                    &mut BigNumContext::new().unwrap(),
                )
                .unwrap();
            let rwptr = Box::into_raw(res_bn);
            Self(rwptr as u64)
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_hashed_generator(
                    self_ptr: *const u64,
                    a: *const u64,
                    n: *const u64,
                    nonce: *const u64,
                    hg_ptr: *mut u64,
                ) -> u64;
            }
            let mut bignum_ptr = nonce.len() as u64;
            unsafe {
                sol_bignum_hashed_generator(
                    &self.0 as *const _ as *const u64,
                    &a.0 as *const _ as *const u64,
                    &n.0 as *const _ as *const u64,
                    nonce as *const _ as *const u64,
                    &mut bignum_ptr as *mut _ as *mut u64,
                );
            }
            Self(bignum_ptr)
        }
    }

    /// Hash to Prime
    pub fn hash_to_prime(u: &BigNumber, a: &BigNumber, z: &BigNumber, nonce: &[u8]) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use blake3::traits::digest::Digest;
            use openssl::bn::{BigNum, BigNumContext};
            let u_bn = unsafe { &*(u.0 as *mut BigNum) };
            let a_bn = unsafe { &*(a.0 as *mut BigNum) };
            let z_bn = unsafe { &*(z.0 as *mut BigNum) };

            let mut input = u_bn.as_ref().to_vec();
            input.extend_from_slice(&a_bn.as_ref().to_vec());
            input.extend_from_slice(&z_bn.as_ref().to_vec());
            input.extend_from_slice(nonce);
            // println!("t:{:?}",input);

            let mut i = 1u32;
            let offset = input.len();
            input.extend_from_slice(&i.to_be_bytes()[..]);
            let end = input.len();
            let ctx = &mut BigNumContext::new().unwrap();
            let mut num;
            loop {
                let mut hash = blake3::Hasher::digest(input.as_slice());
                // Force it to be odd
                hash[31] |= 1;
                // Only need 256 bits just borrow the bottom 32 bytes
                // There should be plenty of primes below 2^256
                // and we want this to be reasonably fast
                //num = BigNumber::from_bytes(&hash[32..]);
                num = BigNum::from_slice(&hash).unwrap();
                if num.is_prime(15, ctx).unwrap() {
                    // msg!("num_bytes:{:?}",num.to_bytes().to_vec());
                    // msg!("i:{}",i);
                    break;
                }
                i += 1;
                let i_bytes = i.to_be_bytes();
                input[offset..end].clone_from_slice(&i_bytes[..]);
            }
            let res_bn = Box::new(num);
            let rwptr = Box::into_raw(res_bn);
            Self(rwptr as u64)
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_hash_to_prime(
                    u: *const u64,
                    a: *const u64,
                    z: *const u64,
                    nonce: *const u64,
                    htp_ptr: *mut u64,
                ) -> u64;
            }
            let mut bignum_ptr = nonce.len() as u64;
            unsafe {
                sol_bignum_hash_to_prime(
                    &u.0 as *const _ as *const u64,
                    &a.0 as *const _ as *const u64,
                    &z.0 as *const _ as *const u64,
                    nonce as *const _ as *const u64,
                    &mut bignum_ptr as *mut _ as *mut u64,
                );
            }
            Self(bignum_ptr)
        }
    }

    /// Log a `BigNum` from a program
    pub fn log(&self) {
        #[cfg(not(target_arch = "bpf"))]
        crate::program_stubs::sol_log(&self.to_string());
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_log_bignum(bignum_addr: *const u64) -> u64;
            }
            unsafe { sol_log_bignum(&self.0 as *const _ as *const u64) };
        }
    }
}

impl fmt::Debug for BigNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::BigNum;
            let braw: &BigNum = unsafe { &*(self.0 as *mut BigNum) };
            write!(f, "{}", braw)
        }
        #[cfg(target_arch = "bpf")]
        {
            write!(f, "{}", 0)
        }
    }
}

impl fmt::Display for BigNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::BigNum;
            let braw: &BigNum = unsafe { &*(self.0 as *mut BigNum) };
            write!(f, "{}", braw)
        }
        #[cfg(target_arch = "bpf")]
        {
            write!(f, "{}", 0)
        }
    }
}

impl AsRef<u64> for BigNumber {
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

impl From<u64> for BigNumber {
    fn from(ptr: u64) -> Self {
        Self(ptr)
    }
}

/// Drop - removes the underlying BigNum
impl Drop for BigNumber {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::BigNum;
            drop(unsafe { Box::from_raw(self.0 as *mut BigNum) });
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_drop(bignum_ptr: *mut u64) -> u64;
            }
            unsafe {
                sol_bignum_drop(&mut self.0 as *mut _ as *mut u64);
            }
        }
    }
}
/// Clone - creates a new BigNumber replicating itself
impl Clone for BigNumber {
    fn clone(&self) -> Self {
        BigNumber::from_bytes(self.to_bytes().as_slice())
    }
}

/// Returns an array of bytes (big endian) of self
pub fn blake3_digest(data: &[u8]) -> Vec<u8> {
    #[cfg(not(target_arch = "bpf"))]
    {
        use blake3::traits::digest::Digest;
        blake3::Hasher::digest(data).to_vec()
    }
    #[cfg(target_arch = "bpf")]
    {
        extern "C" {
            fn sol_blake3_digest(
                data_ptr: *const u8,
                data_len: *const u64,
                digest_ptr: *mut u8,
                digest_len: *mut u64,
            ) -> u64;
        }
        let data_len = data.len();
        let mut digest = [0u8; 32];
        let mut digest_len = digest.len();
        unsafe {
            sol_blake3_digest(
                data.as_ptr() as *const _ as *const u8,
                &data_len as *const _ as *const u64,
                digest.as_mut_ptr() as *mut _ as *mut u8,
                &mut digest_len as *mut _ as *mut u64,
            );
        };
        digest[0..digest_len].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_bignumber_new() {
        let bn1 = BigNumber::new();
        println!("{:?}", bn1);
    }

    #[test]
    fn test_bignumber_hashed_generator() {
        let bn_u = BigNumber::from_u32(11);
        let bn_a = BigNumber::from_u32(59);
        let bn_n = BigNumber::from_u32(7);
        let nonce = b"hgen";
        let result = bn_u.hashed_generator(&bn_a, &bn_n, nonce);
        println!("Hashed generator result = {}", result);
    }

    #[test]
    fn test_bignumber_clone() {
        let bn_u = BigNumber::from_u32(11);
        let bn_u2 = bn_u.clone();
        println!("bn_u = {}", bn_u);
        println!("bn_u2 = {}", bn_u2);
    }
}
