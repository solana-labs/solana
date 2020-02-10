//! Non-Interactive Proofs of Knowledge of Co-prime Roots (NI-PoKCR). See BBF (page 11) for details.
use crate::group::{multi_exp, Group};
use rug::Integer;

#[allow(non_snake_case)]
#[derive(PartialEq, Eq, Hash, Clone, Debug)]
/// Struct for NI-PoKCR.
pub struct Pokcr<G: Group> {
    w: G::Elem,
}

impl<G: Group> Pokcr<G> {
    /// Generates an NI-PoKCR proof.
    pub fn prove(witnesses: &[G::Elem]) -> Self {
        Self {
            w: witnesses.iter().fold(G::id(), |a, b| G::op(&a, b)),
        }
    }

    /// Verifies an NI-PoKCR proof.
    pub fn verify(alphas: &[G::Elem], x: &[Integer], proof: &Self) -> bool {
        let y = multi_exp::<G>(alphas, x);
        let lhs = G::exp(&proof.w, &x.iter().product());
        lhs == y
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::{ElemFrom, Rsa2048};
    use crate::util::int;

    #[test]
    fn test_pokcr() {
        let witnesses = [Rsa2048::elem(2), Rsa2048::elem(3)];
        let x = [int(2), int(2)];
        let alphas = [Rsa2048::elem(4), Rsa2048::elem(9)];
        let proof = Pokcr::<Rsa2048>::prove(&witnesses);
        assert!(proof.w == Rsa2048::elem(6));
        assert!(Pokcr::verify(&alphas, &x, &proof));
    }
}
