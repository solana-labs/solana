use solana_client::rpc_client::RpcClient;
use solana_core::validator::TestValidator;
use solana_faucet::faucet::{request_airdrop_transaction, run_local_faucet};
use solana_sdk::{
    native_token::sol_to_lamports,
    signature::{Keypair, Signer},
};
use solana_tokens::{thin_client::ThinClient, tokens::test_process_distribute_with_client};
use std::sync::mpsc::channel;

#[test]
fn test_process_distribute_with_rpc_client() {
    let validator = TestValidator::run();
    let (sender, receiver) = channel();
    run_local_faucet(validator.alice, sender, None);
    let faucet_addr = receiver.recv().unwrap();

    let rpc_client = RpcClient::new_socket(validator.leader_data.rpc);
    let (blockhash, _fee_calculator) = rpc_client.get_recent_blockhash().unwrap();
    let funding_keypair = Keypair::new();
    let lamports = 1_000;
    let mut transaction =
        request_airdrop_transaction(&faucet_addr, &funding_keypair.pubkey(), lamports, blockhash)
            .unwrap();
    rpc_client
        .send_and_confirm_transaction_with_spinner(&mut transaction, &[&funding_keypair])
        .unwrap();

    let thin_client = ThinClient(rpc_client);
    test_process_distribute_with_client(&thin_client, funding_keypair);
}
