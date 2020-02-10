//! Zero-allocation U256 and U512 types built on GMP. We created this module specifically for our
//! use case of implementing primality checking over 256-bit integers, but it may be worth
//! polishing a bit for more general use.
//!
//! Obviously there are a lot of `unsafe` blocks to work with GMP. Take care when using this module
//! because there may be bugs we did not catch.
//!
//! TODO: Benchmark our U256 vs. 256-bit `rug::Integer` vs. Parity U256.
#![allow(clippy::cast_sign_loss)]

use gmp_mpfr_sys::gmp;
use gmp_mpfr_sys::gmp::mpz_t;
use rug::integer::Order;
use rug::Integer;
use std::cmp::{min, Ord, Ordering, PartialOrd};
use std::convert::From;
use std::mem::transmute;
use std::ops;

macro_rules! u_types {
  ($($t:ident,$size:expr),+) => {
    $(
      #[derive(PartialEq, Eq, Hash, Debug, Clone, Copy)]
      pub struct $t {
        // Field `size` also denotes the sign of the number, while `limbs` reflect only the
        // magnitude.
        // We keep size >= 0 except in very rare circumstances.
        size: i64,
        limbs: [u64; $size],
      }

      impl $t {
        fn data(&self) -> *mut u64 {
          &self.limbs as *const u64 as *mut u64
        }

        fn normalize_size(&mut self) {
          self.size = 0;
          for i in (0..$size).rev() {
            if self.limbs[i] != 0 {
              self.size = (i + 1) as i64;
              break;
            }
          }
        }

        // The cast from `i64` to `i32` is fine since |`size`| is <= 4 for `U256`, and <= 8 for
        // `U512`.
        #[allow(clippy::cast_possible_truncation)]
        fn as_mpz(&self) -> mpz_t {
          mpz_t {
            size: self.size as i32,
            d: self.data(),
            alloc: $size,
          }
        }

        pub fn zero() -> Self {
          Self { size: 0, limbs: [0; $size] }
        }

        pub fn is_zero(&self) -> bool {
          self.size == 0
        }

        pub fn one() -> Self {
          let mut limbs = [0; $size];
          limbs[0] = 1;
          Self { size: 1, limbs }
        }

        pub fn is_odd(&self) -> bool {
          self.limbs[0] & 1 == 1
        }

        #[allow(clippy::if_not_else)]
        /// Panics if `m == 0`.
        pub fn mod_inv(self, m: &Self) -> Option<Self> {
          let mut out = Self::zero();
          let outmpz = out.as_mpz();
          let s = self.as_mpz();
          let m = m.as_mpz();
          let exists = unsafe { gmp::mpz_invert(mut_ptr(&outmpz), mut_ptr(&s), mut_ptr(&m)) };
          if exists != 0 {
            out.size = i64::from(outmpz.size);
            Some(out)
          }
          else {
            None
          }
        }

        /// Panics if `m == 0`.
        pub fn pow_mod(self, e: Self, m: &Self) -> Self {
          let mut out = Self::zero();
          let outmpz = out.as_mpz();
          let s = self.as_mpz();
          let e = e.as_mpz();
          let m = m.as_mpz();
          unsafe { gmp::mpz_powm(mut_ptr(&outmpz), mut_ptr(&s), mut_ptr(&e), mut_ptr(&m)) };
          out.size = i64::from(outmpz.size);
          out
        }

        pub fn is_perfect_square(&self) -> bool {
          let issqr = unsafe { gmp::mpn_perfect_square_p(self.data(), self.size) };
          issqr != 0
        }

        pub fn jacobi(a: i32, b: &Self) -> i32 {
          let mut a_data = 0;
          let a = i32_to_mpz(a, &mut a_data);
          let b = b.as_mpz();
          unsafe { gmp::mpz_jacobi(&a as *const mpz_t, &b as *const mpz_t) }
        }

        pub fn is_congruent(self, i: i32, m: &Self) -> bool {
          let mut data = 0;
          let x = i32_to_mpz(i, &mut data);
          let s = self.as_mpz();
          let m = m.as_mpz();
          let res = unsafe { gmp::mpz_congruent_p(mut_ptr(&s), mut_ptr(&x), mut_ptr(&m)) };
          res != 0
        }

        pub fn is_divisible_u(&self, u: u64) -> bool {
          let s = self.as_mpz();
          let divisible = unsafe {gmp::mpz_divisible_ui_p(mut_ptr(&s), u)};
          divisible != 0
        }

        /// Panics if `buf` is not large enough.
        pub fn write_binary(&self, buf: &mut [u8]) -> usize {
          unsafe { gmp::mpn_get_str(mut_ptr(&buf[0]), 2, self.data(), self.size) }
        }

        pub fn from_be_bytes(bytes: &[u8]) -> Self {
          let x = Self::zero();
          unsafe { gmp::mpn_set_str(x.data(), &bytes[0] as *const u8, bytes.len(), 256) };
          x
        }
      }

      impl PartialEq<u64> for $t {
        fn eq(&self, u: &u64) -> bool {
          (*u == 0 && self.size == 0) || (self.size == 1 && self.limbs[0] == *u)
        }
      }

      impl From<[u64; $size]> for $t {
        fn from(limbs: [u64; $size]) -> Self {
          let mut x = Self { size: 0, limbs };
          x.normalize_size();
          x
        }
      }

      impl From<u64> for $t {
        fn from(x: u64) -> Self {
          let mut limbs = [0; $size];
          limbs[0] = x;
          Self::from(limbs)
        }
      }

      /// Lower-endian `bytes`.
      impl From<[u8; $size * 8]> for $t {
        fn from(bytes: [u8; $size * 8]) -> Self {
          let chunks = unsafe { transmute::<[u8; $size * 8], [[u8; 8]; $size]>(bytes) };
          let mut limbs = [0; $size];
          for i in 0..$size {
            limbs[i] = u64::from_le_bytes(chunks[i]);
          }
          Self::from(limbs)
        }
      }

      /// Lower-endian `bytes`.
      impl From<&[u8; $size * 8]> for $t {
        fn from(bytes: &[u8; $size * 8]) -> Self {
          let chunks = unsafe { transmute::<[u8; $size * 8], [[u8; 8]; $size]>(*bytes) };
          let mut limbs = [0; $size];
          for i in 0..$size {
            limbs[i] = u64::from_le_bytes(chunks[i]);
          }
          Self::from(limbs)
        }
      }

      impl PartialOrd for $t {
        fn partial_cmp(&self, x: &Self) -> Option<Ordering> {
          let x = unsafe { gmp::mpn_cmp(self.data(), x.data(), $size) };
          Some({
            if x < 0 {
              Ordering::Less
            } else if x == 0 {
              Ordering::Equal
            } else {
              Ordering::Greater
            }
          })
        }
      }

      impl Ord for $t {
        fn cmp(&self, x: &Self) -> Ordering {
          let x = unsafe { gmp::mpn_cmp(self.data(), x.data(), $size) };
          if x < 0 {
            Ordering::Less
          } else if x == 0 {
            Ordering::Equal
          } else {
            Ordering::Greater
          }
        }
      }

      impl ops::ShlAssign<u32> for $t {
        fn shl_assign(&mut self, mut x: u32) {
          while x != 0 {
            let sz = min(gmp::LIMB_BITS as u32, x);
            x -= sz;
            unsafe { gmp::mpn_lshift(self.data(), self.data(), $size, sz) };
          }
          self.normalize_size();
        }
      }

      impl ops::Shl<u32> for $t {
        type Output = Self;
        fn shl(self, x: u32) -> Self {
          let mut y = self;
          y <<= x;
          y
        }
      }

      impl ops::ShrAssign<u32> for $t {
        fn shr_assign(&mut self, mut x: u32) {
          while x != 0 {
            let sz = min(gmp::LIMB_BITS as u32, x);
            x -= sz;
            unsafe { gmp::mpn_rshift(self.data(), self.data(), $size, sz) };
          }
          self.normalize_size();
        }
      }

      impl ops::Shr<u32> for $t {
        type Output = Self;
        fn shr(self, x: u32) -> Self {
          let mut y = self;
          y >>= x;
          y
        }
      }

      impl ops::AddAssign for $t {
        /// Panics if result overflows.
        fn add_assign(&mut self, x: Self) {
          let carry = unsafe { gmp::mpn_add_n(self.data(), self.data(), x.data(), $size) };
          assert!(carry == 0);
          self.normalize_size();
        }
      }

      impl ops::Add for $t {
        /// Panics if result overflows.
        type Output = Self;
        fn add(self, x: Self) -> Self {
          let mut y = self;
          y += x;
          y
        }
      }
      impl ops::Add<u64> for $t {
        type Output = Self;
        /// Panics if result overflows.
        fn add(self, x: u64) -> Self {
          self + Self::from(x)
        }
      }

      impl ops::SubAssign for $t {
        /// Panics if result is negative.
        fn sub_assign(&mut self, x: Self) {
          let borrow = unsafe { gmp::mpn_sub_n(self.data(), self.data(), x.data(), $size) };
          assert!(borrow == 0);
          self.normalize_size();
        }
      }

      impl ops::Sub for $t {
        type Output = Self;
        /// Panics if result is negative.
        fn sub(self, x: Self) -> Self {
          let mut y = self;
          y -= x;
          y
        }
      }

      impl ops::Sub<u64> for $t {
        type Output = Self;
        /// Panics if result is negative.
        fn sub(self, x: u64) -> Self {
          self - Self::from(x)
        }
      }

      impl ops::Sub<u64> for &$t {
        type Output = $t;
        /// Panics if result is negative.
        fn sub(self, x: u64) -> $t {
          *self - $t::from(x)
        }
      }

      impl ops::Rem<&Self> for $t {
        type Output = Self;
        fn rem(self, x: &Self) -> Self {
          if x.size > self.size {
            return self;
          }
          let (y, mut rem) = (Self::zero(), Self::zero());
          unsafe {
            gmp::mpn_tdiv_qr(
              y.data(),
              rem.data(),
              0,
              self.data(),
              self.size,
              x.data(),
              x.size,
            )
          };
          rem.normalize_size();
          rem
        }
      }

      impl ops::Rem for $t {
        type Output = Self;
        fn rem(self, x: Self) -> Self {
          #![allow(clippy::op_ref)]
          self % &x
        }
      }

      impl ops::RemAssign<&Self> for $t {
        fn rem_assign(&mut self, x: &Self) {
          if x.size > self.size {
            return;
          }
          let y = Self::zero();
          unsafe {
            gmp::mpn_tdiv_qr(
              y.data(),
              self.data(),
              0,
              self.data(),
              self.size,
              x.data(),
              x.size,
            )
          };
          self.normalize_size();
        }
      }

      impl ops::RemAssign for $t {
        fn rem_assign(&mut self, x: Self) {
          #![allow(clippy::op_ref)]
          *self %= &x;
        }
      }

      impl ops::Div<&Self> for $t {
        type Output = Self;
        fn div(self, x: &Self) -> Self {
          if x.size > self.size {
            return self;
          }
          let (mut y, rem) = (Self::zero(), Self::zero());
          unsafe {
            gmp::mpn_tdiv_qr(
              y.data(),
              rem.data(),
              0,
              self.data(),
              self.size,
              x.data(),
              x.size,
            )
          };
          y.normalize_size();
          y
        }
      }

      impl ops::Div for $t {
        type Output = Self;
        fn div(self, x: Self) -> Self {
          #![allow(clippy::op_ref)]
          self / &x
        }
      }

      impl From<$t> for Integer {
        fn from(x: $t) -> Self {
          Self::from_digits(&x.limbs, Order::Lsf)
        }
      }
    )+
  }
}

