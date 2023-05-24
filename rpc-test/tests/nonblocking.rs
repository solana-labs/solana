use {
    solana_client::{
        connection_cache::Protocol,
        nonblocking::tpu_client::{LeaderTpuService, TpuClient},
        tpu_client::TpuClientConfig,
    },
    solana_sdk::{clock::DEFAULT_MS_PER_SLOT, pubkey::Pubkey, system_transaction},
    solana_test_validator::TestValidatorGenesis,
    std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    tokio::time::{sleep, Duration, Instant},
};

#[tokio::test]
async fn test_tpu_send_transaction() {
    let (test_validator, mint_keypair) = TestValidatorGenesis::default().start_async().await;
    let rpc_client = Arc::new(test_validator.get_async_rpc_client());
    let mut tpu_client = TpuClient::new(
        "tpu_client_test",
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
            break;
        }
    }
    tpu_client.shutdown().await;
}

#[tokio::test]
async fn test_tpu_cache_slot_updates() {
    let (test_validator, _) = TestValidatorGenesis::default().start_async().await;
    let rpc_client = Arc::new(test_validator.get_async_rpc_client());
    let exit = Arc::new(AtomicBool::new(false));
    let mut leader_tpu_service = LeaderTpuService::new(
        rpc_client,
        &test_validator.rpc_pubsub_url(),
        Protocol::QUIC,
        exit.clone(),
    )
    .await
    .unwrap();
    let start_slot = leader_tpu_service.estimated_current_slot();
    let timeout = Duration::from_secs(5);
    let sleep_time = Duration::from_millis(DEFAULT_MS_PER_SLOT);
    let now = Instant::now();
    loop {
        assert!(now.elapsed() < timeout);
        let current_slot = leader_tpu_service.estimated_current_slot();
        if current_slot != start_slot {
            break;
        }
        sleep(sleep_time).await;
    }
    exit.store(true, Ordering::Relaxed);
    leader_tpu_service.join().await;
}
