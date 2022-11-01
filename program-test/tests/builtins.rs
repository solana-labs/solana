use {
    solana_program_test::ProgramTest,
    solana_sdk::{
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        signature::{Keypair, Signer},
        transaction::Transaction,
    },
};

#[tokio::test]
async fn test_bpf_loader_upgradeable_present() {
    // Arrange
    let (mut banks_client, payer, recent_blockhash) = ProgramTest::default().start().await;

    let buffer_keypair = Keypair::new();
    let upgrade_authority_keypair = Keypair::new();

    let rent = banks_client.get_rent().await.unwrap();
    let buffer_rent = rent.minimum_balance(UpgradeableLoaderState::size_of_programdata(1));

    let create_buffer_instructions = bpf_loader_upgradeable::create_buffer(
        &payer.pubkey(),
        &buffer_keypair.pubkey(),
        &upgrade_authority_keypair.pubkey(),
        buffer_rent,
        1,
    )
    .unwrap();

    let mut transaction =
        Transaction::new_with_payer(&create_buffer_instructions[..], Some(&payer.pubkey()));
    transaction.sign(&[&payer, &buffer_keypair], recent_blockhash);

    // Act
    banks_client.process_transaction(transaction).await.unwrap();

    // Assert
    let buffer_account = banks_client
        .get_account(buffer_keypair.pubkey())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(buffer_account.owner, bpf_loader_upgradeable::id());
}
