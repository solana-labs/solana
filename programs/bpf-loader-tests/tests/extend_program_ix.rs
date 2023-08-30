use {
    assert_matches::assert_matches,
    common::{add_upgradeable_loader_account, assert_ix_error, setup_test_context},
    solana_program_test::*,
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        bpf_loader_upgradeable::{extend_program, id, UpgradeableLoaderState},
        instruction::InstructionError,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_instruction::{self, SystemError, MAX_PERMITTED_DATA_LENGTH},
        system_program,
        transaction::{Transaction, TransactionError},
    },
};

mod common;

#[tokio::test]
async fn test_extend_program() {
    let mut context = setup_test_context().await;
    let program_file = find_file("noop.so").expect("Failed to find the file");
    let data = read_file(program_file);

    let program_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    add_upgradeable_loader_account(
        &mut context,
        &program_address,
        &UpgradeableLoaderState::Program {
            programdata_address,
        },
        UpgradeableLoaderState::size_of_program(),
        |_| {},
    )
    .await;
    let programdata_data_offset = UpgradeableLoaderState::size_of_programdata_metadata();
    let program_data_len = data.len() + programdata_data_offset;
    add_upgradeable_loader_account(
        &mut context,
        &programdata_address,
        &UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(Pubkey::new_unique()),
        },
        program_data_len,
        |account| account.data_as_mut_slice()[programdata_data_offset..].copy_from_slice(&data),
    )
    .await;

    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;
    const ADDITIONAL_BYTES: u32 = 42;
    let transaction = Transaction::new_signed_with_payer(
        &[extend_program(
            &program_address,
            Some(&payer.pubkey()),
            ADDITIONAL_BYTES,
        )],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );

    assert_matches!(client.process_transaction(transaction).await, Ok(()));
    let updated_program_data_account = client
        .get_account(programdata_address)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_program_data_account.data().len(),
        program_data_len + ADDITIONAL_BYTES as usize
    );
}

#[tokio::test]
async fn test_failed_extend_twice_in_same_slot() {
    let mut context = setup_test_context().await;
    let program_file = find_file("noop.so").expect("Failed to find the file");
    let data = read_file(program_file);

    let program_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    add_upgradeable_loader_account(
        &mut context,
        &program_address,
        &UpgradeableLoaderState::Program {
            programdata_address,
        },
        UpgradeableLoaderState::size_of_program(),
        |_| {},
    )
    .await;
    let programdata_data_offset = UpgradeableLoaderState::size_of_programdata_metadata();
    let program_data_len = data.len() + programdata_data_offset;
    add_upgradeable_loader_account(
        &mut context,
        &programdata_address,
        &UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(Pubkey::new_unique()),
        },
        program_data_len,
        |account| account.data_as_mut_slice()[programdata_data_offset..].copy_from_slice(&data),
    )
    .await;

    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;
    const ADDITIONAL_BYTES: u32 = 42;
    let transaction = Transaction::new_signed_with_payer(
        &[extend_program(
            &program_address,
            Some(&payer.pubkey()),
            ADDITIONAL_BYTES,
        )],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );

    assert_matches!(client.process_transaction(transaction).await, Ok(()));
    let updated_program_data_account = client
        .get_account(programdata_address)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_program_data_account.data().len(),
        program_data_len + ADDITIONAL_BYTES as usize
    );

    let recent_blockhash = client
        .get_new_latest_blockhash(&recent_blockhash)
        .await
        .unwrap();
    // Extending the program in the same slot should fail
    let transaction = Transaction::new_signed_with_payer(
        &[extend_program(
            &program_address,
            Some(&payer.pubkey()),
            ADDITIONAL_BYTES,
        )],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );

    assert_matches!(
        client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(0, InstructionError::InvalidArgument)
    );
}

#[tokio::test]
async fn test_extend_program_not_upgradeable() {
    let mut context = setup_test_context().await;

    let program_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    add_upgradeable_loader_account(
        &mut context,
        &program_address,
        &UpgradeableLoaderState::Program {
            programdata_address,
        },
        UpgradeableLoaderState::size_of_program(),
        |_| {},
    )
    .await;
    add_upgradeable_loader_account(
        &mut context,
        &programdata_address,
        &UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: None,
        },
        100,
        |_| {},
    )
    .await;

    let payer_address = context.payer.pubkey();
    assert_ix_error(
        &mut context,
        extend_program(&program_address, Some(&payer_address), 42),
        None,
        InstructionError::Immutable,
        "should fail because the program data account isn't upgradeable",
    )
    .await;
}

