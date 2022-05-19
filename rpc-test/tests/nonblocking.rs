use {
    solana_client::{nonblocking::tpu_client::TpuClient, tpu_client::TpuClientConfig},
    solana_sdk::{pubkey::Pubkey, system_transaction},
    solana_test_validator::TestValidatorGenesis,
    std::{
        sync::Arc,
        time::{Duration, Instant},
    },
};

#[tokio::test]
async fn test_tpu_send_transaction() {
    let (test_validator, mint_keypair) = TestValidatorGenesis::default().start_async().await;
    let rpc_client = Arc::new(test_validator.get_async_rpc_client());
    let tpu_client = TpuClient::new(
        rpc_client.clone(),
        &test_validator.rpc_pubsub_url(),
        TpuClientConfig::default(),
    )
    .await
    .unwrap();

    let recent_blockhash = rpc_client.get_latest_blockhash().await.unwrap();
    let tx =
        system_transaction::transfer(&mint_keypair, &Pubkey::new_unique(), 42, recent_blockhash);
    assert!(tpu_client.send_transaction(&tx).await);

    let timeout = Duration::from_secs(5);
    let now = Instant::now();
    let signatures = vec![tx.signatures[0]];
    loop {
        assert!(now.elapsed() < timeout);
        let statuses = rpc_client
            .get_signature_statuses(&signatures)
            .await
            .unwrap();
        if statuses.value.get(0).is_some() {
            return;
        }
    }
}
