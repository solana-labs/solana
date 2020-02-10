//! Primality testing for U256 inputs. Use `is_prob_prime` unless you have a specific reason to use
//! a lower-level test.
use crate::uint::{u256, u512, U256};

mod constants;
use constants::{D_VALUES, SMALL_PRIMES};

/// Implements the Baillie-PSW probabilistic primality test, which is known to be deterministic over
/// all integers up to 64 bits.
///
/// Outperforms naked Miller-Rabin (i.e. iterated Fermat tests of random base) at wide `n` since
/// Fermat and Lucas pseudoprimes have been shown to be anticorrelated. Steps of BPSW are as
/// follows:
///
/// 1. Accept small primes and reject multiples of them.
/// 2. Do a single iteration of Miller-Rabin (in particular, a base-2 Fermat test).
/// 3. Do a strong probabilistic Lucas test (squares filtered during test initialization).
pub fn is_prob_prime(n: &U256) -> bool {
    for &p in SMALL_PRIMES.iter() {
        if n.is_divisible_u(p) {
            return *n == p;
        }
    }
    passes_miller_rabin_base_2(&n) && passes_lucas(&n)
}

/// A single iteration of the Miller-Rabin test (base-2 Fermat test).
pub fn passes_miller_rabin_base_2(n: &U256) -> bool {
    let (d, r) = (n - 1).remove_factor(u256(2));
    let mut x = u256(2).pow_mod(d, n);
    if x == 1 || x == n - 1 {
        return true;
    }
    for _ in 1..r {
        x = x * x % n;
        if x == 1 {
            return false;
        }
        if x == n - 1 {
            return true;
        }
    }
    false
}

/// Strong Lucas probable prime test (NOT the more common Lucas primality test which requires
/// factorization of `n-1`).
///
/// Selects parameters `d`, `p`, `q` according to Selfridge's method.
///
/// If `n` passes, it is either prime or a "strong" Lucas pseudoprime. (The precise meaning of
/// "strong" is not fixed in the literature.) Procedure can be further strengthened by implementing
/// more tests in Section 6 of [Baillie and Wagstaff 1980], but for now this is TODO.
///
/// See also: [Lucas pseudoprime](https://en.wikipedia.org/wiki/Lucas_pseudoprime) on Wikipedia.
pub fn passes_lucas(n: &U256) -> bool {
    let d_ = choose_d(n);
    if d_.is_err() {
        return false;
    }
    let d = d_.unwrap();
    let q = (1 - d) / 4;

    let (u_delta, v_delta, q_delta_over_2) =
        compute_lucas_sequences(*n + 1, n, u256(1), u256(1), q, d);
    // `u_delta % n != 0` proves n composite.
    u_delta == 0
  // Additional check which is not strictly part of Lucas test but nonetheless filters some
    // composite n for free. See section "Checking additional congruence conditions" on Wikipedia.
    && v_delta.is_congruent(2 * q, n)
    // Congruence check which holds for prime n by Euler's criterion.
    && q_delta_over_2.is_congruent(q * U256::jacobi(q, n), n)
}

#[derive(Debug)]
struct IsPerfectSquare();

/// Finds and returns first `D` in `[5, -7, 9, ..., 5 + 2 * max_iter]` for which Jacobi symbol
/// `(D/n) = -1`, or `Err` if no such `D` exists. In the case that `n` is square, there is no such
/// `D` even with `max_iter` infinite. Hence if you are not entirely sure that `n` is nonsquare,
/// you should pass a low value to `max_iter` to avoid wasting too much time. Note that the average
/// number of iterations required for nonsquare `n` is 1.8, and empirically we find it is extremely
/// rare that `|d| > 13`.
///
/// We experimented with postponing the `is_perfect_square` check until after some number of
/// iterations but ultimately found no performance gain. It is likely that most perfect squares
/// are caught by the Miller-Rabin test.
fn choose_d(n: &U256) -> Result<i32, IsPerfectSquare> {
    if n.is_perfect_square() {
        return Err(IsPerfectSquare());
    }
    for &d in D_VALUES.iter() {
        if U256::jacobi(d, n) == -1 {
            return Ok(d);
        }
    }
    panic!("n is not square but we still couldn't find a d value!")
}