#[tokio::test]
async fn test_extend_program_by_zero_bytes() {
    let mut context = setup_test_context().await;

    let program_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    add_upgradeable_loader_account(
        &mut context,
        &program_address,
        &UpgradeableLoaderState::Program {
            programdata_address,
        },
        UpgradeableLoaderState::size_of_program(),
        |_| {},
    )
    .await;
    add_upgradeable_loader_account(
        &mut context,
        &programdata_address,
        &UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(Pubkey::new_unique()),
        },
        100,
        |_| {},
    )
    .await;

    let payer_address = context.payer.pubkey();
    assert_ix_error(
        &mut context,
        extend_program(&program_address, Some(&payer_address), 0),
        None,
        InstructionError::InvalidInstructionData,
        "should fail because the program data account must be extended by more than 0 bytes",
    )
    .await;
}

#[tokio::test]
async fn test_extend_program_past_max_size() {
    let mut context = setup_test_context().await;

    let program_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    add_upgradeable_loader_account(
        &mut context,
        &program_address,
        &UpgradeableLoaderState::Program {
            programdata_address,
        },
        UpgradeableLoaderState::size_of_program(),
        |_| {},
    )
    .await;
    add_upgradeable_loader_account(
        &mut context,
        &programdata_address,
        &UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(Pubkey::new_unique()),
        },
        MAX_PERMITTED_DATA_LENGTH as usize,
        |_| {},
    )
    .await;

    let payer_address = context.payer.pubkey();
    assert_ix_error(
        &mut context,
        extend_program(&program_address, Some(&payer_address), 1),
        None,
        InstructionError::InvalidRealloc,
        "should fail because the program data account cannot be extended past the max data size",
    )
    .await;
}

#[tokio::test]
async fn test_extend_program_with_invalid_payer() {
    let mut context = setup_test_context().await;
    let rent = context.banks_client.get_rent().await.unwrap();

    let program_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    add_upgradeable_loader_account(
        &mut context,
        &program_address,
        &UpgradeableLoaderState::Program {
            programdata_address,
        },
        UpgradeableLoaderState::size_of_program(),
        |_| {},
    )
    .await;
    add_upgradeable_loader_account(
        &mut context,
        &programdata_address,
        &UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(Pubkey::new_unique()),
        },
        100,
        |_| {},
    )
    .await;

    let payer_with_sufficient_funds = Keypair::new();
    context.set_account(
        &payer_with_sufficient_funds.pubkey(),
        &AccountSharedData::new(10_000_000_000, 0, &system_program::id()),
    );

    let payer_with_insufficient_funds = Keypair::new();
    context.set_account(
        &payer_with_insufficient_funds.pubkey(),
        &AccountSharedData::new(rent.minimum_balance(0), 0, &system_program::id()),
    );

    let payer_with_invalid_owner = Keypair::new();
    context.set_account(
        &payer_with_invalid_owner.pubkey(),
        &AccountSharedData::new(rent.minimum_balance(0), 0, &id()),
    );

    assert_ix_error(
        &mut context,
        extend_program(
            &program_address,
            Some(&payer_with_insufficient_funds.pubkey()),
            1024,
        ),
        Some(&payer_with_insufficient_funds),
        InstructionError::from(SystemError::ResultWithNegativeLamports),
        "should fail because the payer has insufficient funds to cover program data account rent",
    )
    .await;

    assert_ix_error(
        &mut context,
        extend_program(
            &program_address,
            Some(&payer_with_invalid_owner.pubkey()),
            1,
        ),
        Some(&payer_with_invalid_owner),
        InstructionError::ExternalAccountLamportSpend,
        "should fail because the payer is not a system account",
    )
    .await;

    let mut ix = extend_program(
        &program_address,
        Some(&payer_with_sufficient_funds.pubkey()),
        1,
    );

    // Demote payer account meta to non-signer so that transaction signing succeeds
    {
        let payer_meta = ix
            .accounts
            .iter_mut()
            .find(|meta| meta.pubkey == payer_with_sufficient_funds.pubkey())
            .expect("expected to find payer account meta");
        payer_meta.is_signer = false;
    }

    assert_ix_error(
        &mut context,
        ix,
        None,
        InstructionError::PrivilegeEscalation,
        "should fail because the payer did not sign",
    )
    .await;
}

