//! Functionality derived from the `SVMMessage` base functions.
//!

use {
    solana_program::program_utils::limited_deserialize,
    solana_sdk::{
        nonce::NONCED_TX_MARKER_IX_INDEX, pubkey::Pubkey, system_instruction::SystemInstruction,
        system_program,
    },
    solana_svm_transaction::svm_message::SVMMessage,
};

/// If the message uses a durable nonce, return the pubkey of the nonce account
pub fn get_durable_nonce(message: &impl SVMMessage) -> Option<&Pubkey> {
    let account_keys = message.account_keys();
    message
        .instructions_iter()
        .nth(usize::from(NONCED_TX_MARKER_IX_INDEX))
        .filter(
            |ix| match account_keys.get(usize::from(ix.program_id_index)) {
                Some(program_id) => system_program::check_id(program_id),
                _ => false,
            },
        )
        .filter(|ix| {
            matches!(
                limited_deserialize(ix.data, 4 /* serialized size of AdvanceNonceAccount */),
                Ok(SystemInstruction::AdvanceNonceAccount)
            )
        })
        .and_then(|ix| {
            ix.accounts.first().and_then(|idx| {
                let index = usize::from(*idx);
                if !message.is_writable(index) {
                    None
                } else {
                    account_keys.get(index)
                }
            })
        })
}

