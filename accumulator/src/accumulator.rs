//! Accumulator library, built on a generic group interface.
use crate::group::UnknownOrderGroup;
use crate::hash::hash_to_prime;
use crate::proof::{Poe, Poke2};
use crate::util::{divide_and_conquer, int, prime_hash_product, shamir_trick};
use rug::Integer;
use std::hash::Hash;
use std::marker::PhantomData;

#[derive(Debug)]
/// The different types of accumulator errors.
pub enum AccError {
    /// Bad witness.
    BadWitness,

    /// Error when updating a witness.
    BadWitnessUpdate,

    /// Division by zero.
    DivisionByZero,

    /// Inexact division where exact division was expected.
    InexactDivision,

    /// Inputs not coprime when they were expected to be coprime.
    InputsNotCoprime,
}

// See https://doc.rust-lang.org/std/marker/struct.PhantomData.html#ownership-and-the-drop-check
// for recommendations regarding phantom types. Note that we disregard the suggestion to use a
// const reference in the phantom type parameter, which causes issues for the `Send` trait.
#[derive(Debug, Eq, Hash, PartialEq)]
/// A cryptographic accumulator. Wraps a single unknown-order group element and phantom data
/// representing the type `T` being hashed-to-prime and accumulated.
pub struct Accumulator<G: UnknownOrderGroup, T> {
    phantom: PhantomData<T>,
    /// the value
    value: G::Elem,
}

