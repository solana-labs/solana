use {
    assert_matches::assert_matches,
    common::{
        add_lookup_table_account, assert_ix_error, new_address_lookup_table, setup_test_context,
    },
    solana_program_test::*,
    solana_sdk::{
        account::{ReadableAccount, WritableAccount},
        address_lookup_table::{
            instruction::extend_lookup_table,
            state::{AddressLookupTable, LookupTableMeta},
        },
        clock::Clock,
        instruction::{Instruction, InstructionError},
        pubkey::{Pubkey, PUBKEY_BYTES},
        signature::{Keypair, Signer},
        transaction::{Transaction, TransactionError},
    },
    std::{borrow::Cow, result::Result},
};

mod common;

struct ExpectedTableAccount {
    lamports: u64,
    data_len: usize,
    state: AddressLookupTable<'static>,
}

struct TestCase<'a> {
    lookup_table_address: Pubkey,
    instruction: Instruction,
    extra_signer: Option<&'a Keypair>,
    expected_result: Result<ExpectedTableAccount, InstructionError>,
}

async fn run_test_case(context: &mut ProgramTestContext, test_case: TestCase<'_>) {
    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;

    let mut signers = vec![payer];
    if let Some(extra_signer) = test_case.extra_signer {
        signers.push(extra_signer);
    }

    let transaction = Transaction::new_signed_with_payer(
        &[test_case.instruction],
        Some(&payer.pubkey()),
        &signers,
        recent_blockhash,
    );

    let process_result = client.process_transaction(transaction).await;

    match test_case.expected_result {
        Ok(expected_account) => {
            assert_matches!(process_result, Ok(()));

            let table_account = client
                .get_account(test_case.lookup_table_address)
                .await
                .unwrap()
                .unwrap();

            let lookup_table = AddressLookupTable::deserialize(&table_account.data).unwrap();
            assert_eq!(lookup_table, expected_account.state);
            assert_eq!(table_account.lamports(), expected_account.lamports);
            assert_eq!(table_account.data().len(), expected_account.data_len);
        }
        Err(expected_err) => {
            assert_eq!(
                process_result.unwrap_err().unwrap(),
                TransactionError::InstructionError(0, expected_err),
            );
        }
    }
}

#[tokio::test]
async fn test_extend_lookup_table() {
    let mut context = setup_test_context().await;
    let authority = Keypair::new();
    let current_bank_slot = 1;
    let rent = context.banks_client.get_rent().await.unwrap();

    for extend_same_slot in [true, false] {
        for (num_existing_addresses, num_new_addresses, expected_result) in [
            (0, 0, Err(InstructionError::InvalidInstructionData)),
            (0, 1, Ok(())),
            (0, 10, Ok(())),
            (1, 1, Ok(())),
            (1, 10, Ok(())),
            (255, 1, Ok(())),
            (255, 2, Err(InstructionError::InvalidInstructionData)),
            (246, 10, Ok(())),
            (256, 1, Err(InstructionError::InvalidArgument)),
        ] {
            let mut lookup_table =
                new_address_lookup_table(Some(authority.pubkey()), num_existing_addresses);
            if extend_same_slot {
                lookup_table.meta.last_extended_slot = current_bank_slot;
            }

            let lookup_table_address = Pubkey::new_unique();
            let lookup_table_account =
                add_lookup_table_account(&mut context, lookup_table_address, lookup_table.clone())
                    .await;

            let mut new_addresses = Vec::with_capacity(num_new_addresses);
            new_addresses.resize_with(num_new_addresses, Pubkey::new_unique);
            let instruction = extend_lookup_table(
                lookup_table_address,
                authority.pubkey(),
                Some(context.payer.pubkey()),
                new_addresses.clone(),
            );

            let mut expected_addresses: Vec<Pubkey> = lookup_table.addresses.to_vec();
            expected_addresses.extend(new_addresses);

            let expected_result = expected_result.map(|_| {
                let expected_data_len =
                    lookup_table_account.data().len() + num_new_addresses * PUBKEY_BYTES;
                let expected_lamports = rent.minimum_balance(expected_data_len);
                let expected_lookup_table = AddressLookupTable {
                    meta: LookupTableMeta {
                        last_extended_slot: current_bank_slot,
                        last_extended_slot_start_index: if extend_same_slot {
                            0u8
                        } else {
                            num_existing_addresses as u8
                        },
                        deactivation_slot: lookup_table.meta.deactivation_slot,
                        authority: lookup_table.meta.authority,
                        _padding: 0u16,
                    },
                    addresses: Cow::Owned(expected_addresses),
                };
                ExpectedTableAccount {
                    lamports: expected_lamports,
                    data_len: expected_data_len,
                    state: expected_lookup_table,
                }
            });

            let test_case = TestCase {
                lookup_table_address,
                instruction,
                extra_signer: Some(&authority),
                expected_result,
            };

            run_test_case(&mut context, test_case).await;
        }
    }
}