u_types!(U256, 4, U512, 8);

impl U512 {
    /// Returns the lower half of this `U512` as a `U256`.
    /// TODO: Make checked?
    pub fn low_u256(self) -> U256 {
        let mut x = unsafe { transmute::<Self, (U256, [u64; 4])>(self) }.0;
        x.normalize_size();
        x
    }
}

impl From<&U256> for U512 {
    fn from(x: &U256) -> Self {
        let mut limbs = [0; 8];
        limbs[..4].copy_from_slice(&x.limbs);
        Self {
            size: x.size,
            limbs,
        }
    }
}

impl From<U256> for U512 {
    fn from(x: U256) -> Self {
        Self::from(&x)
    }
}

// This gets its own implementation for performance.
impl ops::Rem<&U256> for U512 {
    type Output = U256;
    fn rem(self, x: &U256) -> U256 {
        if x.size > self.size {
            return self.low_u256();
        }
        let (y, mut rem) = (Self::zero(), U256::zero());
        unsafe {
            gmp::mpn_tdiv_qr(
                y.data(),
                rem.data(),
                0,
                self.data(),
                self.size,
                x.data(),
                x.size,
            )
        };
        rem.normalize_size();
        rem
    }
}

impl ops::Rem<U256> for U512 {
    type Output = U256;
    fn rem(self, x: U256) -> U256 {
        #![allow(clippy::op_ref)]
        self % &x
    }
}

