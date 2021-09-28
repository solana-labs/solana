use {
    solana_program_test::ProgramTest,
    solana_sdk::{
        instruction::Instruction, packet::PACKET_DATA_SIZE, pubkey::Pubkey, signature::Signer,
        transaction::Transaction,
    },
};

#[tokio::test]
async fn test_transaction_too_large() {
    let (mut banks_client, payer, recent_blockhash) = ProgramTest::default().start().await;

    let mut transaction = Transaction::new_with_payer(
        &[Instruction {
            program_id: Pubkey::default(),
            accounts: vec![],
            data: [42; PACKET_DATA_SIZE * 2].to_vec(),
        }],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[&payer], recent_blockhash);

    assert_eq!(
        "transport transaction error: Transaction too large",
        &banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .to_string(),
    );
}
