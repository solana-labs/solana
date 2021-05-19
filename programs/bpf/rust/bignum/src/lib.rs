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
const NEG_LONG_DEC_STRING: &str =
    "-1470463693494555670176851280755142329532258274256991544781479988\
                            712408107190720087233560906792937436573943189716784305633216335039\
                            300236370809933808677983409391545753391897467230180786617074456716\
                            591448871466263060696957107957862111484694673874424855359234132302\
                            162208163361387727626078022804936564470716886986414133429438273232\
                            416190048073715996321578752244853524209178212395809614878549824744\
                            227969245726015222693764433413133633359171080169137831743765672068\
                            374040331773668233371864426354886263106537340208256187214278963052\
                            996538599452325797319977413534714912781503130883692806087195354368\
                            8304190675878204079994222";

/// BigNumber construction
fn test_constructors() {
    msg!("BigNumber constructors");
    let base_bn_0 = BigNumber::new();
    assert_eq!(base_bn_0.to_bytes(), vec![0]);
    let default_0 = BigNumber::default();
    assert!(base_bn_0 == default_0);
    let new_bn_0 = BigNumber::from_u32(0);
    assert!(new_bn_0 == default_0);
    let max_bn_u32 = BigNumber::from_u32(u32::MAX);
    assert_eq!(max_bn_u32.to_bytes(), vec![255, 255, 255, 255]);
    let bn_from_dec = BigNumber::from_dec_str(LONG_DEC_STRING);
    assert!(!bn_from_dec.is_negative());
    let bn_from_dec = BigNumber::from_dec_str(NEG_LONG_DEC_STRING);
    assert!(bn_from_dec.is_negative());
}

/// BigNumber simple number and simple maths
fn test_basic_maths() {
    msg!("BigNumber Basic Maths");
    let bn_5 = BigNumber::from_u32(5);
    let bn_258 = BigNumber::from_u32(258);
    let added = bn_5.add(&bn_258);
    assert_eq!(added.to_bytes(), [1, 7]);
    let subed = bn_5.sub(&bn_258);
    assert_eq!(subed.to_bytes(), vec![253]);
    let muled = bn_5.mul(&bn_5);
    assert_eq!(muled.to_bytes(), vec![25]);
    let bn_300 = BigNumber::from_u32(300);
    let bn_10 = BigNumber::from_u32(10);
    let dived = bn_300.div(&bn_10);
    assert_eq!(dived.to_bytes(), vec![30]);
}

/// BigNumber bigger numbers and complex maths
fn test_complex_maths() {
    msg!("BigNumber Complex Maths");
    let bn_arg1 = BigNumber::from_u32(300);
    let sqr_res = bn_arg1.sqr();
    assert_eq!(sqr_res.to_bytes(), vec![1, 95, 144]);
    let bn_arg2 = BigNumber::from_u32(8);
    let bn_arg3 = BigNumber::from_u32(2);
    let exp_res = bn_arg2.exp(&bn_arg3);
    assert_eq!(exp_res.to_bytes(), vec![64]);
    let bn_arg1 = BigNumber::from_u32(300);
    let bn_arg2 = BigNumber::from_u32(11);
    let mod_sqr = bn_arg1.mod_sqr(&bn_arg2);
    assert_eq!(mod_sqr.to_bytes(), vec![9]);
    let bn_arg1 = BigNumber::from_u32(300);
    let bn_arg2 = BigNumber::from_u32(11);
    let bn_arg3 = BigNumber::from_u32(7);
    let mod_exp = bn_arg1.mod_exp(&bn_arg2, &bn_arg3);
    assert_eq!(mod_exp.to_bytes(), vec![6]);
    let mod_mul = bn_arg1.mod_mul(&bn_arg2, &bn_arg3);
    assert_eq!(mod_mul.to_bytes(), vec![3]);
    let bn_arg1 = BigNumber::from_u32(415);
    let bn_arg2 = BigNumber::from_u32(7);
    let mod_inv = bn_arg1.mod_inv(&bn_arg2);
    assert_eq!(mod_inv.to_bytes(), vec![4]);
}

fn test_output_logging() {
    let bn_from_dec = BigNumber::from_dec_str(LONG_DEC_STRING);
    bn_from_dec.log();
    let bn_from_dec = BigNumber::from_dec_str(NEG_LONG_DEC_STRING);
    bn_from_dec.log();
}

#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u64 {
    msg!("bignum");
    test_constructors();
    test_basic_maths();
    test_complex_maths();
    test_output_logging();
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

    #[test]
    fn test_logging() {
        test_output_logging();
    }
}