/// Computes the Lucas sequences `{u_i(p, q)}` and `{v_i(p, q)}` up to a specified index `k_target`
/// in O(log(`k_target`)) time by recursively calculating only the `(2i)`th and `(2i+1)`th elements
/// in an order determined by the binary expansion of `k`. Also returns `q^{k/2} (mod n)`, which is
/// used in a stage of the strong Lucas test. In the Lucas case we specify that `d = p^2 - 4q` and
/// set `k_target = delta = n - (d/n) = n + 1`.
///
/// Note that `p` does not show up in the code because it is set to 1.
#[allow(clippy::cast_sign_loss)]
fn compute_lucas_sequences(
    k_target: U256,
    n: &U256,
    mut u: U256,
    mut v: U256,
    q0: i32,
    d: i32,
) -> (U256, U256, U256) {
    // Mod an `i32` into the `[0, n)` range.
    let i_mod_n = |x: i32| {
        if x < 0 {
            *n - (u256(x.abs() as u64) % n)
        } else {
            u256(x as u64) % n
        }
    };
    let q0 = i_mod_n(q0);
    let d = i_mod_n(d);
    let mut q = q0;
    let mut q_k_over_2 = q0;

    // Finds `t` in `Z_n` with `2t = x (mod n)`.
    // Assumes `x` in `[0, n)`.
    let half = |x: U256| {
        if x.is_odd() {
            (x >> 1) + (*n >> 1) + 1
        } else {
            x >> 1
        }
    };
    let sub_mod_n = |a, b| {
        if a > b {
            (a - b) % n
        } else {
            *n - (b - a) % n
        }
    };
    // Write binary expansion of `k` as [x_1, ..., x_l], e.g. [1, 0, 1, 1] for 11. `x_1` is always
    // 1. For `i = 2, 3, ..., l`, do the following: if `x_i = 0`, then update `u_k` and `v_k` to
    // `u_{2k}` and `v_{2k}`, respectively. Else if `x_i = 1`, update to `u_{2k+1}` and `v_{2k+1}`.
    // At the end of the loop we will have computed `u_k` and `v_k`, with `k` as given, in
    // `log(delta)` time.
    let mut k_target_bits = [0; 257];
    let len = k_target.write_binary(&mut k_target_bits);
    for &bit in k_target_bits[..len].iter().skip(1) {
        // Compute `(u, v)_{2k}` from `(u, v)_k` according to the following:
        // u_2k = u_k * v_k (mod n)
        // v_2k = v_k^2 - 2*q^k (mod n)
        u = u * v % n;
        v = sub_mod_n(v * v, u512(q) << 1);
        // Continuously maintain `q_k = q^k (mod n)` and `q_k_over_2 = q^{k/2} (mod n)`.
        q_k_over_2 = q;
        q = q * q % n;
        if bit == 1 {
            // Compute `(u, v)_{2k+1}` from `(u, v)_{2k}` according to the following:
            // u_{2k+1} = 1/2 * (p*u_{2k} + v_{2k}) (mod n)
            // v_{2k+1} = 1/2 * (d*u_{2k} + p*v_{2k}) (mod n)
            let u_old = u;
            u = half((u512(u) + u512(v)) % n);
            v = half((d * u_old + u512(v)) % n);
            q = q * q0 % n;
        }
    }
    // These are all `mod n` so the `low_u256` is lossless. We could make it checked...
    (u, v, q_k_over_2)
}

#[cfg(test)]
mod tests {
    use self::constants::*;
    use super::*;
    #[test]
    fn test_miller_rabin() {
        assert!(passes_miller_rabin_base_2(&u256(13)));
        assert!(!passes_miller_rabin_base_2(&u256(65)));
        for &p in LARGE_PRIMES.iter() {
            assert!(passes_miller_rabin_base_2(&u256(p)));
            assert!(!passes_miller_rabin_base_2(
                &(u256(p) * u256(106_957)).low_u256()
            ));
        }
        for &n in STRONG_BASE_2_PSEUDOPRIMES.iter() {
            assert!(passes_miller_rabin_base_2(&u256(n)));
        }
    }

    #[test]
    fn test_lucas() {
        assert!(passes_lucas(&u256(5)));
        // Should fail on `p = 2`.
        for &sp in SMALL_PRIMES[1..].iter() {
            assert!(passes_lucas(&u256(sp)));
            assert!(!passes_lucas(&(u256(sp) * u256(2047)).low_u256()));
        }
        for &mp in MED_PRIMES.iter() {
            assert!(passes_lucas(&u256(mp)));
            assert!(!passes_lucas(&(u256(mp) * u256(5)).low_u256()));
        }
        for &lp in LARGE_PRIMES.iter() {
            assert!(passes_lucas(&u256(lp)));
            assert!(!passes_lucas(&(u256(lp) * u256(7)).low_u256()));
        }
    }

    #[test]
    fn test_is_prob_prime() {
        // Sanity checks.
        assert!(is_prob_prime(&u256(2)));
        assert!(is_prob_prime(&u256(5)));
        assert!(is_prob_prime(&u256(7)));
        assert!(is_prob_prime(&u256(241)));
        assert!(is_prob_prime(&u256(7919)));
        assert!(is_prob_prime(&u256(48131)));
        assert!(is_prob_prime(&u256(76463)));
        assert!(is_prob_prime(&u256(115_547)));

        // Medium primes.
        for &p in MED_PRIMES.iter() {
            assert!(is_prob_prime(&u256(p)));
        }

        // Large primes.
        for &p in LARGE_PRIMES.iter() {
            assert!(is_prob_prime(&u256(p)));
        }

        // Large, difficult-to-factor composites.
        for &p in LARGE_PRIMES.iter() {
            for &q in LARGE_PRIMES.iter() {
                assert!(!is_prob_prime(&(u256(p) * u256(q)).low_u256()));
            }
        }
    }
}
