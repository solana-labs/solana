//! @brief Solana Rust-based BigNumber for bpf-programs

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use std::fmt;

#[repr(transparent)]
#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
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

    /// Returns the size, in bytes, of BigNum. Typically used in
    /// assessing buffer size needed to fetch the bytes for serialization, etc.
    pub fn size_in_bytes(&self) -> usize {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::BigNum;
            let self_bignum: &BigNum = unsafe { &*(self.0 as *const BigNum) };
            self_bignum.num_bytes() as usize
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_size_in_bytes(bignum_ptr: *const u64, bytes_len: *mut u64) -> u64;
            }
            let mut bignum_size = 0u64;
            unsafe {
                sol_bignum_size_in_bytes(
                    &self.0 as *const _ as *const u64,
                    &mut bignum_size as *mut _ as *mut u64,
                );
            }
            bignum_size as usize
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

    /// Returns an array of bytes of self
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
            // Get the BigNum size, allocate result vector
            let mut bignum_size = self.size_in_bytes() as u64;
            let mut my_buffer = Vec::<u8>::with_capacity(bignum_size as usize);
            unsafe {
                sol_bignum_to_bytes(
                    &self.0 as *const _ as *const u64,
                    my_buffer.as_mut_ptr() as *mut _ as *mut u8,
                    &mut bignum_size as *mut _ as *mut u64,
                );
                my_buffer.set_len(bignum_size as usize);
            }
            my_buffer.clone()
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
                fn sol_bignum_sqr(self_ptr: *const u64, modulus_ptr: *const u64) -> u64;
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_bignumber_new() {
        let bn1 = BigNumber::new();
        assert_eq!(bn1.to_bytes(), BigNumber::new().to_bytes());
    }

    #[test]
    fn test_bignumber_clone() {
        let bn_u = BigNumber::from_u32(11);
        let bn_u2 = bn_u.clone();
        assert_eq!(bn_u2.to_bytes(), bn_u.to_bytes());
    }
}
