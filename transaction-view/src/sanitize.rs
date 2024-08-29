use crate::{
    result::{Result, TransactionViewError},
    transaction_data::TransactionData,
    transaction_view::UnsanitizedTransactionView,
};

pub(crate) fn sanitize(view: &UnsanitizedTransactionView<impl TransactionData>) -> Result<()> {
    sanitize_signatures(view)?;
    sanitize_account_access(view)?;
    sanitize_instructions(view)?;
    sanitize_address_table_lookups(view)
}

fn sanitize_signatures(view: &UnsanitizedTransactionView<impl TransactionData>) -> Result<()> {
    // Check the required number of signatures matches the number of signatures.
    if view.num_signatures() != view.num_required_signatures() {
        return Err(TransactionViewError::SanitizeError);
    }

    // Each signature is associated with a unique static public key.
    // Check that there are at least as many static account keys as signatures.
    if view.num_static_account_keys() < view.num_signatures() {
        return Err(TransactionViewError::SanitizeError);
    }

    Ok(())
}

fn sanitize_account_access(view: &UnsanitizedTransactionView<impl TransactionData>) -> Result<()> {
    // Check there is no overlap of signing area and readonly non-signing area.
    // We have already checked that `num_required_signatures` is less than or equal to `num_static_account_keys`,
    // so it is safe to use wrapping arithmetic.
    if view.num_readonly_unsigned_accounts()
        > view
            .num_static_account_keys()
            .wrapping_sub(view.num_required_signatures())
    {
        return Err(TransactionViewError::SanitizeError);
    }

    // Check there is at least 1 writable fee-payer account.
    if view.num_readonly_signed_accounts() >= view.num_required_signatures() {
        return Err(TransactionViewError::SanitizeError);
    }

    // Check there are not more than 256 accounts.
    if total_number_of_accounts(view) > 256 {
        return Err(TransactionViewError::SanitizeError);
    }

    Ok(())
}

fn sanitize_instructions(view: &UnsanitizedTransactionView<impl TransactionData>) -> Result<()> {
    // already verified there is at least one static account.
    let max_program_id_index = view.num_static_account_keys().wrapping_sub(1);
    // verified that there are no more than 256 accounts in `sanitize_account_access`
    let max_account_index = total_number_of_accounts(view).wrapping_sub(1) as u8;

    for instruction in view.instructions_iter() {
        // Check that program indexes are static account keys.
        if instruction.program_id_index > max_program_id_index {
            return Err(TransactionViewError::SanitizeError);
        }

        // Check that the program index is not the fee-payer.
        if instruction.program_id_index == 0 {
            return Err(TransactionViewError::SanitizeError);
        }

        // Check that all account indexes are valid.
        for account_index in instruction.accounts.iter().copied() {
            if account_index > max_account_index {
                return Err(TransactionViewError::SanitizeError);
            }
        }
    }

    Ok(())
}

fn sanitize_address_table_lookups(
    view: &UnsanitizedTransactionView<impl TransactionData>,
) -> Result<()> {
    for address_table_lookup in view.address_table_lookup_iter() {
        // Check that there is at least one account lookup.
        if address_table_lookup.writable_indexes.is_empty()
            && address_table_lookup.readonly_indexes.is_empty()
        {
            return Err(TransactionViewError::SanitizeError);
        }
    }

    Ok(())
}

