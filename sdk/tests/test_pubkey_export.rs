// Simple test to make sure we haven't broken the re-export of the pubkey macro in solana_sdk
#[test]
fn test_sdk_pubkey_export() {
    assert_eq!(
        solana_sdk::pubkey!("ZkTokenProof1111111111111111111111111111111"),
        solana_pubkey::pubkey!("ZkTokenProof1111111111111111111111111111111")
    );
}
