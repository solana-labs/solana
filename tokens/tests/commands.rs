use safecoin_client::rpc_client::RpcClient;
use safecoin_core::test_validator::TestValidator;
use safecoin_sdk::signature::{Keypair, Signer};
use safecoin_tokens::commands::test_process_distribute_tokens_with_client;

#[test]
fn test_process_distribute_with_rpc_client() {
    safecoin_logger::setup();

    let mint_keypair = Keypair::new();
    let test_validator = TestValidator::with_no_fees(mint_keypair.pubkey());

    let client = RpcClient::new(test_validator.rpc_url());
    test_process_distribute_tokens_with_client(&client, mint_keypair, None);
}