#[tokio::test]
async fn test_extend_program_without_payer() {
    let mut context = setup_test_context().await;
    let rent = context.banks_client.get_rent().await.unwrap();

    let program_file = find_file("noop.so").expect("Failed to find the file");
    let data = read_file(program_file);

    let program_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    add_upgradeable_loader_account(
        &mut context,
        &program_address,
        &UpgradeableLoaderState::Program {
            programdata_address,
        },
        UpgradeableLoaderState::size_of_program(),
        |_| {},
    )
    .await;
    let programdata_data_offset = UpgradeableLoaderState::size_of_programdata_metadata();
    let program_data_len = data.len() + programdata_data_offset;
    add_upgradeable_loader_account(
        &mut context,
        &programdata_address,
        &UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(Pubkey::new_unique()),
        },
        program_data_len,
        |account| account.data_as_mut_slice()[programdata_data_offset..].copy_from_slice(&data),
    )
    .await;

    assert_ix_error(
        &mut context,
        extend_program(&program_address, None, 1024),
        None,
        InstructionError::NotEnoughAccountKeys,
        "should fail because program data has insufficient funds to cover rent",
    )
    .await;

    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;

    const ADDITIONAL_BYTES: u32 = 42;
    let min_balance_increase_for_extend = rent
        .minimum_balance(ADDITIONAL_BYTES as usize)
        .saturating_sub(rent.minimum_balance(0));

    let transaction = Transaction::new_signed_with_payer(
        &[
            system_instruction::transfer(
                &payer.pubkey(),
                &programdata_address,
                min_balance_increase_for_extend,
            ),
            extend_program(&program_address, None, ADDITIONAL_BYTES),
        ],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );

    assert_matches!(client.process_transaction(transaction).await, Ok(()));
    let updated_program_data_account = client
        .get_account(programdata_address)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_program_data_account.data().len(),
        program_data_len + ADDITIONAL_BYTES as usize
    );
}

#[tokio::test]
async fn test_extend_program_with_invalid_system_program() {
    let mut context = setup_test_context().await;

    let program_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    add_upgradeable_loader_account(
        &mut context,
        &program_address,
        &UpgradeableLoaderState::Program {
            programdata_address,
        },
        UpgradeableLoaderState::size_of_program(),
        |_| {},
    )
    .await;
    let program_data_len = 100;
    add_upgradeable_loader_account(
        &mut context,
        &programdata_address,
        &UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(Pubkey::new_unique()),
        },
        program_data_len,
        |_| {},
    )
    .await;

    let payer_address = context.payer.pubkey();
    let mut ix = extend_program(&program_address, Some(&payer_address), 1);

    // Change system program to an invalid key
    {
        let system_program_meta = ix
            .accounts
            .iter_mut()
            .find(|meta| meta.pubkey == crate::system_program::ID)
            .expect("expected to find system program account meta");
        system_program_meta.pubkey = Pubkey::new_unique();
    }

    assert_ix_error(
        &mut context,
        ix,
        None,
        InstructionError::MissingAccount,
        "should fail because the system program is missing",
    )
    .await;
}

#[tokio::test]
async fn test_extend_program_with_mismatch_program_data() {
    let mut context = setup_test_context().await;
    let payer_address = context.payer.pubkey();

    let program_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    add_upgradeable_loader_account(
        &mut context,
        &program_address,
        &UpgradeableLoaderState::Program {
            programdata_address,
        },
        UpgradeableLoaderState::size_of_program(),
        |_| {},
    )
    .await;

    let mismatch_programdata_address = Pubkey::new_unique();
    add_upgradeable_loader_account(
        &mut context,
        &mismatch_programdata_address,
        &UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(Pubkey::new_unique()),
        },
        100,
        |_| {},
    )
    .await;

    let mut ix = extend_program(&program_address, Some(&payer_address), 1);

    // Replace ProgramData account meta with invalid account
    {
        let program_data_meta = ix
            .accounts
            .iter_mut()
            .find(|meta| meta.pubkey == programdata_address)
            .expect("expected to find program data account meta");
        program_data_meta.pubkey = mismatch_programdata_address;
    }

    assert_ix_error(
        &mut context,
        ix,
        None,
        InstructionError::InvalidArgument,
        "should fail because the program data account doesn't match the program",
    )
    .await;
}

#[tokio::test]
async fn test_extend_program_with_readonly_program_data() {
    let mut context = setup_test_context().await;
    let payer_address = context.payer.pubkey();

    let program_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    add_upgradeable_loader_account(
        &mut context,
        &program_address,
        &UpgradeableLoaderState::Program {
            programdata_address,
        },
        UpgradeableLoaderState::size_of_program(),
        |_| {},
    )
    .await;
    add_upgradeable_loader_account(
        &mut context,
        &programdata_address,
        &UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(Pubkey::new_unique()),
        },
        100,
        |_| {},
    )
    .await;

    let mut ix = extend_program(&program_address, Some(&payer_address), 1);

    // Demote ProgramData account meta to read-only
    {
        let program_data_meta = ix
            .accounts
            .iter_mut()
            .find(|meta| meta.pubkey == programdata_address)
            .expect("expected to find program data account meta");
        program_data_meta.is_writable = false;
    }

    assert_ix_error(
        &mut context,
        ix,
        None,
        InstructionError::InvalidArgument,
        "should fail because the program data account is not writable",
    )
    .await;
}