#[tokio::test]
async fn test_extend_lookup_table_with_wrong_authority() {
    let mut context = setup_test_context().await;

    let authority = Keypair::new();
    let wrong_authority = Keypair::new();
    let initialized_table = new_address_lookup_table(Some(authority.pubkey()), 0);
    let lookup_table_address = Pubkey::new_unique();
    add_lookup_table_account(&mut context, lookup_table_address, initialized_table).await;

    let new_addresses = vec![Pubkey::new_unique()];
    let ix = extend_lookup_table(
        lookup_table_address,
        wrong_authority.pubkey(),
        Some(context.payer.pubkey()),
        new_addresses,
    );

    assert_ix_error(
        &mut context,
        ix,
        Some(&wrong_authority),
        InstructionError::IncorrectAuthority,
    )
    .await;
}

#[tokio::test]
async fn test_extend_lookup_table_without_signing() {
    let mut context = setup_test_context().await;

    let authority = Keypair::new();
    let initialized_table = new_address_lookup_table(Some(authority.pubkey()), 10);
    let lookup_table_address = Pubkey::new_unique();
    add_lookup_table_account(&mut context, lookup_table_address, initialized_table).await;

    let new_addresses = vec![Pubkey::new_unique()];
    let mut ix = extend_lookup_table(
        lookup_table_address,
        authority.pubkey(),
        Some(context.payer.pubkey()),
        new_addresses,
    );
    ix.accounts[1].is_signer = false;

    assert_ix_error(
        &mut context,
        ix,
        None,
        InstructionError::MissingRequiredSignature,
    )
    .await;
}

#[tokio::test]
async fn test_extend_deactivated_lookup_table() {
    let mut context = setup_test_context().await;

    let authority = Keypair::new();
    let initialized_table = {
        let mut table = new_address_lookup_table(Some(authority.pubkey()), 0);
        table.meta.deactivation_slot = 0;
        table
    };
    let lookup_table_address = Pubkey::new_unique();
    add_lookup_table_account(&mut context, lookup_table_address, initialized_table).await;

    let new_addresses = vec![Pubkey::new_unique()];
    let ix = extend_lookup_table(
        lookup_table_address,
        authority.pubkey(),
        Some(context.payer.pubkey()),
        new_addresses,
    );

    assert_ix_error(
        &mut context,
        ix,
        Some(&authority),
        InstructionError::InvalidArgument,
    )
    .await;
}

#[tokio::test]
async fn test_extend_immutable_lookup_table() {
    let mut context = setup_test_context().await;

    let authority = Keypair::new();
    let initialized_table = new_address_lookup_table(None, 1);
    let lookup_table_address = Pubkey::new_unique();
    add_lookup_table_account(&mut context, lookup_table_address, initialized_table).await;

    let new_addresses = vec![Pubkey::new_unique()];
    let ix = extend_lookup_table(
        lookup_table_address,
        authority.pubkey(),
        Some(context.payer.pubkey()),
        new_addresses,
    );

    assert_ix_error(
        &mut context,
        ix,
        Some(&authority),
        InstructionError::Immutable,
    )
    .await;
}

#[tokio::test]
async fn test_extend_lookup_table_without_payer() {
    let mut context = setup_test_context().await;

    let authority = Keypair::new();
    let initialized_table = new_address_lookup_table(Some(authority.pubkey()), 0);
    let lookup_table_address = Pubkey::new_unique();
    add_lookup_table_account(&mut context, lookup_table_address, initialized_table).await;

    let new_addresses = vec![Pubkey::new_unique()];
    let ix = extend_lookup_table(
        lookup_table_address,
        authority.pubkey(),
        None,
        new_addresses,
    );

    assert_ix_error(
        &mut context,
        ix,
        Some(&authority),
        InstructionError::NotEnoughAccountKeys,
    )
    .await;
}

#[tokio::test]
async fn test_extend_prepaid_lookup_table_without_payer() {
    let mut context = setup_test_context().await;

    let authority = Keypair::new();
    let lookup_table_address = Pubkey::new_unique();

    let expected_state = {
        // initialize lookup table
        let empty_lookup_table = new_address_lookup_table(Some(authority.pubkey()), 0);
        let mut lookup_table_account =
            add_lookup_table_account(&mut context, lookup_table_address, empty_lookup_table).await;

        // calculate required rent exempt balance for adding one address
        let mut temp_lookup_table = new_address_lookup_table(Some(authority.pubkey()), 1);
        let data = temp_lookup_table.clone().serialize_for_tests().unwrap();
        let rent = context.banks_client.get_rent().await.unwrap();
        let rent_exempt_balance = rent.minimum_balance(data.len());

        // prepay for one address
        lookup_table_account.set_lamports(rent_exempt_balance);
        context.set_account(&lookup_table_address, &lookup_table_account);

        // test will extend table in the current bank's slot
        let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
        temp_lookup_table.meta.last_extended_slot = clock.slot;

        ExpectedTableAccount {
            lamports: rent_exempt_balance,
            data_len: data.len(),
            state: temp_lookup_table,
        }
    };

    let new_addresses = expected_state.state.addresses.to_vec();
    let instruction = extend_lookup_table(
        lookup_table_address,
        authority.pubkey(),
        None,
        new_addresses,
    );

    run_test_case(
        &mut context,
        TestCase {
            lookup_table_address,
            instruction,
            extra_signer: Some(&authority),
            expected_result: Ok(expected_state),
        },
    )
    .await;
}
