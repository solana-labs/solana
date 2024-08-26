use {
    borsh::{from_slice, to_vec, BorshDeserialize, BorshSerialize},
    solana_program::{
        account_info::AccountInfo, entrypoint::ProgramResult, program::set_return_data,
        pubkey::Pubkey,
    },
};

#[derive(BorshSerialize, BorshDeserialize, Clone, Copy)]
struct Test128 {
    a: u128,
    b: u128,
}

#[derive(BorshDeserialize)]
struct InputData {
    test_128: Test128,
    arg1: i64,
    arg2: i64,
    arg3: i64,
    arg4: i64,
    arg5: i64,
    arg6: i64,
    arg7: i64,
    arg8: i64,
}

#[derive(BorshSerialize)]
struct OutputData {
    res_128: u128,
    res_256: Test128,
    many_args_1: i64,
    many_args_2: i64,
}

solana_program::entrypoint_no_alloc!(entry);

pub fn entry(_program_id: &Pubkey, _accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    // This code is supposed to occupy stack space. The purpose of this test is to make sure
    // we operate on the limits of the stack frame safely.
    let buffer: [u8; 3800] = [1; 3800];

    let mut x: [u8; 16] = [0; 16];
    x.copy_from_slice(&buffer[3784..3800]);
    x[10] = 0x39;
    x[11] = 0x37;

    // Assert the function call hasn't overwritten these values
    check_arr(x);
    assert_eq!(x[10], 0x39);
    assert_eq!(x[11], 0x37);

    // The function call must not overwrite the values and the return must be correct.
    let y = check_arr_and_return(x);
    assert_eq!(x[10], 0x39);
    assert_eq!(x[11], 0x37);
    assert_eq!(y[10], 0x39);
    assert_eq!(y[11], 0x37);
    assert_eq!(y[15], 17);

    let decoded: InputData = from_slice::<InputData>(data).unwrap();

    let output = OutputData {
        res_128: test_128_arg(decoded.test_128.a, decoded.test_128.b),
        res_256: test_256_arg(decoded.test_128),
        many_args_1: many_args(
            decoded.arg1,
            decoded.arg2,
            decoded.arg3,
            decoded.arg4,
            decoded.arg5,
            decoded.arg6,
            decoded.arg7,
            decoded.arg8,
        ),
        many_args_2: many_args_stack_space(
            decoded.arg1,
            decoded.arg2,
            decoded.arg3,
            decoded.arg4,
            decoded.arg5,
            decoded.arg6,
            decoded.arg7,
            decoded.arg8,
        ),
    };

    let encoded = to_vec(&output).unwrap();

    set_return_data(encoded.as_slice());

    Ok(())
}

// In this function the argument is promoted to a pointer, so it does not overwrite the stack.
#[allow(improper_ctypes_definitions)]
#[inline(never)]
extern "C" fn check_arr(x: [u8; 16]) {
    for (idx, item) in x.iter().enumerate() {
        if idx != 10 && idx != 11 {
            assert!(*item == 1u8);
        }
    }
    assert_eq!(x[11], 0x37);
    assert_eq!(x[10], 0x39);
}

// Both the argument and return value are promoted to pointers.
#[allow(improper_ctypes_definitions)]
#[inline(never)]
extern "C" fn check_arr_and_return(mut x: [u8; 16]) -> [u8; 16] {
    for (idx, item) in x.iter().enumerate() {
        if idx != 10 && idx != 11 {
            assert!(*item == 1u8);
        }
    }
    assert_eq!(x[11], 0x37);
    assert_eq!(x[10], 0x39);
    x[15] = 17;
    x
}

// Test a 128 bit argument
#[allow(clippy::arithmetic_side_effects)]
#[inline(never)]
fn test_128_arg(x: u128, y: u128) -> u128 {
    x % y
}

// Test a 256-bit argument
#[allow(clippy::arithmetic_side_effects)]
#[inline(never)]
fn test_256_arg(x: Test128) -> Test128 {
    Test128 {
        a: x.a + x.b,
        b: x.a - x.b,
    }
}

// Test a function that needs to save arguments in the stack
#[allow(clippy::arithmetic_side_effects)]
#[inline(never)]
extern "C" fn many_args(a: i64, b: i64, c: i64, d: i64, e: i64, f: i64, g: i64, h: i64) -> i64 {
    let i = a + b;
    let j = i - c;
    let k = j + d;
    let l = k - e;
    let m = l % f;
    let n = m - g;
    n + h
}

// Test a function that utilizes stack space and needs to retrieve arguments from the caller stack
#[allow(clippy::arithmetic_side_effects)]
#[inline(never)]
extern "C" fn many_args_stack_space(
    a: i64,
    b: i64,
    c: i64,
    d: i64,
    e: i64,
    f: i64,
    g: i64,
    h: i64,
) -> i64 {
    let s: [i64; 3] = [1, 2, 3];
    let i = a + b;
    let j = i - c;
    let k = j + d;
    let l = k - e;
    let m = l % f;
    let n = m - g;
    let o = n + h;
    let p = o + s[0];
    let q = p + s[1];
    q - s[2]
}
