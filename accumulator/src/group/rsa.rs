//! RSA (2048) group using GMP integers in the `rug` crate.
use super::{ElemFrom, Group, UnknownOrderGroup};
use crate::util::{int, TypeRep};
use rug::Integer;
use std::str::FromStr;

#[allow(clippy::module_name_repetitions)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// RSA-2048 group implementation. Modulus taken from
/// [here](https://en.wikipedia.org/wiki/RSA_numbers#RSA-2048). **Note**: If you want to use
/// `Rsa2048` outside the context of this crate, be advised that it treats `x` and `-x` as the same
/// element for sound proofs-of-exponentiation. See BBF (page 9).
pub enum Rsa2048 {}

/// RSA-2048 modulus, taken from [Wikipedia](https://en.wikipedia.org/wiki/RSA_numbers#RSA-2048).
const RSA2048_MODULUS_DECIMAL: &str =
  "251959084756578934940271832400483985714292821262040320277771378360436620207075955562640185258807\
  8440691829064124951508218929855914917618450280848912007284499268739280728777673597141834727026189\
  6375014971824691165077613379859095700097330459748808428401797429100642458691817195118746121515172\
  6546322822168699875491824224336372590851418654620435767984233871847744479207399342365848238242811\
  9816381501067481045166037730605620161967625613384414360383390441495263443219011465754445417842402\
  0924616515723350778707749817125772467962926386356373289912154831438167899885040445364023527381951\
  378636564391212010397122822120720357";

lazy_static! {
  pub static ref RSA2048_MODULUS: Integer = Integer::from_str(RSA2048_MODULUS_DECIMAL).unwrap();
  pub static ref HALF_MODULUS: Integer = RSA2048_MODULUS.clone() / 2;
}

#[allow(clippy::module_name_repetitions)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// An RSA 2048 group element, directly wrapping a GMP integer from the `rug` crate.
pub struct Rsa2048Elem(pub Integer);

impl TypeRep for Rsa2048 {
  type Rep = Integer;
  fn rep() -> &'static Self::Rep {
    &RSA2048_MODULUS
  }
}

impl Group for Rsa2048 {
  type Elem = Rsa2048Elem;
  fn op_(modulus: &Integer, a: &Rsa2048Elem, b: &Rsa2048Elem) -> Rsa2048Elem {
    Self::elem(int(&a.0 * &b.0) % modulus)
  }

  fn id_(_: &Integer) -> Rsa2048Elem {
    Self::elem(1)
  }

  fn inv_(modulus: &Integer, x: &Rsa2048Elem) -> Rsa2048Elem {
    Self::elem(x.0.invert_ref(modulus).unwrap())
  }

  fn exp_(modulus: &Integer, x: &Rsa2048Elem, n: &Integer) -> Rsa2048Elem {
    // A side-channel resistant impl is 40% slower; we'll consider it in the future if we need to.
    Self::elem(x.0.pow_mod_ref(n, modulus).unwrap())
  }
}

impl<T> ElemFrom<T> for Rsa2048
where
  Integer: From<T>,
{
  fn elem(t: T) -> Rsa2048Elem {
    let modulus = Self::rep();
    let val = int(t) % modulus;
    if val > *HALF_MODULUS {
      Rsa2048Elem(<(Integer, Integer)>::from((-val).div_rem_euc_ref(&modulus)).1)
    } else {
      Rsa2048Elem(val)
    }
  }
}

impl UnknownOrderGroup for Rsa2048 {
  fn unknown_order_elem_(_: &Integer) -> Rsa2048Elem {
    Self::elem(2)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_init() {
    let _x = &Rsa2048::rep();
  }

  #[test]
  fn test_op() {
    let a = Rsa2048::op(&Rsa2048::elem(2), &Rsa2048::elem(3));
    assert!(a == Rsa2048::elem(6));
    let b = Rsa2048::op(&Rsa2048::elem(-2), &Rsa2048::elem(-3));
    assert!(b == Rsa2048::elem(6));
  }

  /// Tests that `-x` and `x` are treated as the same element.
  #[test]
  fn test_cosets() {
    assert!(Rsa2048::elem(3) == Rsa2048::elem(RSA2048_MODULUS.clone() - 3));
    // TODO: Add a trickier coset test involving `op`.
  }

  #[test]
  fn test_exp() {
    let a = Rsa2048::exp(&Rsa2048::elem(2), &int(3));
    assert!(a == Rsa2048::elem(8));
    let b = Rsa2048::exp(&Rsa2048::elem(2), &int(4096));
    assert!(
      b == Rsa2048::elem(
        Integer::parse(
          "2172073899553954285893691587818692186975191598984015216589930386158248724081087849265975\
          17496727372037176277380476487000099770530440575029170919732871116716934260655466121508332\
          32954361536709981055037121764270784874720971933716065574032615073613728454497477072129686\
          53887333057277396369601863707823088589609031265453680152037285312247125429494632830592984\
          49823194163842041340565518401459166858709515078878951293564147044227487142171138804897039\
          34147612551938082501753055296801829703017260731439871110215618988509545129088484396848644\
          805730347466581515692959313583208325725034506693916571047785061884094866050395109710"
        )
        .unwrap()
      )
    );
    let c = Rsa2048::exp(&Rsa2048::elem(2), &RSA2048_MODULUS);
    dbg!(c);
    let d = Rsa2048::exp(&Rsa2048::elem(2), &(RSA2048_MODULUS.clone() * int(2)));
    dbg!(d);
  }

  #[test]
  fn test_inv() {
    let x = Rsa2048::elem(2);
    let inv = Rsa2048::inv(&x);
    assert!(Rsa2048::op(&x, &inv) == Rsa2048::id());
  }
}
