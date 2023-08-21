use {
    assert_matches::assert_matches,
    solana_program_test::*,
    solana_sdk::{
        ed25519_instruction::new_ed25519_instruction,
        signature::Signer,
        transaction::{Transaction, TransactionError},
    },
};

// Since ed25519_dalek is still using the old version of rand, this test
// copies the `generate` implementation at:
// https://docs.rs/ed25519-dalek/1.0.1/src/ed25519_dalek/secret.rs.html#167
fn generate_keypair() -> ed25519_dalek::Keypair {
    use rand::RngCore;
    let mut rng = rand::thread_rng();
    let mut seed = [0u8; ed25519_dalek::SECRET_KEY_LENGTH];
    rng.fill_bytes(&mut seed);
    let secret =
        ed25519_dalek::SecretKey::from_bytes(&seed[..ed25519_dalek::SECRET_KEY_LENGTH]).unwrap();
    let public = ed25519_dalek::PublicKey::from(&secret);
    ed25519_dalek::Keypair { secret, public }
}

#[tokio::test]
async fn test_success() {
    let mut context = ProgramTest::default().start_with_context().await;

    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;

    let privkey = generate_keypair();
    let message_arr = b"hello";
    let instruction = new_ed25519_instruction(&privkey, message_arr);

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );

    assert_matches!(client.process_transaction(transaction).await, Ok(()));
}

#[tokio::test]
async fn test_failure() {
    let mut context = ProgramTest::default().start_with_context().await;

    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;

    let privkey = generate_keypair();
    let message_arr = b"hello";
    let mut instruction = new_ed25519_instruction(&privkey, message_arr);

    instruction.data[0] += 1;

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );

    assert_matches!(
        client.process_transaction(transaction).await,
        Err(BanksClientError::TransactionError(
            TransactionError::InvalidAccountIndex
        ))
    );
}
