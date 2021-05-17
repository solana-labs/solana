//! @brief Solana Rust-based BigNumber for bpf-programs

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use std::fmt;

#[repr(C)]
#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    AbiExample,
)]
pub struct BigNumber {
    value: Vec<u8>,
    negative: bool,
}

// pub struct BigNumber(u64);

impl BigNumber {
    /// Returns a BigNumber with initial value of 0
    pub fn new() -> Self {
        let mut value = Vec::<u8>::new();
        value.push(0u8);
        Self {
            negative: false,
            value,
        }
    }

    /// Returns the size, in bytes, of BigNum. Typically used in
    /// assessing buffer size needed to fetch the bytes for serialization, etc.
    pub fn size_in_bytes(&self) -> usize {
        self.value.len()
    }

    /// Returns a BigNumber with initial value set to a u32 value
    pub fn from_u32(val: u32) -> Self {
        if val == 0 {
            let mut value = Vec::<u8>::new();
            value.push(0);
            Self {
                negative: false,
                value,
            }
        } else {
            #[cfg(not(target_arch = "bpf"))]
            {
                use openssl::bn::BigNum;
                let value = BigNum::from_u32(val).unwrap().to_vec();
                Self {
                    negative: false,
                    value,
                }
            }
            #[cfg(target_arch = "bpf")]
            {
                extern "C" {
                    fn sol_bignum_from_u32(bytes_ptr: *mut u8, bytes_len: u64, val: u64) -> u64;
                }
                let vec_size = {
                    match val {
                        0..=255 => 1,
                        256..=65_535 => 2,
                        65_536..=16_777_215 => 3,
                        _ => 4,
                    }
                };
                let mut value = Vec::<u8>::with_capacity(vec_size);
                unsafe {
                    sol_bignum_from_u32(
                        value.as_mut_ptr() as *mut _ as *mut u8,
                        vec_size as u64,
                        val as u64,
                    );
                    value.set_len(vec_size);
                }
                Self {
                    negative: false,
                    value,
                }
            }
        }
    }
    /// Returns a BigNumber from a decimal string
    pub fn from_dec_str(string: &str) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::BigNum;
            let bn = BigNum::from_dec_str(string).unwrap();
            let value = bn.to_vec();
            Self {
                negative: bn.is_negative(),
                value,
            }
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_from_dec_str(
                    in_dec_str_ptr: *const u64,
                    in_size: u64,
                    out_ptr: *mut u8,
                    out_size_ptr: *mut u64,
                    out_is_neg_flag_ptr: *mut u64,
                ) -> u64;
            }
            let mut value = Vec::<u8>::with_capacity(string.len());
            let mut value_len = string.len() as u64;
            let mut is_negative = 0u64;
            unsafe {
                sol_bignum_from_dec_str(
                    string.as_ptr() as *const _ as *const u64,
                    string.len() as u64,
                    value.as_mut_ptr() as *mut _ as *mut u8,
                    &mut value_len as *mut _ as *mut u64,
                    &mut is_negative as *mut _ as *mut u64,
                );
                value.set_len(value_len as usize);
            }
            Self {
                negative: is_negative != 0,
                value,
            }
        }
    }

    /// Flag indicating if negative or not
    pub fn is_negative(&self) -> bool {
        self.negative
    }

    /// Returns an array of bytes of self
    pub fn to_bytes(&self) -> &[u8] {
        &self.value
    }

    /// Add BigNumbers
    pub fn add(&self, rhs: &BigNumber) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::BigNum;
            let my_num = BigNum::from_slice(&self.value).unwrap();
            let rhs_num = BigNum::from_slice(&rhs.value).unwrap();
            let mut bn_res = BigNum::new().unwrap();
            bn_res.checked_add(&my_num, &rhs_num).unwrap();
            let value = bn_res.as_ref().to_vec();
            Self {
                negative: bn_res.is_negative(),
                value,
            }
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_add(
                    arg_address: *const u64,
                    arg_count: u64,
                    out_value_address: *mut u8,
                    out_neg_address: *mut u64,
                    out_size_address: *mut u64,
                ) -> u64;
            }
            // Setup the argument array (3)
            let neg_values = vec![self.negative as u8, rhs.negative as u8];
            let arg_array = vec![self.to_bytes(), rhs.to_bytes(), &neg_values];
            // Setup the result information
            let mut value_len = std::cmp::max(self.value.len(), rhs.value.len()) + 1;
            let mut value = Vec::<u8>::with_capacity(value_len as usize);
            let mut is_negative = 0u64;
            unsafe {
                sol_bignum_add(
                    arg_array.as_ptr() as *const _ as *const u64,
                    arg_array.len() as u64,
                    value.as_mut_ptr() as *mut _ as *mut u8,
                    &mut is_negative as *mut _ as *mut u64,
                    &mut value_len as *mut _ as *mut u64,
                );
                value.set_len(value_len as usize);
            }
            Self {
                negative: is_negative != 0,
                value,
            }
        }
    }
    /// Subtract BigNumbers
    pub fn sub(&self, rhs: &BigNumber) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::BigNum;
            let my_num = BigNum::from_slice(&self.value).unwrap();
            let rhs_num = BigNum::from_slice(&rhs.value).unwrap();
            let mut bn_res = BigNum::new().unwrap();
            bn_res.checked_sub(&my_num, &rhs_num).unwrap();
            let value = bn_res.as_ref().to_vec();
            Self {
                negative: bn_res.is_negative(),
                value,
            }
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_sub(
                    arg_address: *const u64,
                    arg_count: u64,
                    out_value_address: *mut u8,
                    out_neg_address: *mut u64,
                    out_size_address: *mut u64,
                ) -> u64;
            }
            // Setup the argument array (3)
            let neg_values = vec![self.negative as u8, rhs.negative as u8];
            let arg_array = vec![self.to_bytes(), rhs.to_bytes(), &neg_values];
            // Setup the result information
            let mut value_len = std::cmp::max(self.value.len(), rhs.value.len()) + 1;
            let mut value = Vec::<u8>::with_capacity(value_len as usize);
            let mut is_negative = 0u64;
            unsafe {
                sol_bignum_sub(
                    arg_array.as_ptr() as *const _ as *const u64,
                    arg_array.len() as u64,
                    value.as_mut_ptr() as *mut _ as *mut u8,
                    &mut is_negative as *mut _ as *mut u64,
                    &mut value_len as *mut _ as *mut u64,
                );
                value.set_len(value_len as usize);
            }
            Self {
                negative: is_negative != 0,
                value,
            }
        }
    }

    /// Multiple BigNumbers
    pub fn mul(&self, rhs: &BigNumber) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::{BigNum, BigNumContext};
            let my_num = BigNum::from_slice(&self.value).unwrap();
            let rhs_num = BigNum::from_slice(&rhs.value).unwrap();
            let mut bn_res = BigNum::new().unwrap();
            bn_res.checked_mul(&my_num, &rhs_num, &mut BigNumContext::new().unwrap()).unwrap();
            let value = bn_res.as_ref().to_vec();
            Self {
                negative: bn_res.is_negative(),
                value,
            }
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_mul(
                    arg_address: *const u64,
                    arg_count: u64,
                    out_value_address: *mut u8,
                    out_neg_address: *mut u64,
                    out_size_address: *mut u64,
                ) -> u64;
            }
            // Setup the argument array (3)
            let neg_values = vec![self.negative as u8, rhs.negative as u8];
            let arg_array = vec![self.to_bytes(), rhs.to_bytes(), &neg_values];
            // Setup the result information
            let mut value_len = std::cmp::max(self.value.len(), rhs.value.len()) + 1;
            let mut value = Vec::<u8>::with_capacity(value_len as usize);
            let mut is_negative = 0u64;
            unsafe {
                sol_bignum_mul(
                    arg_array.as_ptr() as *const _ as *const u64,
                    arg_array.len() as u64,
                    value.as_mut_ptr() as *mut _ as *mut u8,
                    &mut is_negative as *mut _ as *mut u64,
                    &mut value_len as *mut _ as *mut u64,
                );
                value.set_len(value_len as usize);
            }
            Self {
                negative: is_negative != 0,
                value,
            }
        }
    }
    /// Divide BigNumbers
    pub fn div(&self, rhs: &BigNumber) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::{BigNum, BigNumContext};
            let my_num = BigNum::from_slice(&self.value).unwrap();
            let rhs_num = BigNum::from_slice(&rhs.value).unwrap();
            let mut bn_res = BigNum::new().unwrap();
            bn_res.checked_div(&my_num, &rhs_num, &mut BigNumContext::new().unwrap()).unwrap();
            let value = bn_res.as_ref().to_vec();
            Self {
                negative: bn_res.is_negative(),
                value,
            }
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_div(
                    arg_address: *const u64,
                    arg_count: u64,
                    out_value_address: *mut u8,
                    out_neg_address: *mut u64,
                    out_size_address: *mut u64,
                ) -> u64;
            }
            // Setup the argument array (3)
            let neg_values = vec![self.negative as u8, rhs.negative as u8];
            let arg_array = vec![self.to_bytes(), rhs.to_bytes(), &neg_values];
            // Setup the result information
            let mut value_len = std::cmp::max(self.value.len(), rhs.value.len()) + 1;
            let mut value = Vec::<u8>::with_capacity(value_len as usize);
            let mut is_negative = 0u64;
            unsafe {
                sol_bignum_div(
                    arg_array.as_ptr() as *const _ as *const u64,
                    arg_array.len() as u64,
                    value.as_mut_ptr() as *mut _ as *mut u8,
                    &mut is_negative as *mut _ as *mut u64,
                    &mut value_len as *mut _ as *mut u64,
                );
                value.set_len(value_len as usize);
            }
            Self {
                negative: is_negative != 0,
                value,
            }
        }
    }

    /// Square BigNumbers
    pub fn sqr(&self) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::{BigNum, BigNumContext};
            let my_num = BigNum::from_slice(&self.value).unwrap();
            let mut bn_res = BigNum::new().unwrap();
            bn_res.sqr(&my_num, &mut BigNumContext::new().unwrap()).unwrap();
            let value = bn_res.as_ref().to_vec();
            Self {
                negative: bn_res.is_negative(),
                value,
            }
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_sqr(
                    arg_address: *const u64,
                    arg_count: u64,
                    out_value_address: *mut u8,
                    out_neg_address: *mut u64,
                    out_size_address: *mut u64,
                ) -> u64;
            }
            // Setup the argument array (3)
            let neg_values = vec![self.negative as u8];
            let arg_array = vec![self.to_bytes(), &neg_values];
            // Setup the result information
            let mut value_len = self.value.len()*2;
            let mut value = Vec::<u8>::with_capacity(value_len as usize);
            let mut is_negative = 0u64;

            unsafe {
                sol_bignum_sqr(
                    arg_array.as_ptr() as *const _ as *const u64,
                    arg_array.len() as u64,
                    value.as_mut_ptr() as *mut _ as *mut u8,
                    &mut is_negative as *mut _ as *mut u64,
                    &mut value_len as *mut _ as *mut u64,
                );
                value.set_len(value_len as usize);
            }
            Self {
                negative: is_negative != 0,
                value,
            }
        }
    }

    /// Raise BigNumber to exponent
    pub fn exp(&self, exponent: &BigNumber) -> Self {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::{BigNum, BigNumContext};
            let my_num = BigNum::from_slice(&self.value).unwrap();
            let rhs_num = BigNum::from_slice(&exponent.value).unwrap();
            let mut bn_res = BigNum::new().unwrap();
            bn_res.exp(&my_num, &rhs_num, &mut BigNumContext::new().unwrap()).unwrap();
            let value = bn_res.as_ref().to_vec();
            Self {
                negative: bn_res.is_negative(),
                value,
            }
        }
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_bignum_exp(
                    arg_address: *const u64,
                    arg_count: u64,
                    out_value_address: *mut u8,
                    out_neg_address: *mut u64,
                    out_size_address: *mut u64,
                ) -> u64;
            }
            // Setup the argument array (3)
            let neg_values = vec![self.negative as u8, exponent.negative as u8];
            let arg_array = vec![self.to_bytes(), exponent.to_bytes(), &neg_values];
            // Setup the result information
            let mut value_len = std::cmp::max(self.value.len(), exponent.value.len()) + 1;
            let mut value = Vec::<u8>::with_capacity(value_len as usize);
            let mut is_negative = 0u64;
            unsafe {
                sol_bignum_exp(
                    arg_array.as_ptr() as *const _ as *const u64,
                    arg_array.len() as u64,
                    value.as_mut_ptr() as *mut _ as *mut u8,
                    &mut is_negative as *mut _ as *mut u64,
                    &mut value_len as *mut _ as *mut u64,
                );
                value.set_len(value_len as usize);
            }
            Self {
                negative: is_negative != 0,
                value,
            }
        }
    }

    // /// Mod multiplier (self * multiplier) % modulus
    // pub fn mod_mul(&self, multiplier: &BigNumber, modulus: &BigNumber) -> Self {
    //     #[cfg(not(target_arch = "bpf"))]
    //     {
    //         use openssl::bn::{BigNum, BigNumContext};
    //         let mut new_self = Box::new(BigNum::new().unwrap());
    //         let myself_ptr: &BigNum = unsafe { &*(self.0 as *const BigNum) };
    //         let multiplier_ptr: &BigNum = unsafe { &*(multiplier.0 as *const BigNum) };
    //         let modulus_ptr: &BigNum = unsafe { &*(modulus.0 as *const BigNum) };
    //         new_self
    //             .mod_mul(
    //                 myself_ptr,
    //                 multiplier_ptr,
    //                 modulus_ptr,
    //                 &mut BigNumContext::new().unwrap(),
    //             )
    //             .unwrap();
    //         let rwptr = Box::into_raw(new_self);
    //         Self(rwptr as u64)
    //     }
    //     #[cfg(target_arch = "bpf")]
    //     {
    //         extern "C" {
    //             fn sol_bignum_mod_mul(
    //                 self_ptr: *const u64,
    //                 multiplier_ptr: *const u64,
    //                 modulus_ptr: *const u64,
    //                 bignum_ptr: *mut u64,
    //             ) -> u64;
    //         }
    //         let mut bignum_ptr = 0u64;
    //         unsafe {
    //             sol_bignum_mod_mul(
    //                 &self.0 as *const _ as *const u64,
    //                 &multiplier.0 as *const _ as *const u64,
    //                 &modulus.0 as *const _ as *const u64,
    //                 &mut bignum_ptr as *mut _ as *mut u64,
    //             );
    //         }
    //         Self(bignum_ptr)
    //     }
    // }

    // /// Finds the inverse of modulus on self
    // pub fn mod_inv(&self, modulus: &BigNumber) -> Self {
    //     #[cfg(not(target_arch = "bpf"))]
    //     {
    //         use openssl::bn::{BigNum, BigNumContext};
    //         let mut new_self = Box::new(BigNum::new().unwrap());
    //         let myself_ptr: &BigNum = unsafe { &*(self.0 as *const BigNum) };
    //         let modulus_ptr: &BigNum = unsafe { &*(modulus.0 as *const BigNum) };
    //         new_self
    //             .mod_inverse(myself_ptr, modulus_ptr, &mut BigNumContext::new().unwrap())
    //             .unwrap();
    //         let rwptr = Box::into_raw(new_self);
    //         Self(rwptr as u64)
    //     }
    //     #[cfg(target_arch = "bpf")]
    //     {
    //         extern "C" {
    //             fn sol_bignum_mod_inv(
    //                 self_ptr: *const u64,
    //                 modulus_ptr: *const u64,
    //                 bignum_ptr: *mut u64,
    //             ) -> u64;
    //         }
    //         let mut bignum_ptr = 0u64;
    //         unsafe {
    //             sol_bignum_mod_inv(
    //                 &self.0 as *const _ as *const u64,
    //                 &modulus.0 as *const _ as *const u64,
    //                 &mut bignum_ptr as *mut _ as *mut u64,
    //             );
    //         }
    //         Self(bignum_ptr)
    //     }
    // }

    // /// Square BigNumbers
    // pub fn mod_sqr(&self, modulus: &BigNumber) -> Self {
    //     #[cfg(not(target_arch = "bpf"))]
    //     {
    //         use openssl::bn::{BigNum, BigNumContext, BigNumRef};
    //         let my_raw: &BigNum = unsafe { &*(self.0 as *const BigNum) };
    //         let my_mod: &BigNum = unsafe { &*(modulus.0 as *const BigNum) };
    //         let mut bbox = Box::new(BigNum::new().unwrap());
    //         BigNumRef::mod_sqr(
    //             &mut *bbox,
    //             my_raw,
    //             my_mod,
    //             &mut BigNumContext::new().unwrap(),
    //         )
    //         .unwrap();
    //         let rwptr = Box::into_raw(bbox);
    //         Self(rwptr as u64)
    //     }
    //     #[cfg(target_arch = "bpf")]
    //     {
    //         extern "C" {
    //             fn sol_bignum_mod_sqr(
    //                 self_ptr: *const u64,
    //                 modulus_ptr: *const u64,
    //                 bignum_ptr: *mut u64,
    //             ) -> u64;
    //         }
    //         let mut bignum_ptr = 0u64;
    //         unsafe {
    //             sol_bignum_mod_sqr(
    //                 &self.0 as *const _ as *const u64,
    //                 &modulus.0 as *const _ as *const u64,
    //                 &mut bignum_ptr as *mut _ as *mut u64,
    //             );
    //         }
    //         Self(bignum_ptr)
    //     }
    // }

    // /// Compute modular exponentiation (self ^ rhs mod order) and return the result
    // pub fn mod_exp(&self, exponent: &BigNumber, modulus: &BigNumber) -> Self {
    //     #[cfg(not(target_arch = "bpf"))]
    //     {
    //         use openssl::bn::{BigNum, BigNumContext};
    //         let mut new_self = Box::new(BigNum::new().unwrap());
    //         let myself_ptr: &BigNum = unsafe { &*(self.0 as *const BigNum) };
    //         let exponent_ptr: &BigNum = unsafe { &*(exponent.0 as *const BigNum) };
    //         let modulus_ptr: &BigNum = unsafe { &*(modulus.0 as *const BigNum) };
    //         new_self
    //             .mod_exp(
    //                 myself_ptr,
    //                 exponent_ptr,
    //                 modulus_ptr,
    //                 &mut BigNumContext::new().unwrap(),
    //             )
    //             .unwrap();
    //         let rwptr = Box::into_raw(new_self);
    //         Self(rwptr as u64)
    //     }
    //     #[cfg(target_arch = "bpf")]
    //     {
    //         extern "C" {
    //             fn sol_bignum_mod_exp(
    //                 bignum_ptr: *mut u64,
    //                 self_ptr: *const u64,
    //                 exponent_ptr: *const u64,
    //                 modulus_ptr: *const u64,
    //             ) -> u64;
    //         }
    //         let bignum_ptr = &mut 0u64;
    //         unsafe {
    //             sol_bignum_mod_exp(
    //                 &mut *bignum_ptr as *mut _ as *mut u64,
    //                 &self.0 as *const _ as *const u64,
    //                 &exponent.0 as *const _ as *const u64,
    //                 &modulus.0 as *const _ as *const u64,
    //             );
    //         }
    //         Self(*bignum_ptr)
    //     }
    // }

    // /// Log a `BigNum` from a program
    // pub fn log(&self) {
    //     #[cfg(not(target_arch = "bpf"))]
    //     crate::program_stubs::sol_log(&self.to_string());
    //     #[cfg(target_arch = "bpf")]
    //     {
    //         extern "C" {
    //             fn sol_bignum_log(bignum_addr: *const u64) -> u64;
    //         }
    //         unsafe { sol_bignum_log(&self.0 as *const _ as *const u64) };
    //     }
    // }
}

impl Default for BigNumber {
    fn default() -> Self {
        let mut value = Vec::<u8>::new();
        value.push(0);
        Self {
            negative: false,
            value,
        }
    }
}
impl fmt::Debug for BigNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        #[cfg(not(target_arch = "bpf"))]
        {
            use openssl::bn::BigNum;
            let braw = BigNum::from_slice(&self.value)
                .unwrap()
                .to_dec_str()
                .unwrap();
            // let braw: &BigNum = unsafe { &*(self.0 as *mut BigNum) };
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
            let braw = BigNum::from_slice(&self.value)
                .unwrap()
                .to_dec_str()
                .unwrap();
            // let braw: &BigNum = unsafe { &*(self.0 as *mut BigNum) };
            write!(f, "{}", braw)
        }
        #[cfg(target_arch = "bpf")]
        {
            write!(f, "{}", 0)
        }
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
