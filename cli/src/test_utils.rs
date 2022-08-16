use {
    solana_client::rpc_client::RpcClient,
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

pub fn wait_for_next_epoch(rpc_client: &RpcClient) -> Epoch {
    let current_epoch = rpc_client.get_epoch_info().unwrap().epoch;
    println!("waiting for epoch {}", current_epoch + 1);
    loop {
        sleep(Duration::from_millis(DEFAULT_MS_PER_SLOT));

        let next_epoch = rpc_client.get_epoch_info().unwrap().epoch;
        if next_epoch > current_epoch {
            return next_epoch;
        }
    }
}
