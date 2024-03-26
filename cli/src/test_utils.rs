use {
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{
        clock::{Epoch, DEFAULT_MS_PER_SLOT},
        commitment_config::CommitmentConfig,
    },
    std::{thread::sleep, time::Duration},
};

#[macro_export]
macro_rules! check_balance {
    ($expected_balance:expr, $client:expr, $pubkey:expr) => {
        (0..5).for_each(|tries| {
            let balance = $client
                .get_balance_with_commitment($pubkey, CommitmentConfig::processed())
                .unwrap()
                .value;
            if balance == $expected_balance {
                return;
            }
            if tries == 4 {
                assert_eq!(balance, $expected_balance);
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        });
    };
    ($expected_balance:expr, $client:expr, $pubkey:expr,) => {
        check_balance!($expected_balance, $client, $pubkey)
    };
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

pub fn wait_n_slots(rpc_client: &RpcClient, n: u64) -> u64 {
    let slot = rpc_client.get_slot().unwrap();
    loop {
        sleep(Duration::from_millis(DEFAULT_MS_PER_SLOT));
        let new_slot = rpc_client.get_slot().unwrap();
        if new_slot - slot > n {
            return new_slot;
        }
    }
}

pub fn wait_for_next_epoch_plus_n_slots(rpc_client: &RpcClient, n: u64) -> (Epoch, u64) {
    let current_epoch = rpc_client.get_epoch_info().unwrap().epoch;
    let next_epoch = current_epoch + 1;
    println!("waiting for epoch {next_epoch} plus {n} slots");
    loop {
        sleep(Duration::from_millis(DEFAULT_MS_PER_SLOT));

        let next_epoch = rpc_client.get_epoch_info().unwrap().epoch;
        if next_epoch > current_epoch {
            let new_slot = wait_n_slots(rpc_client, n);
            return (next_epoch, new_slot);
        }
    }
}
