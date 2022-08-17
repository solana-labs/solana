use {
    assert_matches::assert_matches,
    common::{assert_ix_error, overwrite_slot_hashes_with_slots, setup_test_context},
    solana_address_lookup_table_program::{
        id,
        instruction::create_lookup_table_v2,
        state::{AddressLookupTable, LOOKUP_TABLE_META_SIZE},
    },
    solana_program_test::*,
    solana_sdk::{
        clock::Slot, instruction::InstructionError, pubkey::Pubkey, rent::Rent, signature::Signer,
        signer::keypair::Keypair, transaction::Transaction,
    },
};

mod common;

#[tokio::test]
async fn test_create_lookup_table_v2() {
    let mut context = setup_test_context().await;

    let test_recent_slot = 123;
    overwrite_slot_hashes_with_slots(&mut context, &[test_recent_slot]);

    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;
    let authority_keypair = Keypair::new();
    let authority_address = authority_keypair.pubkey();
    let (create_lookup_table_ix, lookup_table_address) =
        create_lookup_table_v2(authority_address, payer.pubkey(), test_recent_slot);

    // First create should succeed
    {
        let transaction = Transaction::new_signed_with_payer(
            &[create_lookup_table_ix.clone()],
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        );

        assert_matches!(client.process_transaction(transaction).await, Ok(()));
        let lookup_table_account = client
            .get_account(lookup_table_address)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(lookup_table_account.owner, crate::id());
        assert_eq!(lookup_table_account.data.len(), LOOKUP_TABLE_META_SIZE);
        assert_eq!(
            lookup_table_account.lamports,
            Rent::default().minimum_balance(LOOKUP_TABLE_META_SIZE)
        );
        let lookup_table = AddressLookupTable::deserialize(&lookup_table_account.data).unwrap();
        assert_eq!(lookup_table.meta.deactivation_slot, Slot::MAX);
        assert_eq!(lookup_table.meta.authority, Some(authority_address));
        assert_eq!(lookup_table.meta.last_extended_slot, 0);
        assert_eq!(lookup_table.meta.last_extended_slot_start_index, 0);
        assert_eq!(lookup_table.addresses.len(), 0);
    }

    // Second create should fail
    {
        context.last_blockhash = client
            .get_new_latest_blockhash(&recent_blockhash)
            .await
            .unwrap();
        assert_ix_error(
            &mut context,
            create_lookup_table_ix,
            None,
            InstructionError::AccountAlreadyInitialized,
        )
        .await;
    }
}

#[tokio::test]
async fn test_create_lookup_table_use_payer_as_authority() {
    let mut context = setup_test_context().await;

    let test_recent_slot = 123;
    overwrite_slot_hashes_with_slots(&mut context, &[test_recent_slot]);

    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;
    let authority_address = payer.pubkey();
    let transaction = Transaction::new_signed_with_payer(
        &[create_lookup_table_v2(authority_address, payer.pubkey(), test_recent_slot).0],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );

    assert_matches!(client.process_transaction(transaction).await, Ok(()));
}

#[tokio::test]
async fn test_create_lookup_table_not_recent_slot() {
    let mut context = setup_test_context().await;
    let payer = &context.payer;
    let authority_keypair = Keypair::new();
    let authority_address = authority_keypair.pubkey();

    let ix = create_lookup_table_v2(authority_address, payer.pubkey(), Slot::MAX).0;

    assert_ix_error(
        &mut context,
        ix,
        None,
        InstructionError::InvalidInstructionData,
    )
    .await;
}

#[tokio::test]
async fn test_create_lookup_table_pda_mismatch() {
    let mut context = setup_test_context().await;
    let test_recent_slot = 123;
    overwrite_slot_hashes_with_slots(&mut context, &[test_recent_slot]);
    let payer = &context.payer;
    let authority_keypair = Keypair::new();
    let authority_address = authority_keypair.pubkey();

    let mut ix = create_lookup_table_v2(authority_address, payer.pubkey(), test_recent_slot).0;
    ix.accounts[0].pubkey = Pubkey::new_unique();

    assert_ix_error(&mut context, ix, None, InstructionError::InvalidArgument).await;
}
