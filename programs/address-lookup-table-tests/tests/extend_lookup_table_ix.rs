use {
    assert_matches::assert_matches,
    common::{add_lookup_table_account, new_address_lookup_table, setup_test_context},
    solana_address_lookup_table_program::{
        instruction::extend_lookup_table,
        state::{AddressLookupTable, LookupTableMeta},
    },
    solana_program_test::*,
    solana_sdk::{
        account::ReadableAccount,
        instruction::Instruction,
        instruction::InstructionError,
        pubkey::{Pubkey, PUBKEY_BYTES},
        signature::{Keypair, Signer},
        transaction::{Transaction, TransactionError},
    },
    std::borrow::Cow,
    std::result::Result,
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
                context.payer.pubkey(),
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
                        derivation_slot: lookup_table.meta.derivation_slot,
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
async fn test_extend_addresses_authority_errors() {
    let mut context = setup_test_context().await;
    let authority = Keypair::new();

    for (existing_authority, ix_authority, use_signer, expected_err) in [
        (
            Some(authority.pubkey()),
            Keypair::new(),
            true,
            InstructionError::IncorrectAuthority,
        ),
        (
            Some(authority.pubkey()),
            authority,
            false,
            InstructionError::MissingRequiredSignature,
        ),
        (None, Keypair::new(), true, InstructionError::Immutable),
    ] {
        let lookup_table = new_address_lookup_table(existing_authority, 0);
        let lookup_table_address = Pubkey::new_unique();
        let _ = add_lookup_table_account(&mut context, lookup_table_address, lookup_table.clone())
            .await;

        let num_new_addresses = 1;
        let mut new_addresses = Vec::with_capacity(num_new_addresses);
        new_addresses.resize_with(num_new_addresses, Pubkey::new_unique);
        let mut instruction = extend_lookup_table(
            lookup_table_address,
            ix_authority.pubkey(),
            context.payer.pubkey(),
            new_addresses.clone(),
        );
        if !use_signer {
            instruction.accounts[1].is_signer = false;
        }

        let mut expected_addresses: Vec<Pubkey> = lookup_table.addresses.to_vec();
        expected_addresses.extend(new_addresses);

        let extra_signer = if use_signer {
            Some(&ix_authority)
        } else {
            None
        };

        let test_case = TestCase {
            lookup_table_address,
            instruction,
            extra_signer,
            expected_result: Err(expected_err),
        };

        run_test_case(&mut context, test_case).await;
    }
}
