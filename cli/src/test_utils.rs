use {
    solana_client::rpc_client::RpcClient,
    solana_sdk::{clock::DEFAULT_MS_PER_SLOT, commitment_config::CommitmentConfig, pubkey::Pubkey},
    std::{thread::sleep, time::Duration},
};

pub fn check_recent_balance(expected_balance: u64, client: &RpcClient, pubkey: &Pubkey) {
    (0..5).for_each(|tries| {
        let balance = client
            .get_balance_with_commitment(pubkey, CommitmentConfig::processed())
            .unwrap()
            .value;
        if balance == expected_balance {
            return;
        }
        if tries == 4 {
            assert_eq!(balance, expected_balance);
        }
        sleep(Duration::from_millis(500));
    });
}

pub fn check_ready(rpc_client: &RpcClient) {
    while rpc_client
        .get_slot_with_commitment(CommitmentConfig::processed())
        .unwrap()
        < 5
    {
        sleep(Duration::from_millis(DEFAULT_MS_PER_SLOT));
    }
}
