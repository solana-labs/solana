//! Big_mod_exp Syscall tests

extern crate solana_program;
use solana_program::{
    custom_panic_default, msg,
};

fn big_mod_exp_test(){
    use num_bigint::BigUint;

    #[serde(rename_all = "PascalCase")]
    #[derive(serde::Deserialize)]
    struct TestCase{
        base: String,
        exponent: String,
        modulus: String,
        expected: String,
    }

    let test_data = r#"[
        {
            "Base":     "1111111111111111111111111111111111111111111111111111111111111111",
            "Exponent": "1111111111111111111111111111111111111111111111111111111111111111",
            "Modulus":  "111111111111111111111111111111111111111111111111111111111111110A",
            "Expected": "0A7074864588D6847F33A168209E516F60005A0CEC3F33AAF70E8002FE964BCD"
        },
        {
            "Base":     "2222222222222222222222222222222222222222222222222222222222222222",
            "Exponent": "2222222222222222222222222222222222222222222222222222222222222222",
            "Modulus":  "1111111111111111111111111111111111111111111111111111111111111111",
            "Expected": "00"
        },
        {
            "Base":     "3333333333333333333333333333333333333333333333333333333333333333",
            "Exponent": "3333333333333333333333333333333333333333333333333333333333333333",
            "Modulus":  "2222222222222222222222222222222222222222222222222222222222222222",
            "Expected": "1111111111111111111111111111111111111111111111111111111111111111"
        },
        {
            "Base":     "9874231472317432847923174392874918237439287492374932871937289719",
            "Exponent": "0948403985401232889438579475812347232099080051356165126166266222",
            "Modulus":  "25532321a214321423124212222224222b242222222222222222222222222444",
            "Expected": "220ECE1C42624E98AEE7EB86578B2FE5C4855DFFACCB43CCBB708A3AB37F184D"
        },
        {
            "Base":     "3494396663463663636363662632666565656456646566786786676786768766",
            "Exponent": "2324324333246536456354655645656616169896565698987033121934984955",
            "Modulus":  "0218305479243590485092843590249879879842313131156656565565656566",
            "Expected": "012F2865E8B9E79B645FCE3A9E04156483AE1F9833F6BFCF86FCA38FC2D5BEF0"
        },
        {
            "Base":     "0000000000000000000000000000000000000000000000000000000000000005",
            "Exponent": "0000000000000000000000000000000000000000000000000000000000000002",
            "Modulus":  "0000000000000000000000000000000000000000000000000000000000000007",
            "Expected": "0000000000000000000000000000000000000000000000000000000000000004"
        },
        {
            "Base":     "0000000000000000000000000000000000000000000000000000000000000019",
            "Exponent": "0000000000000000000000000000000000000000000000000000000000000019",
            "Modulus":  "0000000000000000000000000000000000000000000000000000000000000064",
            "Expected": "0000000000000000000000000000000000000000000000000000000000000019"
        }
    ]"#;

    let to_big = |str : &String| -> BigUint {
        let vec = array_bytes::hex2bytes_unchecked(str);
        BigUint::from_bytes_be(vec.as_slice())
    };

    let test_cases: Vec<TestCase> = serde_json::from_str(test_data).unwrap();
    test_cases.iter().for_each(|test|{
        let base = to_big(&test.base);
        let exponent = to_big(&test.exponent);
        let modulus = to_big(&test.modulus);
        let expected = to_big(&test.expected);

        let result = base.modpow(&exponent, &modulus);
        assert_eq!(result, expected);
    });
}


#[no_mangle]
pub extern "C" fn entrypoint(_input: *mut u8) -> u64 {
    msg!("big_mod_exp");

    big_mod_exp_test();

    0
}

custom_panic_default!();