//! @brief BigNumber Syscall test

extern crate solana_program;
use solana_program::{bignum::BigNumber, custom_panic_default, msg};

const LONG_DEC_STRING: &str = "1470463693494555670176851280755142329532258274256991544781479988\
                            712408107190720087233560906792937436573943189716784305633216335039\
                            300236370809933808677983409391545753391897467230180786617074456716\
                            591448871466263060696957107957862111484694673874424855359234132302\
                            162208163361387727626078022804936564470716886986414133429438273232\
                            416190048073715996321578752244853524209178212395809614878549824744\
                            227969245726015222693764433413133633359171080169137831743765672068\
                            374040331773668233371864426354886263106537340208256187214278963052\
                            996538599452325797319977413534714912781503130883692806087195354368\
                            8304190675878204079994222";

/// Compares the array of numbers return from BigNumbers
fn compare_bignum_equal(lhs: &BigNumber, rhs: &BigNumber) -> bool {
    lhs.to_bytes() == rhs.to_bytes()
}

/// BigNumber construction
fn test_constructors() {
    msg!("BigNumber constructors");
    let base_bn_0 = BigNumber::new();
    base_bn_0.log();
    let new_bn_0 = BigNumber::from_u32(0);
    new_bn_0.log();
    let max_bn_u32 = BigNumber::from_u32(u32::MAX);
    max_bn_u32.log();
    let new_bn_from_long_str = BigNumber::from_bytes(LONG_DEC_STRING.as_bytes());
    new_bn_from_long_str.log();
    let empty_bn = BigNumber::from_bytes(&[0u8]);
    empty_bn.log();
}

/// BigNumber simple number and simple maths
fn test_basic_maths() {
    msg!("BigNumber Basic Maths");
    let lhs = BigNumber::from_u32(3);
    let rhs = BigNumber::from_u32(4);
    let add_res = lhs.add(&rhs);
    assert!(compare_bignum_equal(&add_res, &BigNumber::from_u32(7)));
    let sub_res = rhs.sub(&lhs);
    assert!(compare_bignum_equal(&sub_res, &BigNumber::from_u32(1)));
    let lhs = BigNumber::from_u32(20);
    let rhs = BigNumber::from_u32(10);
    let div_res = lhs.div(&rhs);
    assert!(compare_bignum_equal(&div_res, &BigNumber::from_u32(2)));
    let mul_res = rhs.mul(&lhs);
    assert!(compare_bignum_equal(&mul_res, &BigNumber::from_u32(200)));
}

/// BigNumber bigger numbers and complex maths
fn test_complex_maths() {
    msg!("BigNumber Complex Maths");
    let base_2 = BigNumber::from_u32(3);
    let base_3 = BigNumber::from_u32(3);
    let exp_base_3 = base_3.clone();
    let modulus_7 = BigNumber::from_u32(7);
    assert!(compare_bignum_equal(
        &base_3.mod_mul(&exp_base_3, &modulus_7),
        &BigNumber::from_u32(2)
    ));
    let base_15 = BigNumber::from_u32(15);
    assert!(compare_bignum_equal(
        &base_15.mod_sqr(&modulus_7),
        &BigNumber::from_u32(1)
    ));
    assert_eq!(base_15.sqr().to_bytes(), [225]);
    assert_eq!(base_15.exp(&base_2).to_bytes(), [13, 47]);
    assert_eq!(base_3.mod_inv(&modulus_7).to_bytes(), [5]);
}

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u64 {
    msg!("bignum");
    test_constructors();
    test_basic_maths();
    test_complex_maths();
    0u64
}

custom_panic_default!();

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_basic_constructors_pass() {
        test_constructors();
    }
    #[test]
    fn test_simple_maths_pass() {
        test_basic_maths();
    }
    #[test]
    fn test_complex_maths_pass() {
        test_complex_maths();
    }
}
