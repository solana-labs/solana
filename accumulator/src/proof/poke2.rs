//! Non-Interactive Proofs of Knowledge of Exponent (NI-PoKE2). See BBF (pages 10 and 42) for
//! details.
use crate::group::UnknownOrderGroup;
use crate::hash::{blake2b, hash_to_prime};
use rug::Integer;

#[allow(non_snake_case)]
#[derive(PartialEq, Eq, Hash, Clone, Debug)]
/// Struct for NI-PoKE2.
pub struct Poke2<G: UnknownOrderGroup> {
    z: G::Elem,
    Q: G::Elem,
    r: Integer,
}

impl<G: UnknownOrderGroup> Poke2<G> {
    /// Computes a proof that you know `exp` s.t. `base ^ exp = result`.
    pub fn prove(base: &G::Elem, exp: &Integer, result: &G::Elem) -> Self {
        let g = G::unknown_order_elem();
        let z = G::exp(&g, exp);
        let l = hash_to_prime(&(base, result, &z));
        let alpha = blake2b(&(base, result, &z, &l));
        let (q, r) = <(Integer, Integer)>::from(exp.div_rem_euc_ref(&l));
        #[allow(non_snake_case)]
        let Q = G::exp(&G::op(&base, &G::exp(&g, &alpha)), &q);
        Self { z, Q, r }
    }

    /// Verifies that the prover knows `exp` s.t. `base ^ exp = result`.
    #[allow(non_snake_case)]
    pub fn verify(base: &G::Elem, result: &G::Elem, Self { z, Q, r }: &Self) -> bool {
        let g = G::unknown_order_elem();
        let l = hash_to_prime(&(base, result, &z));
        let alpha = blake2b(&(base, result, &z, &l));
        let lhs = G::op(
            &G::exp(Q, &l),
            &G::exp(&G::op(&base, &G::exp(&g, &alpha)), &r),
        );
        let rhs = G::op(result, &G::exp(&z, &alpha));
        lhs == rhs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::{ElemFrom, Group, Rsa2048};
    use crate::util::int;

    #[test]
    fn test_poke2() {
        // 2^20 = 1048576
        let base = Rsa2048::unknown_order_elem();
        let exp = int(20);
        let result = Rsa2048::elem(1_048_576);
        let proof = Poke2::<Rsa2048>::prove(&base, &exp, &result);
        assert!(Poke2::verify(&base, &result, &proof));
        // Must compare entire structs since elements `z`, `Q`, and `r` are private.
        assert!(
            proof
                == Poke2 {
                    z: Rsa2048::elem(1_048_576),
                    Q: Rsa2048::elem(1),
                    r: int(20)
                }
        );

        // 2^35 = 34359738368
        let exp_2 = int(35);
        let result_2 = Rsa2048::elem(34_359_738_368u64);
        let proof_2 = Poke2::<Rsa2048>::prove(&base, &exp_2, &result_2);
        assert!(Poke2::verify(&base, &result_2, &proof_2));
        // Cannot verify wrong base/exp/result triple with wrong pair.
        assert!(!Poke2::verify(&base, &result_2, &proof));
        assert!(
            proof_2
                == Poke2 {
                    z: Rsa2048::elem(34_359_738_368u64),
                    Q: Rsa2048::elem(1),
                    r: int(35)
                }
        );
    }

    #[test]
    fn test_poke2_negative() {
        let base = Rsa2048::elem(2);
        let exp = int(-5);
        let result = Rsa2048::exp(&base, &exp);
        let proof = Poke2::<Rsa2048>::prove(&base, &exp, &result);
        assert!(Poke2::verify(&base, &result, &proof));
    }
}