#[tokio::test]
async fn test_extend_program_with_invalid_program_data_state() {
    let mut context = setup_test_context().await;
    let payer_address = context.payer.pubkey();

    let program_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    add_upgradeable_loader_account(
        &mut context,
        &program_address,
        &UpgradeableLoaderState::Program {
            programdata_address,
        },
        UpgradeableLoaderState::size_of_program(),
        |_| {},
    )
    .await;
    add_upgradeable_loader_account(
        &mut context,
        &programdata_address,
        &UpgradeableLoaderState::Buffer {
            authority_address: Some(payer_address),
        },
        100,
        |_| {},
    )
    .await;

    assert_ix_error(
        &mut context,
        extend_program(&program_address, Some(&payer_address), 1024),
        None,
        InstructionError::InvalidAccountData,
        "should fail because the program data account state isn't valid",
    )
    .await;
}

#[tokio::test]
async fn test_extend_program_with_invalid_program_data_owner() {
    let mut context = setup_test_context().await;
    let payer_address = context.payer.pubkey();

    let program_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    add_upgradeable_loader_account(
        &mut context,
        &program_address,
        &UpgradeableLoaderState::Program {
            programdata_address,
        },
        UpgradeableLoaderState::size_of_program(),
        |_| {},
    )
    .await;

    let invalid_owner = Pubkey::new_unique();
    add_upgradeable_loader_account(
        &mut context,
        &program_address,
        &UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(payer_address),
        },
        100,
        |account| account.set_owner(invalid_owner),
    )
    .await;

    assert_ix_error(
        &mut context,
        extend_program(&program_address, Some(&payer_address), 1024),
        None,
        InstructionError::InvalidAccountOwner,
        "should fail because the program data account owner isn't valid",
    )
    .await;
}

#[tokio::test]
async fn test_extend_program_with_readonly_program() {
    let mut context = setup_test_context().await;
    let payer_address = context.payer.pubkey();

    let program_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    add_upgradeable_loader_account(
        &mut context,
        &program_address,
        &UpgradeableLoaderState::Program {
            programdata_address,
        },
        UpgradeableLoaderState::size_of_program(),
        |_| {},
    )
    .await;
    add_upgradeable_loader_account(
        &mut context,
        &programdata_address,
        &UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(Pubkey::new_unique()),
        },
        100,
        |_| {},
    )
    .await;

    let mut ix = extend_program(&program_address, Some(&payer_address), 1);

    // Demote Program account meta to read-only
    {
        let program_meta = ix
            .accounts
            .iter_mut()
            .find(|meta| meta.pubkey == program_address)
            .expect("expected to find program account meta");
        program_meta.is_writable = false;
    }

    assert_ix_error(
        &mut context,
        ix,
        None,
        InstructionError::InvalidArgument,
        "should fail because the program account is not writable",
    )
    .await;
}

#[tokio::test]
async fn test_extend_program_with_invalid_program_owner() {
    let mut context = setup_test_context().await;
    let payer_address = context.payer.pubkey();

    let program_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    let invalid_owner = Pubkey::new_unique();
    add_upgradeable_loader_account(
        &mut context,
        &program_address,
        &UpgradeableLoaderState::Program {
            programdata_address,
        },
        UpgradeableLoaderState::size_of_program(),
        |account| account.set_owner(invalid_owner),
    )
    .await;
    add_upgradeable_loader_account(
        &mut context,
        &programdata_address,
        &UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(Pubkey::new_unique()),
        },
        100,
        |_| {},
    )
    .await;

    assert_ix_error(
        &mut context,
        extend_program(&program_address, Some(&payer_address), 1024),
        None,
        InstructionError::InvalidAccountOwner,
        "should fail because the program account owner isn't valid",
    )
    .await;
}

#[tokio::test]
async fn test_extend_program_with_invalid_program_state() {
    let mut context = setup_test_context().await;
    let payer_address = context.payer.pubkey();

    let program_address = Pubkey::new_unique();
    let (programdata_address, _) = Pubkey::find_program_address(&[program_address.as_ref()], &id());
    add_upgradeable_loader_account(
        &mut context,
        &program_address,
        &UpgradeableLoaderState::Buffer {
            authority_address: Some(payer_address),
        },
        100,
        |_| {},
    )
    .await;

    add_upgradeable_loader_account(
        &mut context,
        &programdata_address,
        &UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(Pubkey::new_unique()),
        },
        100,
        |_| {},
    )
    .await;

    assert_ix_error(
        &mut context,
        extend_program(&program_address, Some(&payer_address), 1024),
        None,
        InstructionError::InvalidAccountData,
        "should fail because the program account state isn't valid",
    )
    .await;
}