// Manual clone impl required because Rust's type inference is not good. See
// https://github.com/rust-lang/rust/issues/26925.
impl<G: UnknownOrderGroup, T: Hash> Clone for Accumulator<G, T> {
    fn clone(&self) -> Self {
        Self {
            phantom: PhantomData,
            value: self.value.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// A witness to one or more values in an accumulator, represented as an accumulator.
pub struct Witness<G: UnknownOrderGroup, T: Hash>(pub Accumulator<G, T>);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// A succinct proof of membership (some element is in some accumulator).
pub struct MembershipProof<G: UnknownOrderGroup, T: Hash> {
    /// The witness for the element in question.
    pub witness: Witness<G, T>,
    proof: Poe<G>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
/// A succinct proof of nonmembership (some element is not in some accumulator).
pub struct NonmembershipProof<G: UnknownOrderGroup, T> {
    phantom: PhantomData<*const T>,
    d: G::Elem,
    v: G::Elem,
    gv_inv: G::Elem,
    poke2_proof: Poke2<G>,
    poe_proof: Poe<G>,
}

impl<G: UnknownOrderGroup, T: Eq + Hash> Accumulator<G, T> {
    /// Returns a new, empty accumulator.
    pub fn empty() -> Self {
        Self {
            phantom: PhantomData,
            value: G::unknown_order_elem(),
        }
    }

    /// Internal add method that also returns the prime hash product of added elements, enabling an
    /// efficient `add_with_proof`.
    fn add_(&self, elems: &[T]) -> (Self, Integer) {
        let x = prime_hash_product(elems);
        let acc_elem = G::exp(&self.value, &x);
        (
            Self {
                phantom: PhantomData,
                value: acc_elem,
            },
            x,
        )
    }

    // The conciseness of `accumulator.add()` and low probability of confusion with implementations of
    // the `Add` trait probably justify this...
    #[allow(clippy::should_implement_trait)]
    /// Adds `elems` to the accumulator. This cannot check whether the elements have already been
    /// added, so is up to clients to ensure uniqueness.
    ///
    /// Uses a move instead of a `&self` reference to prevent accidental use of the old accumulator.
    pub fn add(self, elems: &[T]) -> Self {
        self.add_(elems).0
    }

    /// A specialized version of `add` that also returns a batch membership proof for added elements.
    pub fn add_with_proof(self, elems: &[T]) -> (Self, MembershipProof<G, T>) {
        let (acc, x) = self.add_(elems);
        let proof = Poe::<G>::prove(&self.value, &x, &acc.value);
        (
            acc,
            MembershipProof {
                witness: Witness(self),
                proof,
            },
        )
    }

    /// Internal delete method that also returns the prime hash product of deleted elements, enabling
    /// an efficient `delete_with_proof`.
    ///
    /// Uses a divide-and-conquer approach to running the ShamirTrick, which keeps the average input
    /// smaller: For `[a, b, c, d]` do `S(S(a, b), S(c, d))` instead of `S(S(S(a, b), c), d)`.
    fn delete_(self, elem_witnesses: &[(T, Witness<G, T>)]) -> Result<(Self, Integer), AccError> {
        let prime_witnesses = elem_witnesses
            .iter()
            .map(|(elem, witness)| (hash_to_prime(elem), witness.0.value.clone()))
            .collect::<Vec<_>>();

        for (p, witness_elem) in &prime_witnesses {
            if G::exp(&witness_elem, &p) != self.value {
                return Err(AccError::BadWitness);
            }
        }

        let (prime_product, acc_elem) = divide_and_conquer(
            |(p1, v1), (p2, v2)| Ok((int(p1 * p2), shamir_trick::<G>(&v1, &v2, p1, p2).unwrap())),
            (int(1), self.value),
            &prime_witnesses[..],
        )?;

        Ok((
            Self {
                phantom: PhantomData,
                value: acc_elem.clone(),
            },
            prime_product,
        ))
    }

    /// Removes the elements in `elem_witnesses` from the accumulator.
    ///
    /// # Arguments
    ///
    /// * `elem_witnesses` - Tuples consisting of (element to delete, element's witness).
    ///
    /// Uses a move instead of a `&self` reference to prevent accidental use of the old accumulator.
    pub fn delete(self, elem_witnesses: &[(T, Witness<G, T>)]) -> Result<Self, AccError> {
        Ok(self.delete_(elem_witnesses)?.0)
    }

    /// A specialized version of `delete` that also returns a batch membership proof for deleted
    /// elements.
    pub fn delete_with_proof(
        self,
        elem_witnesses: &[(T, Witness<G, T>)],
    ) -> Result<(Self, MembershipProof<G, T>), AccError> {
        let (acc, prime_product) = self.clone().delete_(elem_witnesses)?;
        let proof = Poe::<G>::prove(&acc.value, &prime_product, &self.value);
        Ok((
            acc.clone(),
            MembershipProof {
                witness: Witness(acc),
                proof,
            },
        ))
    }

    /// Computes the batch membership proof for the elements in `elem_witnesses` w.r.t this
    /// accumulator.
    ///
    /// # Arguments
    ///
    /// * `elem_witnesses` - Tuples consisting of (element to prove, element's witness).
    pub fn prove_membership(
        &self,
        elem_witnesses: &[(T, Witness<G, T>)],
    ) -> Result<MembershipProof<G, T>, AccError> {
        let witness_accum = self.clone().delete(elem_witnesses)?;
        let prod = elem_witnesses
            .iter()
            .map(|(t, _)| hash_to_prime(t))
            .product();
        let proof = Poe::<G>::prove(&witness_accum.value, &prod, &self.value);
        Ok(MembershipProof {
            witness: Witness(witness_accum),
            proof,
        })
    }

    /// Verifies a membership proof against the current accumulator and an element `t` whose
    /// inclusion is being proven.
    pub fn verify_membership(
        &self,
        t: &T,
        MembershipProof { witness, proof }: &MembershipProof<G, T>,
    ) -> bool {
        let exp = hash_to_prime(t);
        Poe::verify(&witness.0.value, &exp, &self.value, proof)
    }

    /// Batch version of `verify_membership` for multiple `elems`.
    pub fn verify_membership_batch(
        &self,
        elems: &[T],
        MembershipProof { witness, proof }: &MembershipProof<G, T>,
    ) -> bool {
        let exp = prime_hash_product(elems);
        Poe::verify(&witness.0.value, &exp, &self.value, proof)
    }

    /// Updates a `witness` for `tracked_elems` w.r.t the current accumulator, adding the elements in
    /// `untracked_additions` to the tracked set and removing the elements in `untracked_deletions`
    /// from the tracked set.
    ///
    /// See Section 4.2 of LLX for implementation details.
    pub fn update_membership_witness(
        &self,
        witness: Witness<G, T>,
        tracked_elems: &[T],
        untracked_additions: &[T],
        untracked_deletions: &[T],
    ) -> Result<Witness<G, T>, AccError> {
        let x = prime_hash_product(tracked_elems);
        let x_hat = prime_hash_product(untracked_deletions);

        for elem in tracked_elems {
            if untracked_additions.contains(elem) || untracked_deletions.contains(elem) {
                return Err(AccError::BadWitnessUpdate);
            }
        }

        let (gcd, a, b) = <(Integer, Integer, Integer)>::from(x.gcd_cofactors_ref(&x_hat));
        assert!(gcd == int(1));

        let w = witness.0.add(untracked_additions);
        let w_to_b = G::exp(&w.value, &b);
        let acc_new_to_a = G::exp(&self.value, &a);
        Ok(Witness(Self {
            phantom: PhantomData,
            value: G::op(&w_to_b, &acc_new_to_a),
        }))
    }

    /// Computes the batch non-membership proof for the elements in `elems` w.r.t this accumulator
    /// and its `acc_set`.
    ///
    /// # Arguments
    ///
    /// * `acc_set` - The set of elements committed to by this accumulator.
    /// * `elems` - The set of elements you want to prove are not in `acc_set`.
    pub fn prove_nonmembership(
        &self,
        acc_set: &[T],
        elems: &[T],
    ) -> Result<NonmembershipProof<G, T>, AccError> {
        let x: Integer = elems.iter().map(hash_to_prime).product();
        let s = acc_set.iter().map(hash_to_prime).product();
        let (gcd, a, b) = <(Integer, Integer, Integer)>::from(x.gcd_cofactors_ref(&s));

        if gcd != int(1) {
            return Err(AccError::InputsNotCoprime);
        }

        let g = G::unknown_order_elem();
        let d = G::exp(&g, &a);
        let v = G::exp(&self.value, &b);
        let gv_inv = G::op(&g, &G::inv(&v));

        let poke2_proof = Poke2::prove(&self.value, &b, &v);
        let poe_proof = Poe::prove(&d, &x, &gv_inv);
        Ok(NonmembershipProof {
            phantom: PhantomData,
            d,
            v,
            gv_inv,
            poke2_proof,
            poe_proof,
        })
    }

    /// Verifies a non-membership proof against the current accumulator and elements `elems` whose
    /// non-inclusion is being proven.
    pub fn verify_nonmembership(
        &self,
        elems: &[T],
        NonmembershipProof {
            d,
            v,
            gv_inv,
            poke2_proof,
            poe_proof,
            ..
        }: &NonmembershipProof<G, T>,
    ) -> bool {
        let x = elems.iter().map(hash_to_prime).product();
        Poke2::verify(&self.value, v, poke2_proof) && Poe::verify(d, &x, gv_inv, poe_proof)
    }
}

impl<G: UnknownOrderGroup, T: Eq + Hash> From<&[T]> for Accumulator<G, T> {
    fn from(ts: &[T]) -> Self {
        Self::empty().add(ts)
    }
}

impl<G: UnknownOrderGroup, T: Clone + Hash> Witness<G, T> {
    /// Given a witness for `witness_set`, returns a witness for `witness_subset`.
    ///
    /// The `witness_subset` must be a subset of the `witness_set`.
    pub fn compute_subset_witness(
        self,
        witness_set: &[T],
        witness_subset: &[T],
    ) -> Result<Self, AccError>
    where
        T: PartialEq,
    {
        for witness in witness_subset {
            if !witness_set.contains(witness) {
                return Err(AccError::BadWitness);
            }
        }

        let numerator = prime_hash_product(witness_set);
        let denominator = prime_hash_product(witness_subset);
        let (quotient, remainder) = numerator.div_rem(denominator);

        if remainder != int(0) {
            return Err(AccError::InexactDivision);
        }

        Ok(Self(Accumulator {
            phantom: PhantomData,
            value: G::exp(&self.0.value, &quotient),
        }))
    }

    /// Given a witness for many `elems`, computes a sub-witness for each individual element in
    /// O(N log N) time.
    pub fn compute_individual_witnesses(&self, elems: &[T]) -> Vec<(T, Self)> {
        let hashes = elems.iter().map(hash_to_prime).collect::<Vec<_>>();
        elems
            .iter()
            .zip(self.root_factor(&hashes).iter())
            .map(|(x, y)| (x.clone(), y.clone()))
            .collect()
    }

    #[allow(non_snake_case)]
    fn root_factor(&self, elems: &[Integer]) -> Vec<Self> {
        if elems.len() == 1 {
            return vec![self.clone()];
        }
        let half_n = elems.len() / 2;
        let g_l = elems[..half_n].iter().fold(self.clone(), |sum, x| {
            Self(Accumulator {
                phantom: PhantomData,
                value: G::exp(&sum.0.value, x),
            })
        });
        let g_r = elems[half_n..].iter().fold(self.clone(), |sum, x| {
            Self(Accumulator {
                phantom: PhantomData,
                value: G::exp(&sum.0.value, x),
            })
        });
        let mut L = g_r.root_factor(&Vec::from(&elems[..half_n]));
        let mut R = g_l.root_factor(&Vec::from(&elems[half_n..]));
        L.append(&mut R);
        L
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::Rsa2048;

    fn new_acc<G: UnknownOrderGroup, T: Hash + Eq>(data: &[T]) -> Accumulator<G, T> {
        Accumulator::<G, T>::empty().add(data)
    }

    macro_rules! test_all_groups {
    ($test_func:ident, $func_name_rsa:ident, $func_name_class:ident, $($attr:meta)*) => {
      #[test]
      $(
        #[$attr]
      )*
      fn $func_name_rsa() {
        $test_func::<Rsa2048>();
      }
    };
  }

    test_all_groups!(test_add, test_add_rsa2048, test_add_class,);
    fn test_add<G: UnknownOrderGroup>() {
        let acc = new_acc::<G, &'static str>(&["a", "b"]);
        let new_elems = ["c", "d"];
        let (acc_new, proof) = acc.add_with_proof(&new_elems);
        let acc_expected = G::exp(
            &G::unknown_order_elem(),
            &prime_hash_product(&["a", "b", "c", "d"]),
        );
        assert!(acc_new.value == acc_expected);
        assert!(acc_new.verify_membership_batch(&new_elems, &proof));
    }

    test_all_groups!(test_delete, test_delete_rsa2048, test_delete_class,);
    fn test_delete<G: UnknownOrderGroup>() {
        let acc_0 = new_acc::<G, &'static str>(&["a", "b"]);
        let (acc_1, c_proof) = acc_0.clone().add_with_proof(&["c"]);
        let (acc_2, proof) = acc_1
            .clone()
            .delete_with_proof(&[("c", c_proof.witness)])
            .expect("valid delete expected");
        assert!(acc_2 == acc_0);
        assert!(acc_1.verify_membership(&"c", &proof));
    }

    test_all_groups!(
        test_delete_empty,
        test_delete_empty_rsa2048,
        test_delete_empty_class,
    );
    fn test_delete_empty<G: UnknownOrderGroup>() {
        let acc = new_acc::<G, &'static str>(&["a", "b"]);
        let (acc_new, proof) = acc
            .clone()
            .delete_with_proof(&[])
            .expect("valid delete expected");
        assert!(acc_new == acc);
        assert!(acc.verify_membership_batch(&[], &proof));
    }

    test_all_groups!(
        test_delete_bad_witness,
        test_delete_bad_witness_rsa2048,
        test_delete_bad_witness_class,
        should_panic(expected = "BadWitness")
    );
    fn test_delete_bad_witness<G: UnknownOrderGroup>() {
        let acc = Accumulator::<G, &'static str>::empty();
        let a_witness = Witness(new_acc::<G, &'static str>(&["b", "c"]));
        let b_witness = Witness(new_acc::<G, &'static str>(&["a", "c"]));
        acc.delete(&[("a", a_witness), ("b", b_witness)]).unwrap();
    }

    test_all_groups!(
        test_update_membership_witness,
        test_update_membership_witness_rsa2048,
        test_update_membership_witness_class,
    );
    fn test_update_membership_witness<G: UnknownOrderGroup>() {
        let acc = new_acc::<G, &'static str>(&["a", "b", "c"]);
        let witness = Witness(new_acc::<G, &'static str>(&["c", "d"]));
        let witness_new = acc
            .update_membership_witness(witness, &["a"], &["b"], &["d"])
            .unwrap();
        assert!(witness_new.0.add(&["a"]) == acc);
    }

    test_all_groups!(
        test_update_membership_witness_failure,
        test_update_membership_witness_failure_rsa2048,
        test_update_membership_witness_failure_class,
        should_panic(expected = "BadWitnessUpdate")
    );
    fn test_update_membership_witness_failure<G: UnknownOrderGroup>() {
        let acc = new_acc::<G, &'static str>(&["a", "b", "c"]);
        let witness = Witness(new_acc::<G, &'static str>(&["c", "d"]));
        acc.update_membership_witness(witness, &["a"], &["b"], &["a"])
            .unwrap();
    }

    test_all_groups!(
        test_prove_nonmembership,
        test_prove_nonmembership_rsa2048,
        test_prove_nonmembership_class,
    );
    fn test_prove_nonmembership<G: UnknownOrderGroup>() {
        let acc_set = ["a", "b"];
        let acc = new_acc::<G, &'static str>(&acc_set);
        let non_members = ["c", "d"];
        let proof = acc
            .prove_nonmembership(&acc_set, &non_members)
            .expect("valid proof expected");
        assert!(acc.verify_nonmembership(&non_members, &proof));
    }

    test_all_groups!(
        test_compute_sub_witness,
        test_compute_sub_witness_rsa2048,
        test_compute_sub_witness_class,
    );
    fn test_compute_sub_witness<G: UnknownOrderGroup>() {
        let empty_witness = Witness(Accumulator::<G, &'static str>::empty());
        let sub_witness = empty_witness
            .compute_subset_witness(&["a", "b"], &["a"])
            .unwrap();
        let exp_quotient_expected = Witness(new_acc::<G, &'static str>(&["b"]));
        assert!(sub_witness == exp_quotient_expected);
    }

    test_all_groups!(
        test_compute_sub_witness_failure,
        test_compute_sub_witness_failure_rsa2048,
        test_compute_sub_witness_failure_class,
        should_panic(expected = "BadWitness")
    );
    fn test_compute_sub_witness_failure<G: UnknownOrderGroup>() {
        let empty_witness = Witness(Accumulator::<G, &'static str>::empty());
        empty_witness
            .compute_subset_witness(&["a", "b"], &["c"])
            .unwrap();
    }

    fn test_compute_individual_witnesses<G: UnknownOrderGroup>() {
        let acc = new_acc::<G, &'static str>(&["a", "b", "c"]);
        let witness_multiple = Witness(new_acc::<G, &'static str>(&["a"]));
        let witnesses = witness_multiple.compute_individual_witnesses(&["b", "c"]);
        for (elem, witness) in witnesses {
            assert_eq!(acc.value, G::exp(&witness.0.value, &hash_to_prime(elem)));
        }
    }

    #[test]
    fn test_compute_individual_witnesses_rsa2048() {
        // Class version takes too long for a unit test.
        test_compute_individual_witnesses::<Rsa2048>();
    }
}
