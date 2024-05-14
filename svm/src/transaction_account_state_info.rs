use {
    crate::account_rent_state::RentState,
    solana_sdk::{
        account::ReadableAccount,
        message::SanitizedMessage,
        native_loader,
        rent::Rent,
        transaction::Result,
        transaction_context::{IndexOfAccount, TransactionContext},
    },
};

#[derive(PartialEq, Debug)]
pub(crate) struct TransactionAccountStateInfo {
    rent_state: Option<RentState>, // None: readonly account
}

impl TransactionAccountStateInfo {
    pub(crate) fn new(
        rent: &Rent,
        transaction_context: &TransactionContext,
        message: &SanitizedMessage,
    ) -> Vec<Self> {
        (0..message.account_keys().len())
            .map(|i| {
                let rent_state = if message.is_writable(i) {
                    let state = if let Ok(account) =
                        transaction_context.get_account_at_index(i as IndexOfAccount)
                    {
                        let account = account.borrow();

                        // Native programs appear to be RentPaying because they carry low lamport
                        // balances; however they will never be loaded as writable
                        debug_assert!(!native_loader::check_id(account.owner()));

                        Some(RentState::from_account(&account, rent))
                    } else {
                        None
                    };
                    debug_assert!(
                        state.is_some(),
                        "message and transaction context out of sync, fatal"
                    );
                    state
                } else {
                    None
                };
                Self { rent_state }
            })
            .collect()
    }

    pub(crate) fn verify_changes(
        pre_state_infos: &[Self],
        post_state_infos: &[Self],
        transaction_context: &TransactionContext,
    ) -> Result<()> {
        for (i, (pre_state_info, post_state_info)) in
            pre_state_infos.iter().zip(post_state_infos).enumerate()
        {
            RentState::check_rent_state(
                pre_state_info.rent_state.as_ref(),
                post_state_info.rent_state.as_ref(),
                transaction_context,
                i as IndexOfAccount,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use {
        crate::{
            account_rent_state::RentState,
            transaction_account_state_info::TransactionAccountStateInfo,
        },
        solana_sdk::{
            account::AccountSharedData,
            hash::Hash,
            instruction::CompiledInstruction,
            message::{LegacyMessage, Message, MessageHeader, SanitizedMessage},
            rent::Rent,
            signature::{Keypair, Signer},
            transaction::TransactionError,
            transaction_context::TransactionContext,
        },
    };

    #[test]
    fn test_new() {
        let rent = Rent::default();
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let key3 = Keypair::new();
        let key4 = Keypair::new();

        let message = Message {
            account_keys: vec![key2.pubkey(), key1.pubkey(), key4.pubkey()],
            header: MessageHeader::default(),
            instructions: vec![
                CompiledInstruction {
                    program_id_index: 1,
                    accounts: vec![0],
                    data: vec![],
                },
                CompiledInstruction {
                    program_id_index: 1,
                    accounts: vec![2],
                    data: vec![],
                },
            ],
            recent_blockhash: Hash::default(),
        };

        let legacy = LegacyMessage::new(message);
        let sanitized_message = SanitizedMessage::Legacy(legacy);

        let transaction_accounts = vec![
            (key1.pubkey(), AccountSharedData::default()),
            (key2.pubkey(), AccountSharedData::default()),
            (key3.pubkey(), AccountSharedData::default()),
        ];

        let context = TransactionContext::new(transaction_accounts, rent.clone(), 20, 20);
        let result = TransactionAccountStateInfo::new(&rent, &context, &sanitized_message);
        assert_eq!(
            result,
            vec![
                TransactionAccountStateInfo {
                    rent_state: Some(RentState::Uninitialized)
                },
                TransactionAccountStateInfo { rent_state: None },
                TransactionAccountStateInfo {
                    rent_state: Some(RentState::Uninitialized)
                }
            ]
        );
    }

    #[test]
    #[should_panic(expected = "message and transaction context out of sync, fatal")]
    fn test_new_panic() {
        let rent = Rent::default();
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let key3 = Keypair::new();
        let key4 = Keypair::new();

        let message = Message {
            account_keys: vec![key2.pubkey(), key1.pubkey(), key4.pubkey(), key3.pubkey()],
            header: MessageHeader::default(),
            instructions: vec![
                CompiledInstruction {
                    program_id_index: 1,
                    accounts: vec![0],
                    data: vec![],
                },
                CompiledInstruction {
                    program_id_index: 1,
                    accounts: vec![2],
                    data: vec![],
                },
            ],
            recent_blockhash: Hash::default(),
        };

        let legacy = LegacyMessage::new(message);
        let sanitized_message = SanitizedMessage::Legacy(legacy);

        let transaction_accounts = vec![
            (key1.pubkey(), AccountSharedData::default()),
            (key2.pubkey(), AccountSharedData::default()),
            (key3.pubkey(), AccountSharedData::default()),
        ];

        let context = TransactionContext::new(transaction_accounts, rent.clone(), 20, 20);
        let _result = TransactionAccountStateInfo::new(&rent, &context, &sanitized_message);
    }

    #[test]
    fn test_verify_changes() {
        let key1 = Keypair::new();
        let key2 = Keypair::new();
        let pre_rent_state = vec![
            TransactionAccountStateInfo {
                rent_state: Some(RentState::Uninitialized),
            },
            TransactionAccountStateInfo {
                rent_state: Some(RentState::Uninitialized),
            },
        ];
        let post_rent_state = vec![TransactionAccountStateInfo {
            rent_state: Some(RentState::Uninitialized),
        }];

        let transaction_accounts = vec![
            (key1.pubkey(), AccountSharedData::default()),
            (key2.pubkey(), AccountSharedData::default()),
        ];

        let context = TransactionContext::new(transaction_accounts, Rent::default(), 20, 20);

        let result = TransactionAccountStateInfo::verify_changes(
            &pre_rent_state,
            &post_rent_state,
            &context,
        );
        assert!(result.is_ok());

        let pre_rent_state = vec![TransactionAccountStateInfo {
            rent_state: Some(RentState::Uninitialized),
        }];
        let post_rent_state = vec![TransactionAccountStateInfo {
            rent_state: Some(RentState::RentPaying {
                data_size: 2,
                lamports: 5,
            }),
        }];

        let transaction_accounts = vec![
            (key1.pubkey(), AccountSharedData::default()),
            (key2.pubkey(), AccountSharedData::default()),
        ];

        let context = TransactionContext::new(transaction_accounts, Rent::default(), 20, 20);
        let result = TransactionAccountStateInfo::verify_changes(
            &pre_rent_state,
            &post_rent_state,
            &context,
        );
        assert_eq!(
            result.err(),
            Some(TransactionError::InsufficientFundsForRent { account_index: 0 })
        );
    }
}