impl U256 {
    /// Returns (result of removing all `f`s, number of `f`s removed)
    pub fn remove_factor(self, f: Self) -> (Self, u64) {
        // For some reason this needs extra scratch space.
        let mut out = U512::zero();
        let outmpz = out.as_mpz();
        let s = self.as_mpz();
        let f = f.as_mpz();
        let c = unsafe { gmp::mpz_remove(mut_ptr(&outmpz), mut_ptr(&s), mut_ptr(&f)) };
        out.size = i64::from(outmpz.size);
        (out.low_u256(), c)
    }
}

/// It turns out to be faster to provide multiplication as `U256 * U256 -> U512`, because it lets us
/// use `mpn_mul_n` instead of `mpn_mul`.
impl ops::Mul<&Self> for U256 {
    type Output = U512;
    fn mul(self, x: &Self) -> U512 {
        let mut y = U512::zero();
        unsafe { gmp::mpn_mul_n(y.data(), self.data(), x.data(), 4) };
        y.normalize_size();
        y
    }
}

impl ops::Mul for U256 {
    type Output = U512;
    fn mul(self, x: Self) -> U512 {
        #![allow(clippy::op_ref)]
        self * &x
    }
}

#[allow(unused_mut)]
fn mut_ptr<T>(mut t: &T) -> *mut T {
    t as *const T as *mut T
}