/// For the instruction at `index`, return an iterator over input accounts
/// that are signers.
pub fn get_ix_signers(message: &impl SVMMessage, index: usize) -> impl Iterator<Item = &Pubkey> {
    message
        .instructions_iter()
        .nth(index)
        .into_iter()
        .flat_map(|ix| {
            ix.accounts
                .iter()
                .copied()
                .map(usize::from)
                .filter(|index| message.is_signer(*index))
                .filter_map(|signer_index| message.account_keys().get(signer_index))
        })
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            hash::Hash,
            instruction::CompiledInstruction,
            message::{
                legacy,
                v0::{self, LoadedAddresses, MessageAddressTableLookup},
                MessageHeader, SanitizedMessage, SanitizedVersionedMessage, SimpleAddressLoader,
                VersionedMessage,
            },
        },
        std::collections::HashSet,
    };

    #[test]
    fn test_get_durable_nonce() {
        fn create_message_for_test(
            num_signers: u8,
            num_writable: u8,
            account_keys: Vec<Pubkey>,
            instructions: Vec<CompiledInstruction>,
            loaded_addresses: Option<LoadedAddresses>,
        ) -> SanitizedMessage {
            let header = MessageHeader {
                num_required_signatures: num_signers,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: u8::try_from(account_keys.len()).unwrap()
                    - num_writable,
            };
            let (versioned_message, loader) = match loaded_addresses {
                None => (
                    VersionedMessage::Legacy(legacy::Message {
                        header,
                        account_keys,
                        recent_blockhash: Hash::default(),
                        instructions,
                    }),
                    SimpleAddressLoader::Disabled,
                ),
                Some(loaded_addresses) => (
                    VersionedMessage::V0(v0::Message {
                        header,
                        account_keys,
                        recent_blockhash: Hash::default(),
                        instructions,
                        address_table_lookups: vec![MessageAddressTableLookup {
                            account_key: Pubkey::new_unique(),
                            writable_indexes: (0..loaded_addresses.writable.len())
                                .map(|x| x as u8)
                                .collect(),
                            readonly_indexes: (0..loaded_addresses.readonly.len())
                                .map(|x| (loaded_addresses.writable.len() + x) as u8)
                                .collect(),
                        }],
                    }),
                    SimpleAddressLoader::Enabled(loaded_addresses),
                ),
            };
            SanitizedMessage::try_new(
                SanitizedVersionedMessage::try_new(versioned_message).unwrap(),
                loader,
                &HashSet::new(),
            )
            .unwrap()
        }

        // No instructions - no nonce
        {
            let message = create_message_for_test(1, 1, vec![Pubkey::new_unique()], vec![], None);
            assert!(message.get_durable_nonce().is_none());
            assert!(get_durable_nonce(&message).is_none());
        }

        // system program id instruction - invalid
        {
            let message = create_message_for_test(
                1,
                1,
                vec![Pubkey::new_unique(), system_program::id()],
                vec![CompiledInstruction::new_from_raw_parts(1, vec![], vec![])],
                None,
            );
            assert!(message.get_durable_nonce().is_none());
            assert!(get_durable_nonce(&message).is_none());
        }

        // system program id instruction - not nonce
        {
            let message = create_message_for_test(
                1,
                1,
                vec![Pubkey::new_unique(), system_program::id()],
                vec![CompiledInstruction::new(
                    1,
                    &SystemInstruction::Transfer { lamports: 1 },
                    vec![0, 0],
                )],
                None,
            );
            assert!(message.get_durable_nonce().is_none());
            assert!(get_durable_nonce(&message).is_none());
        }

        // system program id - nonce instruction (no accounts)
        {
            let message = create_message_for_test(
                1,
                1,
                vec![Pubkey::new_unique(), system_program::id()],
                vec![CompiledInstruction::new(
                    1,
                    &SystemInstruction::AdvanceNonceAccount,
                    vec![],
                )],
                None,
            );
            assert!(message.get_durable_nonce().is_none());
            assert!(get_durable_nonce(&message).is_none());
        }

        // system program id - nonce instruction (non-fee-payer, non-writable)
        {
            let payer = Pubkey::new_unique();
            let nonce = Pubkey::new_unique();
            let message = create_message_for_test(
                1,
                1,
                vec![payer, nonce, system_program::id()],
                vec![CompiledInstruction::new(
                    1,
                    &SystemInstruction::AdvanceNonceAccount,
                    vec![1],
                )],
                None,
            );
            assert!(message.get_durable_nonce().is_none());
            assert!(get_durable_nonce(&message).is_none());
        }

        // system program id - nonce instruction fee-payer
        {
            let payer_nonce = Pubkey::new_unique();
            let message = create_message_for_test(
                1,
                1,
                vec![payer_nonce, system_program::id()],
                vec![CompiledInstruction::new(
                    1,
                    &SystemInstruction::AdvanceNonceAccount,
                    vec![0],
                )],
                None,
            );
            assert_eq!(message.get_durable_nonce(), Some(&payer_nonce));
            assert_eq!(get_durable_nonce(&message), Some(&payer_nonce));
        }

        // system program id - nonce instruction w/ trailing bytes fee-payer
        {
            let payer_nonce = Pubkey::new_unique();
            let mut instruction_bytes =
                bincode::serialize(&SystemInstruction::AdvanceNonceAccount).unwrap();
            instruction_bytes.push(0); // add a trailing byte
            let message = create_message_for_test(
                1,
                1,
                vec![payer_nonce, system_program::id()],
                vec![CompiledInstruction::new_from_raw_parts(
                    1,
                    instruction_bytes,
                    vec![0],
                )],
                None,
            );
            assert_eq!(message.get_durable_nonce(), Some(&payer_nonce));
            assert_eq!(get_durable_nonce(&message), Some(&payer_nonce));
        }

        // system program id - nonce instruction (non-fee-payer)
        {
            let payer = Pubkey::new_unique();
            let nonce = Pubkey::new_unique();
            let message = create_message_for_test(
                1,
                2,
                vec![payer, nonce, system_program::id()],
                vec![CompiledInstruction::new(
                    2,
                    &SystemInstruction::AdvanceNonceAccount,
                    vec![1],
                )],
                None,
            );
            assert_eq!(message.get_durable_nonce(), Some(&nonce));
            assert_eq!(get_durable_nonce(&message), Some(&nonce));
        }

        // system program id - nonce instruction (non-fee-payer, multiple accounts)
        {
            let payer = Pubkey::new_unique();
            let other = Pubkey::new_unique();
            let nonce = Pubkey::new_unique();
            let message = create_message_for_test(
                1,
                3,
                vec![payer, other, nonce, system_program::id()],
                vec![CompiledInstruction::new(
                    3,
                    &SystemInstruction::AdvanceNonceAccount,
                    vec![2, 1, 0],
                )],
                None,
            );
            assert_eq!(message.get_durable_nonce(), Some(&nonce));
            assert_eq!(get_durable_nonce(&message), Some(&nonce));
        }

        // system program id - nonce instruction (non-fee-payer, loaded account)
        {
            let payer = Pubkey::new_unique();
            let nonce = Pubkey::new_unique();
            let message = create_message_for_test(
                1,
                1,
                vec![payer, system_program::id()],
                vec![CompiledInstruction::new(
                    1,
                    &SystemInstruction::AdvanceNonceAccount,
                    vec![2, 0, 1],
                )],
                Some(LoadedAddresses {
                    writable: vec![nonce],
                    readonly: vec![],
                }),
            );
            assert_eq!(message.get_durable_nonce(), Some(&nonce));
            assert_eq!(get_durable_nonce(&message), Some(&nonce));
        }
    }

    #[test]
    fn test_get_ix_signers() {
        let signer0 = Pubkey::new_unique();
        let signer1 = Pubkey::new_unique();
        let non_signer = Pubkey::new_unique();
        let loader_key = Pubkey::new_unique();
        let instructions = vec![
            CompiledInstruction::new(3, &(), vec![2, 0]),
            CompiledInstruction::new(3, &(), vec![0, 1]),
            CompiledInstruction::new(3, &(), vec![0, 0]),
        ];

        let message = SanitizedMessage::try_from_legacy_message(
            legacy::Message::new_with_compiled_instructions(
                2,
                1,
                2,
                vec![signer0, signer1, non_signer, loader_key],
                Hash::default(),
                instructions,
            ),
            &HashSet::default(),
        )
        .unwrap();

        assert_eq!(
            get_ix_signers(&message, 0).collect::<HashSet<_>>(),
            HashSet::from_iter([&signer0])
        );
        assert_eq!(
            get_ix_signers(&message, 1).collect::<HashSet<_>>(),
            HashSet::from_iter([&signer0, &signer1])
        );
        assert_eq!(
            get_ix_signers(&message, 2).collect::<HashSet<_>>(),
            HashSet::from_iter([&signer0])
        );
        assert_eq!(
            get_ix_signers(&message, 3).collect::<HashSet<_>>(),
            HashSet::default()
        );
    }
}
