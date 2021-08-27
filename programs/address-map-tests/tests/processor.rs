use {
    solana_address_map_program::{
        id,
        instruction::{
            activate, close_account, deactivate, initialize_account, insert_entries, set_authority,
        },
        processor::process_instruction,
        state::{AddressMap, AddressMapState},
    },
    solana_program_test::*,
    solana_sdk::{
        clock::{Epoch, Slot},
        epoch_schedule::EpochSchedule,
        instruction::InstructionError,
        native_token::LAMPORTS_PER_SOL,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_instruction::transfer,
        transaction::{Transaction, TransactionError},
    },
};

const WARP_EPOCH: Epoch = 1;

async fn setup_warped_context() -> ProgramTestContext {
    let program_test = ProgramTest::new("", id(), Some(process_instruction));
    let mut context = program_test.start_with_context().await;
    let epoch_schedule = EpochSchedule::default();
    let warp_slot = epoch_schedule.get_first_slot_in_epoch(WARP_EPOCH);
    context.warp_to_slot(warp_slot).unwrap();
    context
}

#[tokio::test]
async fn test_initialize_account() {
    let mut context = setup_warped_context().await;
    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;

    // First initialization should succeed
    {
        let authority_address = Pubkey::new_unique();
        let (init_account_instruction, map_address) =
            initialize_account(payer.pubkey(), authority_address, WARP_EPOCH, 0);
        let transaction = Transaction::new_signed_with_payer(
            &[init_account_instruction],
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        );

        assert!(client.process_transaction(transaction).await.is_ok());
        let map_account = client.get_account(map_address).await.unwrap().unwrap();
        let map = AddressMap::deserialize(&map_account.data).unwrap();
        assert_eq!(map.authority, Some(authority_address));
        assert_eq!(map.activation_slot, Slot::MAX);
        assert_eq!(map.deactivation_slot, Slot::MAX);
        assert_eq!(map.num_entries, 0u8);
    }

    // Second initialization should fail
    {
        let authority_address = Pubkey::new_unique();
        let transaction = Transaction::new_signed_with_payer(
            &[initialize_account(payer.pubkey(), authority_address, WARP_EPOCH, 0).0],
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        );

        assert_eq!(
            client
                .process_transaction(transaction)
                .await
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(0, InstructionError::AccountAlreadyInitialized),
        );
    }
}

#[tokio::test]
async fn test_initialize_account_wrong_epoch() {
    let mut context = setup_warped_context().await;
    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;
    let transaction = Transaction::new_signed_with_payer(
        &[initialize_account(payer.pubkey(), Pubkey::new_unique(), WARP_EPOCH + 1, 0).0],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );

    assert_eq!(
        client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::InvalidArgument),
    );
}

#[tokio::test]
async fn test_set_authority() {
    let mut context = setup_warped_context().await;
    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;
    let authority = Keypair::new();
    let new_authority = Pubkey::new_unique();

    let (init_account_instruction, map_address) =
        initialize_account(payer.pubkey(), authority.pubkey(), WARP_EPOCH, 0);
    let transaction = Transaction::new_signed_with_payer(
        &[
            init_account_instruction,
            set_authority(map_address, authority.pubkey(), Some(new_authority)),
        ],
        Some(&payer.pubkey()),
        &[payer, &authority],
        recent_blockhash,
    );

    assert!(client.process_transaction(transaction).await.is_ok());
    let map_account = client.get_account(map_address).await.unwrap().unwrap();
    let map = AddressMap::deserialize(&map_account.data).unwrap();
    assert_eq!(map.authority, Some(new_authority));
}

#[tokio::test]
async fn test_insert_entries() {
    let mut context = setup_warped_context().await;
    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;
    let authority = Keypair::new();

    let (init_account_instruction, map_address) =
        initialize_account(payer.pubkey(), authority.pubkey(), WARP_EPOCH, 10);
    let mut entries = Vec::with_capacity(10);
    entries.resize_with(10, Pubkey::new_unique);
    let transaction = Transaction::new_signed_with_payer(
        &[
            init_account_instruction,
            insert_entries(map_address, authority.pubkey(), 0, entries.clone()),
        ],
        Some(&payer.pubkey()),
        &[payer, &authority],
        recent_blockhash,
    );

    assert!(client.process_transaction(transaction).await.is_ok());
    let map_account = client.get_account(map_address).await.unwrap().unwrap();
    let map = AddressMap::deserialize(&map_account.data).unwrap();
    let map_entries = map.deserialize_entries(&map_account.data).unwrap();
    assert_eq!(map_entries, entries);
}

#[tokio::test]
async fn test_activate() {
    let mut context = setup_warped_context().await;
    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;
    let authority = Keypair::new();

    let (init_account_instruction, map_address) =
        initialize_account(payer.pubkey(), authority.pubkey(), WARP_EPOCH, 0);
    let transaction = Transaction::new_signed_with_payer(
        &[
            init_account_instruction,
            activate(map_address, authority.pubkey()),
        ],
        Some(&payer.pubkey()),
        &[payer, &authority],
        recent_blockhash,
    );

    assert!(client.process_transaction(transaction).await.is_ok());
    let map_account = client.get_account(map_address).await.unwrap().unwrap();
    let map = AddressMap::deserialize(&map_account.data).unwrap();
    let current_slot = client.get_root_slot().await.unwrap();
    assert_eq!(map.activation_slot, current_slot);
}

#[tokio::test]
async fn test_deactivate() {
    let mut context = setup_warped_context().await;
    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;
    let authority = Keypair::new();

    let (init_account_instruction, map_address) =
        initialize_account(payer.pubkey(), authority.pubkey(), WARP_EPOCH, 0);
    let transaction = Transaction::new_signed_with_payer(
        &[
            init_account_instruction,
            activate(map_address, authority.pubkey()),
            deactivate(map_address, authority.pubkey()),
        ],
        Some(&payer.pubkey()),
        &[payer, &authority],
        recent_blockhash,
    );

    assert!(client.process_transaction(transaction).await.is_ok());
    let map_account = client.get_account(map_address).await.unwrap().unwrap();
    let map = AddressMap::deserialize(&map_account.data).unwrap();
    let current_slot = client.get_root_slot().await.unwrap();
    assert_eq!(map.deactivation_slot, current_slot);
}

#[tokio::test]
async fn test_close_account() {
    let mut context = setup_warped_context().await;
    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;
    let authority = Keypair::new();

    let (init_account_instruction, map_address) =
        initialize_account(payer.pubkey(), authority.pubkey(), WARP_EPOCH, 0);
    let transaction = Transaction::new_signed_with_payer(
        &[
            init_account_instruction,
            close_account(map_address, payer.pubkey(), authority.pubkey()),
            transfer(&payer.pubkey(), &map_address, LAMPORTS_PER_SOL),
        ],
        Some(&payer.pubkey()),
        &[payer, &authority],
        recent_blockhash,
    );

    assert!(client.process_transaction(transaction).await.is_ok());
    let closed_account = client.get_account(map_address).await.unwrap().unwrap();
    let uninitialized_data = bincode::serialize(&AddressMapState::Uninitialized).unwrap();
    assert_eq!(
        closed_account.data[0..uninitialized_data.len()],
        uninitialized_data[..]
    );
    assert_eq!(closed_account.lamports, LAMPORTS_PER_SOL);
}