fn total_number_of_accounts(view: &UnsanitizedTransactionView<impl TransactionData>) -> u16 {
    u16::from(view.num_static_account_keys())
        .saturating_add(view.total_writable_lookup_accounts())
        .saturating_add(view.total_readonly_lookup_accounts())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::transaction_view::TransactionView,
        solana_sdk::{
            hash::Hash,
            instruction::CompiledInstruction,
            message::{
                v0::{self, MessageAddressTableLookup},
                Message, MessageHeader, VersionedMessage,
            },
            pubkey::Pubkey,
            signature::Signature,
            system_instruction,
            transaction::VersionedTransaction,
        },
    };

    fn create_legacy_transaction(
        num_signatures: u8,
        header: MessageHeader,
        account_keys: Vec<Pubkey>,
        instructions: Vec<CompiledInstruction>,
    ) -> VersionedTransaction {
        VersionedTransaction {
            signatures: vec![Signature::default(); num_signatures as usize],
            message: VersionedMessage::Legacy(Message {
                header,
                account_keys,
                recent_blockhash: Hash::default(),
                instructions,
            }),
        }
    }

    fn create_v0_transaction(
        num_signatures: u8,
        header: MessageHeader,
        account_keys: Vec<Pubkey>,
        instructions: Vec<CompiledInstruction>,
        address_table_lookups: Vec<MessageAddressTableLookup>,
    ) -> VersionedTransaction {
        VersionedTransaction {
            signatures: vec![Signature::default(); num_signatures as usize],
            message: VersionedMessage::V0(v0::Message {
                header,
                account_keys,
                recent_blockhash: Hash::default(),
                instructions,
                address_table_lookups,
            }),
        }
    }

    fn multiple_transfers() -> VersionedTransaction {
        let payer = Pubkey::new_unique();
        VersionedTransaction {
            signatures: vec![Signature::default()], // 1 signature to be valid.
            message: VersionedMessage::Legacy(Message::new(
                &[
                    system_instruction::transfer(&payer, &Pubkey::new_unique(), 1),
                    system_instruction::transfer(&payer, &Pubkey::new_unique(), 1),
                ],
                Some(&payer),
            )),
        }
    }

    #[test]
    fn test_sanitize_multiple_transfers() {
        let transaction = multiple_transfers();
        let data = bincode::serialize(&transaction).unwrap();
        let view = TransactionView::try_new_unsanitized(data.as_ref()).unwrap();
        assert!(view.sanitize().is_ok());
    }

    #[test]
    fn test_sanitize_signatures() {
        // Too few signatures.
        {
            let transaction = create_legacy_transaction(
                1,
                MessageHeader {
                    num_required_signatures: 2,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                (0..3).map(|_| Pubkey::new_unique()).collect(),
                vec![],
            );
            let data = bincode::serialize(&transaction).unwrap();
            let view = TransactionView::try_new_unsanitized(data.as_ref()).unwrap();
            assert_eq!(
                sanitize_signatures(&view),
                Err(TransactionViewError::SanitizeError)
            );
        }

        // Too many signatures.
        {
            let transaction = create_legacy_transaction(
                2,
                MessageHeader {
                    num_required_signatures: 1,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                (0..3).map(|_| Pubkey::new_unique()).collect(),
                vec![],
            );
            let data = bincode::serialize(&transaction).unwrap();
            let view = TransactionView::try_new_unsanitized(data.as_ref()).unwrap();
            assert_eq!(
                sanitize_signatures(&view),
                Err(TransactionViewError::SanitizeError)
            );
        }

        // Not enough static accounts.
        {
            let transaction = create_legacy_transaction(
                2,
                MessageHeader {
                    num_required_signatures: 2,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                (0..1).map(|_| Pubkey::new_unique()).collect(),
                vec![],
            );
            let data = bincode::serialize(&transaction).unwrap();
            let view = TransactionView::try_new_unsanitized(data.as_ref()).unwrap();
            assert_eq!(
                sanitize_signatures(&view),
                Err(TransactionViewError::SanitizeError)
            );
        }

        // Not enough static accounts - with look up accounts
        {
            let transaction = create_v0_transaction(
                2,
                MessageHeader {
                    num_required_signatures: 2,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                (0..1).map(|_| Pubkey::new_unique()).collect(),
                vec![],
                vec![MessageAddressTableLookup {
                    account_key: Pubkey::new_unique(),
                    writable_indexes: vec![0, 1, 2, 3, 4, 5],
                    readonly_indexes: vec![6, 7, 8],
                }],
            );
            let data = bincode::serialize(&transaction).unwrap();
            let view = TransactionView::try_new_unsanitized(data.as_ref()).unwrap();
            assert_eq!(
                sanitize_signatures(&view),
                Err(TransactionViewError::SanitizeError)
            );
        }
    }

    #[test]
    fn test_sanitize_account_access() {
        // Overlap of signing and readonly non-signing accounts.
        {
            let transaction = create_legacy_transaction(
                1,
                MessageHeader {
                    num_required_signatures: 1,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 2,
                },
                (0..2).map(|_| Pubkey::new_unique()).collect(),
                vec![],
            );
            let data = bincode::serialize(&transaction).unwrap();
            let view = TransactionView::try_new_unsanitized(data.as_ref()).unwrap();
            assert_eq!(
                sanitize_account_access(&view),
                Err(TransactionViewError::SanitizeError)
            );
        }

        // Not enough writable accounts.
        {
            let transaction = create_legacy_transaction(
                1,
                MessageHeader {
                    num_required_signatures: 1,
                    num_readonly_signed_accounts: 1,
                    num_readonly_unsigned_accounts: 0,
                },
                (0..2).map(|_| Pubkey::new_unique()).collect(),
                vec![],
            );
            let data = bincode::serialize(&transaction).unwrap();
            let view = TransactionView::try_new_unsanitized(data.as_ref()).unwrap();
            assert_eq!(
                sanitize_account_access(&view),
                Err(TransactionViewError::SanitizeError)
            );
        }

        // Too many accounts.
        {
            let transaction = create_v0_transaction(
                2,
                MessageHeader {
                    num_required_signatures: 2,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                (0..1).map(|_| Pubkey::new_unique()).collect(),
                vec![],
                vec![
                    MessageAddressTableLookup {
                        account_key: Pubkey::new_unique(),
                        writable_indexes: (0..100).collect(),
                        readonly_indexes: (100..200).collect(),
                    },
                    MessageAddressTableLookup {
                        account_key: Pubkey::new_unique(),
                        writable_indexes: (100..200).collect(),
                        readonly_indexes: (0..100).collect(),
                    },
                ],
            );
            let data = bincode::serialize(&transaction).unwrap();
            let view = TransactionView::try_new_unsanitized(data.as_ref()).unwrap();
            assert_eq!(
                sanitize_account_access(&view),
                Err(TransactionViewError::SanitizeError)
            );
        }
    }

    #[test]
    fn test_sanitize_instructions() {
        let num_signatures = 1;
        let header = MessageHeader {
            num_required_signatures: 1,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 1,
        };
        let account_keys = vec![
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];
        let valid_instructions = vec![
            CompiledInstruction {
                program_id_index: 1,
                accounts: vec![0, 1],
                data: vec![1, 2, 3],
            },
            CompiledInstruction {
                program_id_index: 2,
                accounts: vec![1, 0],
                data: vec![3, 2, 1, 4],
            },
        ];
        let atls = vec![MessageAddressTableLookup {
            account_key: Pubkey::new_unique(),
            writable_indexes: vec![0, 1],
            readonly_indexes: vec![2],
        }];

        // Verify that the unmodified transaction(s) are valid/sanitized.
        {
            let transaction = create_legacy_transaction(
                num_signatures,
                header,
                account_keys.clone(),
                valid_instructions.clone(),
            );
            let data = bincode::serialize(&transaction).unwrap();
            let view = TransactionView::try_new_unsanitized(data.as_ref()).unwrap();
            assert!(sanitize_instructions(&view).is_ok());

            let transaction = create_v0_transaction(
                num_signatures,
                header,
                account_keys.clone(),
                valid_instructions.clone(),
                atls.clone(),
            );
            let data = bincode::serialize(&transaction).unwrap();
            let view = TransactionView::try_new_unsanitized(data.as_ref()).unwrap();
            assert!(sanitize_instructions(&view).is_ok());
        }

        for instruction_index in 0..valid_instructions.len() {
            // Invalid program index.
            {
                let mut instructions = valid_instructions.clone();
                instructions[instruction_index].program_id_index = account_keys.len() as u8;
                let transaction = create_legacy_transaction(
                    num_signatures,
                    header,
                    account_keys.clone(),
                    instructions,
                );
                let data = bincode::serialize(&transaction).unwrap();
                let view = TransactionView::try_new_unsanitized(data.as_ref()).unwrap();
                assert_eq!(
                    sanitize_instructions(&view),
                    Err(TransactionViewError::SanitizeError)
                );
            }

            // Invalid program index with lookups.
            {
                let mut instructions = valid_instructions.clone();
                instructions[instruction_index].program_id_index = account_keys.len() as u8;
                let transaction = create_v0_transaction(
                    num_signatures,
                    header,
                    account_keys.clone(),
                    instructions,
                    atls.clone(),
                );
                let data = bincode::serialize(&transaction).unwrap();
                let view = TransactionView::try_new_unsanitized(data.as_ref()).unwrap();
                assert_eq!(
                    sanitize_instructions(&view),
                    Err(TransactionViewError::SanitizeError)
                );
            }

            // Program index is fee-payer.
            {
                let mut instructions = valid_instructions.clone();
                instructions[instruction_index].program_id_index = 0;
                let transaction = create_legacy_transaction(
                    num_signatures,
                    header,
                    account_keys.clone(),
                    instructions,
                );
                let data = bincode::serialize(&transaction).unwrap();
                let view = TransactionView::try_new_unsanitized(data.as_ref()).unwrap();
                assert_eq!(
                    sanitize_instructions(&view),
                    Err(TransactionViewError::SanitizeError)
                );
            }

            // Invalid account index.
            {
                let mut instructions = valid_instructions.clone();
                instructions[instruction_index]
                    .accounts
                    .push(account_keys.len() as u8);
                let transaction = create_legacy_transaction(
                    num_signatures,
                    header,
                    account_keys.clone(),
                    instructions,
                );
                let data = bincode::serialize(&transaction).unwrap();
                let view = TransactionView::try_new_unsanitized(data.as_ref()).unwrap();
                assert_eq!(
                    sanitize_instructions(&view),
                    Err(TransactionViewError::SanitizeError)
                );
            }

            // Invalid account index with v0.
            {
                let num_lookup_accounts =
                    atls[0].writable_indexes.len() + atls[0].readonly_indexes.len();
                let total_accounts = (account_keys.len() + num_lookup_accounts) as u8;
                let mut instructions = valid_instructions.clone();
                instructions[instruction_index]
                    .accounts
                    .push(total_accounts);
                let transaction = create_v0_transaction(
                    num_signatures,
                    header,
                    account_keys.clone(),
                    instructions,
                    atls.clone(),
                );
                let data = bincode::serialize(&transaction).unwrap();
                let view = TransactionView::try_new_unsanitized(data.as_ref()).unwrap();
                assert_eq!(
                    sanitize_instructions(&view),
                    Err(TransactionViewError::SanitizeError)
                );
            }
        }
    }

    #[test]
    fn test_sanitize_address_table_lookups() {
        fn create_transaction(empty_index: usize) -> VersionedTransaction {
            let payer = Pubkey::new_unique();
            let mut address_table_lookups = vec![
                MessageAddressTableLookup {
                    account_key: Pubkey::new_unique(),
                    writable_indexes: vec![0, 1],
                    readonly_indexes: vec![],
                },
                MessageAddressTableLookup {
                    account_key: Pubkey::new_unique(),
                    writable_indexes: vec![0, 1],
                    readonly_indexes: vec![],
                },
            ];
            address_table_lookups[empty_index].writable_indexes.clear();
            create_v0_transaction(
                1,
                MessageHeader {
                    num_required_signatures: 1,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                vec![payer],
                vec![],
                address_table_lookups,
            )
        }

        for empty_index in 0..2 {
            let transaction = create_transaction(empty_index);
            assert_eq!(
                transaction.message.address_table_lookups().unwrap().len(),
                2
            );
            let data = bincode::serialize(&transaction).unwrap();
            let view = TransactionView::try_new_unsanitized(data.as_ref()).unwrap();
            assert_eq!(
                sanitize_address_table_lookups(&view),
                Err(TransactionViewError::SanitizeError)
            );
        }
    }
}