pub fn u256<T>(t: T) -> U256
where
    U256: From<T>,
{
    U256::from(t)
}

pub fn u512<T>(t: T) -> U512
where
    U512: From<T>,
{
    U512::from(t)
}

fn i32_to_mpz(i: i32, data: &mut u64) -> mpz_t {
    *data = i.abs() as u64;
    mpz_t {
        size: i.signum(),
        d: mut_ptr(&data),
        alloc: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        assert!(u256(1) + u256(0) == u256(1));
        assert!(u256(1) + u256(2) == u256(3));
        assert!(u256([0, 1, 0, 0]) + u256([0, 1, 0, 0]) == u256([0, 2, 0, 0]));
        assert!(u256([0, 1, 0, 0]) + u256([0, 1, 1, 1]) == u256([0, 2, 1, 1]));
    }

    #[should_panic(expected = "assertion failed: carry == 0")]
    #[test]
    fn test_add_overflow() {
        let _ = u256([0, 0, 0, u64::max_value()]) + u256([0, 0, 0, u64::max_value()]);
    }

    #[test]
    fn test_sub() {
        assert!(u256(1) - u256(0) == u256(1));
        assert!(u256(1) - u256(1) == u256(0));
        assert!(u256([0, 1, 0, 0]) - u256([0, 1, 0, 0]) == u256([0, 0, 0, 0]));
        assert!(u256([0, 1, 0, 1]) - u256([0, 1, 0, 0]) == u256([0, 0, 0, 1]));
    }

    #[should_panic(expected = "assertion failed: borrow == 0")]
    #[test]
    fn test_sub_borrow() {
        let _ = u256([0, 1, 0, 0]) - u256([0, 0, 1, 0]);
    }

    #[test]
    fn test_mul() {
        assert!(u256(0) * u256(3) == u512(0));
        assert!(u256(2) * u256(3) == u512(6));
        assert!(u256([0, 1, 0, 0]) * u256([0, 1, 0, 0]) == u512([0, 0, 1, 0, 0, 0, 0, 0]));
        assert!(u256([0, 2, 0, 0]) * u256([0, 1, 0, 1]) == u512([0, 0, 2, 0, 2, 0, 0, 0]));
    }

    #[test]
    fn test_div() {
        assert!(u256(0) / u256(3) == u256(0));
        assert!(u256(5) / u256(3) == u256(1));
        assert!(u256(6) / u256(3) == u256(2));
        assert!(u256([0, 0, 1, 0]) / u256([0, 1, 0, 0]) == u256([0, 1, 0, 0]));
    }

    #[test]
    fn test_rem() {
        assert!(u256(0) % u256(3) == u256(0));
        assert!(u256(5) % u256(3) == u256(2));
        assert!(u256(6) % u256(3) == u256(0));
        assert!(u256([1, 0, 1, 0]) % u256([0, 1, 0, 0]) == u256(1));
    }

    #[test]
    fn test_rem512() {
        assert!(u512(0) % u256(3) == u256(0));
        assert!(u512(5) % u256(3) == u256(2));
        assert!(u512(6) % u256(3) == u256(0));
        assert!(u512([1, 0, 1, 0, 0, 0, 0, 0]) % u256([0, 1, 0, 0]) == u256(1));
    }
}
