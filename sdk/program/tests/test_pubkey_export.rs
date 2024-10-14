use {
    solana_program::{pubkey, pubkey::Pubkey},
    std::str::FromStr,
};

// solana_program::pubkey refers to both a module and a macro.
// This test demonstrates that both imports are working
#[test]
fn test_pubkey_import() {
    let pk = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
    assert_eq!(
        pk,
        Pubkey::from_str("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL").unwrap()
    );
}
