use {
    futures::future::{join_all, Future, FutureExt},
    solana_banks_client::{BanksClient, BanksClientError},
    solana_program_test::ProgramTest,
    solana_sdk::{
        commitment_config::CommitmentLevel, pubkey::Pubkey, signature::Signer,
        transaction::Transaction,
    },
    std::time::{Duration, SystemTime},
    tarpc::context::current,
};

fn one_second_from_now() -> SystemTime {
    SystemTime::now() + Duration::from_secs(1)
}

// like the BanksClient method of the same name,
// but uses a context with a one-second deadline
fn process_transaction_with_commitment(
    client: &mut BanksClient,
    transaction: Transaction,
    commitment: CommitmentLevel,
) -> impl Future<Output = Result<(), BanksClientError>> + '_ {
    let mut ctx = current();
    ctx.deadline = one_second_from_now();
    client
        .process_transaction_with_commitment_and_context(ctx, transaction, commitment)
        .map(|result| match result? {
            None => Err(BanksClientError::ClientError(
                "invalid blockhash or fee-payer",
            )),
            Some(transaction_result) => Ok(transaction_result?),
        })
}

// like the BanksClient method of the same name,
// but uses a context with a one-second deadline
async fn process_transactions_with_commitment(
    client: &mut BanksClient,
    transactions: Vec<Transaction>,
    commitment: CommitmentLevel,
) -> Result<(), BanksClientError> {
    let mut clients: Vec<_> = transactions.iter().map(|_| client.clone()).collect();
    let futures = clients
        .iter_mut()
        .zip(transactions)
        .map(|(client, transaction)| {
            process_transaction_with_commitment(client, transaction, commitment)
        });
    let statuses = join_all(futures).await;
    statuses.into_iter().collect() // Convert Vec<Result<_, _>> to Result<Vec<_>>
}

#[should_panic(expected = "RpcError(DeadlineExceeded")]
#[tokio::test]
async fn timeout() {
    // example of a test that hangs. The reasons for this hanging are a separate issue:
    // https://github.com/solana-labs/solana/issues/30527).
    // We just want to observe the timeout behaviour here.
    let mut pt = ProgramTest::default();
    pt.prefer_bpf(true);
    let context = pt.start_with_context().await;
    let mut client = context.banks_client;
    let payer = context.payer;
    let receiver = Pubkey::new_unique();
    // If you set num_txs to 1, the process_transactions call never hangs.
    // If you set it to 2 it sometimes hangs.
    // If you set it to 10 it seems to always hang.
    // Based on the logs this test usually hangs after processing 3 or 4 transactions
    let num_txs = 10;
    let num_txs_u64 = num_txs as u64;
    let transfer_lamports_base = 1_000_000u64;
    let mut txs: Vec<Transaction> = Vec::with_capacity(num_txs);
    for i in 0..num_txs {
        let ixs = [solana_sdk::system_instruction::transfer(
            &payer.pubkey(),
            &receiver,
            transfer_lamports_base + i as u64, // deduping the tx
        )];
        let msg = solana_sdk::message::Message::new_with_blockhash(
            &ixs,
            Some(&payer.pubkey()),
            &context.last_blockhash,
        );
        let tx = Transaction::new(&[&payer], msg, context.last_blockhash);
        txs.push(tx);
    }
    process_transactions_with_commitment(&mut client, txs, CommitmentLevel::default())
        .await
        .unwrap();
    let balance_after = client.get_balance(receiver).await.unwrap();
    assert_eq!(
        balance_after,
        num_txs_u64 * transfer_lamports_base + ((num_txs_u64 - 1) * num_txs_u64) / 2
    );
}
