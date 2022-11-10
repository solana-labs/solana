//! A future Solana message format.
//!
//! This crate defines two versions of `Message` in their own modules:
//! [`legacy`] and [`v0`]. `legacy` is the current version as of Solana 1.10.0.
//! `v0` is a [future message format] that encodes more account keys into a
//! transaction than the legacy format.
//!
//! [`legacy`]: crate::message::legacy
//! [`v0`]: crate::message::v0
//! [future message format]: https://docs.solana.com/proposals/versioned-transactions

use crate::{
    address_lookup_table_account::AddressLookupTableAccount,
    bpf_loader_upgradeable,
    hash::Hash,
    instruction::{CompiledInstruction, Instruction},
    message::{
        compiled_keys::CompileError, legacy::is_builtin_key_or_sysvar, AccountKeys, CompiledKeys,
        MessageHeader, MESSAGE_VERSION_PREFIX,
    },
    pubkey::Pubkey,
    sanitize::SanitizeError,
    short_vec,
};
pub use loaded::*;

mod loaded;

/// Address table lookups describe an on-chain address lookup table to use
/// for loading more readonly and writable accounts in a single tx.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, AbiExample)]
#[serde(rename_all = "camelCase")]
pub struct MessageAddressTableLookup {
    /// Address lookup table account key
    pub account_key: Pubkey,
    /// List of indexes used to load writable account addresses
    #[serde(with = "short_vec")]
    pub writable_indexes: Vec<u8>,
    /// List of indexes used to load readonly account addresses
    #[serde(with = "short_vec")]
    pub readonly_indexes: Vec<u8>,
}

/// A Solana transaction message (v0).
///
/// This message format supports succinct account loading with
/// on-chain address lookup tables.
///
/// See the [`message`] module documentation for further description.
///
/// [`message`]: crate::message
#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, AbiExample)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// The message header, identifying signed and read-only `account_keys`.
    /// Header values only describe static `account_keys`, they do not describe
    /// any additional account keys loaded via address table lookups.
    pub header: MessageHeader,

    /// List of accounts loaded by this transaction.
    #[serde(with = "short_vec")]
    pub account_keys: Vec<Pubkey>,

    /// The blockhash of a recent block.
    pub recent_blockhash: Hash,

    /// Instructions that invoke a designated program, are executed in sequence,
    /// and committed in one atomic transaction if all succeed.
    ///
    /// # Notes
    ///
    /// Program indexes must index into the list of message `account_keys` because
    /// program id's cannot be dynamically loaded from a lookup table.
    ///
    /// Account indexes must index into the list of addresses
    /// constructed from the concatenation of three key lists:
    ///   1) message `account_keys`
    ///   2) ordered list of keys loaded from `writable` lookup table indexes
    ///   3) ordered list of keys loaded from `readable` lookup table indexes
    #[serde(with = "short_vec")]
    pub instructions: Vec<CompiledInstruction>,

    /// List of address table lookups used to load additional accounts
    /// for this transaction.
    #[serde(with = "short_vec")]
    pub address_table_lookups: Vec<MessageAddressTableLookup>,
}

impl Message {
    /// Sanitize message fields and compiled instruction indexes
    pub fn sanitize(&self, reject_dynamic_program_ids: bool) -> Result<(), SanitizeError> {
        let num_static_account_keys = self.account_keys.len();
        if usize::from(self.header.num_required_signatures)
            .saturating_add(usize::from(self.header.num_readonly_unsigned_accounts))
            > num_static_account_keys
        {
            return Err(SanitizeError::IndexOutOfBounds);
        }

        // there should be at least 1 RW fee-payer account.
        if self.header.num_readonly_signed_accounts >= self.header.num_required_signatures {
            return Err(SanitizeError::InvalidValue);
        }

        let num_dynamic_account_keys = {
            let mut total_lookup_keys: usize = 0;
            for lookup in &self.address_table_lookups {
                let num_lookup_indexes = lookup
                    .writable_indexes
                    .len()
                    .saturating_add(lookup.readonly_indexes.len());

                // each lookup table must be used to load at least one account
                if num_lookup_indexes == 0 {
                    return Err(SanitizeError::InvalidValue);
                }

                total_lookup_keys = total_lookup_keys.saturating_add(num_lookup_indexes);
            }
            total_lookup_keys
        };

        // this is redundant with the above sanitization checks which require that:
        // 1) the header describes at least 1 RW account
        // 2) the header doesn't describe more account keys than the number of account keys
        if num_static_account_keys == 0 {
            return Err(SanitizeError::InvalidValue);
        }

        // the combined number of static and dynamic account keys must be <= 256
        // since account indices are encoded as `u8`
        let total_account_keys = num_static_account_keys.saturating_add(num_dynamic_account_keys);
        if total_account_keys > 256 {
            return Err(SanitizeError::IndexOutOfBounds);
        }

        // `expect` is safe because of earlier check that
        // `num_static_account_keys` is non-zero
        let max_account_ix = total_account_keys
            .checked_sub(1)
            .expect("message doesn't contain any account keys");

        // switch to rejecting program ids loaded from lookup tables so that
        // static analysis on program instructions can be performed without
        // loading on-chain data from a bank
        let max_program_id_ix = if reject_dynamic_program_ids {
            // `expect` is safe because of earlier check that
            // `num_static_account_keys` is non-zero
            num_static_account_keys
                .checked_sub(1)
                .expect("message doesn't contain any static account keys")
        } else {
            max_account_ix
        };

        for ci in &self.instructions {
            if usize::from(ci.program_id_index) > max_program_id_ix {
                return Err(SanitizeError::IndexOutOfBounds);
            }
            // A program cannot be a payer.
            if ci.program_id_index == 0 {
                return Err(SanitizeError::IndexOutOfBounds);
            }
            for ai in &ci.accounts {
                if usize::from(*ai) > max_account_ix {
                    return Err(SanitizeError::IndexOutOfBounds);
                }
            }
        }

        Ok(())
    }
}

impl Message {
    /// Create a signable transaction message from a `payer` public key,
    /// `recent_blockhash`, list of `instructions`, and a list of
    /// `address_lookup_table_accounts`.
    ///
    /// # Examples
    ///
    /// This example uses the [`solana_address_lookup_table_program`], [`solana_rpc_client`], [`solana_sdk`], and [`anyhow`] crates.
    ///
    /// [`solana_address_lookup_table_program`]: https://docs.rs/solana-address-lookup-table-program
    /// [`solana_rpc_client`]: https://docs.rs/solana-rpc-client
    /// [`solana_sdk`]: https://docs.rs/solana-sdk
    /// [`anyhow`]: https://docs.rs/anyhow
    ///
    /// ```
    /// # use solana_program::example_mocks::{
    /// #     solana_address_lookup_table_program,
    /// #     solana_rpc_client,
    /// #     solana_sdk,
    /// # };
    /// # use std::borrow::Cow;
    /// # use solana_sdk::account::Account;
    /// use anyhow::Result;
    /// use solana_address_lookup_table_program::state::AddressLookupTable;
    /// use solana_rpc_client::rpc_client::RpcClient;
    /// use solana_sdk::{
    ///      address_lookup_table_account::AddressLookupTableAccount,
    ///      instruction::{AccountMeta, Instruction},
    ///      message::{VersionedMessage, v0},
    ///      pubkey::Pubkey,
    ///      signature::{Keypair, Signer},
    ///      transaction::VersionedTransaction,
    /// };
    ///
    /// fn create_tx_with_address_table_lookup(
    ///     client: &RpcClient,
    ///     instruction: Instruction,
    ///     address_lookup_table_key: Pubkey,
    ///     payer: &Keypair,
    /// ) -> Result<VersionedTransaction> {
    ///     # client.set_get_account_response(address_lookup_table_key, Account {
    ///     #   lamports: 1,
    ///     #   data: AddressLookupTable {
    ///     #     addresses: Cow::Owned(instruction.accounts.iter().map(|meta| meta.pubkey).collect()),
    ///     #   }.serialize_for_tests().unwrap(),
    ///     #   owner: solana_address_lookup_table_program::ID,
    ///     #   executable: false,
    ///     #   rent_epoch: 1,
    ///     # });
    ///     let raw_account = client.get_account(&address_lookup_table_key)?;
    ///     let address_lookup_table = AddressLookupTable::deserialize(&raw_account.data)?;
    ///     let address_lookup_table_account = AddressLookupTableAccount {
    ///         key: address_lookup_table_key,
    ///         addresses: address_lookup_table.addresses.to_vec(),
    ///     };
    ///
    ///     let blockhash = client.get_latest_blockhash()?;
    ///     let tx = VersionedTransaction::try_new(
    ///         VersionedMessage::V0(v0::Message::try_compile(
    ///             &payer.pubkey(),
    ///             &[instruction],
    ///             &[address_lookup_table_account],
    ///             blockhash,
    ///         )?),
    ///         &[payer],
    ///     )?;
    ///
    ///     # assert!(tx.message.address_table_lookups().unwrap().len() > 0);
    ///     Ok(tx)
    /// }
    /// #
    /// # let client = RpcClient::new(String::new());
    /// # let payer = Keypair::new();
    /// # let address_lookup_table_key = Pubkey::new_unique();
    /// # let instruction = Instruction::new_with_bincode(Pubkey::new_unique(), &(), vec![
    /// #   AccountMeta::new(Pubkey::new_unique(), false),
    /// # ]);
    /// # create_tx_with_address_table_lookup(&client, instruction, address_lookup_table_key, &payer)?;
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn try_compile(
        payer: &Pubkey,
        instructions: &[Instruction],
        address_lookup_table_accounts: &[AddressLookupTableAccount],
        recent_blockhash: Hash,
    ) -> Result<Self, CompileError> {
        let mut compiled_keys = CompiledKeys::compile(instructions, Some(*payer));

        let mut address_table_lookups = Vec::with_capacity(address_lookup_table_accounts.len());
        let mut loaded_addresses_list = Vec::with_capacity(address_lookup_table_accounts.len());
        for lookup_table_account in address_lookup_table_accounts {
            if let Some((lookup, loaded_addresses)) =
                compiled_keys.try_extract_table_lookup(lookup_table_account)?
            {
                address_table_lookups.push(lookup);
                loaded_addresses_list.push(loaded_addresses);
            }
        }

        let (header, static_keys) = compiled_keys.try_into_message_components()?;
        let dynamic_keys = loaded_addresses_list.into_iter().collect();
        let account_keys = AccountKeys::new(&static_keys, Some(&dynamic_keys));
        let instructions = account_keys.try_compile_instructions(instructions)?;

        Ok(Self {
            header,
            account_keys: static_keys,
            recent_blockhash,
            instructions,
            address_table_lookups,
        })
    }

    /// Serialize this message with a version #0 prefix using bincode encoding.
    pub fn serialize(&self) -> Vec<u8> {
        bincode::serialize(&(MESSAGE_VERSION_PREFIX, self)).unwrap()
    }

    /// Returns true if the account at the specified index is called as a program by an instruction
    pub fn is_key_called_as_program(&self, key_index: usize) -> bool {
        if let Ok(key_index) = u8::try_from(key_index) {
            self.instructions
                .iter()
                .any(|ix| ix.program_id_index == key_index)
        } else {
            false
        }
    }

    /// Returns true if the account at the specified index was requested to be
    /// writable.  This method should not be used directly.
    fn is_writable_index(&self, key_index: usize) -> bool {
        let header = &self.header;
        let num_account_keys = self.account_keys.len();
        let num_signed_accounts = usize::from(header.num_required_signatures);
        if key_index >= num_account_keys {
            let loaded_addresses_index = key_index.saturating_sub(num_account_keys);
            let num_writable_dynamic_addresses = self
                .address_table_lookups
                .iter()
                .map(|lookup| lookup.writable_indexes.len())
                .sum();
            loaded_addresses_index < num_writable_dynamic_addresses
        } else if key_index >= num_signed_accounts {
            let num_unsigned_accounts = num_account_keys.saturating_sub(num_signed_accounts);
            let num_writable_unsigned_accounts = num_unsigned_accounts
                .saturating_sub(usize::from(header.num_readonly_unsigned_accounts));
            let unsigned_account_index = key_index.saturating_sub(num_signed_accounts);
            unsigned_account_index < num_writable_unsigned_accounts
        } else {
            let num_writable_signed_accounts = num_signed_accounts
                .saturating_sub(usize::from(header.num_readonly_signed_accounts));
            key_index < num_writable_signed_accounts
        }
    }

    /// Returns true if any static account key is the bpf upgradeable loader
    fn is_upgradeable_loader_in_static_keys(&self) -> bool {
        self.account_keys
            .iter()
            .any(|&key| key == bpf_loader_upgradeable::id())
    }

    /// Returns true if the account at the specified index was requested as writable.
    /// Before loading addresses, we can't demote write locks for dynamically loaded
    /// addresses so this should not be used by the runtime.
    pub fn is_maybe_writable(&self, key_index: usize) -> bool {
        self.is_writable_index(key_index)
            && !{
                // demote reserved ids
                self.account_keys
                    .get(key_index)
                    .map(is_builtin_key_or_sysvar)
                    .unwrap_or_default()
            }
            && !{
                // demote program ids
                self.is_key_called_as_program(key_index)
                    && !self.is_upgradeable_loader_in_static_keys()
            }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{instruction::AccountMeta, message::VersionedMessage},
    };

    #[test]
    fn test_sanitize() {
        assert!(Message {
            header: MessageHeader {
                num_required_signatures: 1,
                ..MessageHeader::default()
            },
            account_keys: vec![Pubkey::new_unique()],
            ..Message::default()
        }
        .sanitize(
            true, // require_static_program_ids
        )
        .is_ok());
    }

    #[test]
    fn test_sanitize_with_instruction() {
        assert!(Message {
            header: MessageHeader {
                num_required_signatures: 1,
                ..MessageHeader::default()
            },
            account_keys: vec![Pubkey::new_unique(), Pubkey::new_unique()],
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![0],
                data: vec![]
            }],
            ..Message::default()
        }
        .sanitize(
            true, // require_static_program_ids
        )
        .is_ok());
    }

    #[test]
    fn test_sanitize_with_table_lookup() {
        assert!(Message {
            header: MessageHeader {
                num_required_signatures: 1,
                ..MessageHeader::default()
            },
            account_keys: vec![Pubkey::new_unique()],
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: Pubkey::new_unique(),
                writable_indexes: vec![1, 2, 3],
                readonly_indexes: vec![0],
            }],
            ..Message::default()
        }
        .sanitize(
            true, // require_static_program_ids
        )
        .is_ok());
    }

    #[test]
    fn test_sanitize_with_table_lookup_and_ix_with_dynamic_program_id() {
        let message = Message {
            header: MessageHeader {
                num_required_signatures: 1,
                ..MessageHeader::default()
            },
            account_keys: vec![Pubkey::new_unique()],
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: Pubkey::new_unique(),
                writable_indexes: vec![1, 2, 3],
                readonly_indexes: vec![0],
            }],
            instructions: vec![CompiledInstruction {
                program_id_index: 4,
                accounts: vec![0, 1, 2, 3],
                data: vec![],
            }],
            ..Message::default()
        };

        assert!(message.sanitize(
            false, // require_static_program_ids
        ).is_ok());

        assert!(message.sanitize(
            true, // require_static_program_ids
        ).is_err());
    }

    #[test]
    fn test_sanitize_with_table_lookup_and_ix_with_static_program_id() {
        assert!(Message {
            header: MessageHeader {
                num_required_signatures: 1,
                ..MessageHeader::default()
            },
            account_keys: vec![Pubkey::new_unique(), Pubkey::new_unique()],
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: Pubkey::new_unique(),
                writable_indexes: vec![1, 2, 3],
                readonly_indexes: vec![0],
            }],
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![2, 3, 4, 5],
                data: vec![]
            }],
            ..Message::default()
        }
        .sanitize(
            true, // require_static_program_ids
        )
        .is_ok());
    }

    #[test]
    fn test_sanitize_without_signer() {
        assert!(Message {
            header: MessageHeader::default(),
            account_keys: vec![Pubkey::new_unique()],
            ..Message::default()
        }
        .sanitize(
            true, // require_static_program_ids
        )
        .is_err());
    }

    #[test]
    fn test_sanitize_without_writable_signer() {
        assert!(Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 1,
                ..MessageHeader::default()
            },
            account_keys: vec![Pubkey::new_unique()],
            ..Message::default()
        }
        .sanitize(
            true, // require_static_program_ids
        )
        .is_err());
    }

    #[test]
    fn test_sanitize_with_empty_table_lookup() {
        assert!(Message {
            header: MessageHeader {
                num_required_signatures: 1,
                ..MessageHeader::default()
            },
            account_keys: vec![Pubkey::new_unique()],
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: Pubkey::new_unique(),
                writable_indexes: vec![],
                readonly_indexes: vec![],
            }],
            ..Message::default()
        }
        .sanitize(
            true, // require_static_program_ids
        )
        .is_err());
    }

    #[test]
    fn test_sanitize_with_max_account_keys() {
        assert!(Message {
            header: MessageHeader {
                num_required_signatures: 1,
                ..MessageHeader::default()
            },
            account_keys: (0..=u8::MAX).map(|_| Pubkey::new_unique()).collect(),
            ..Message::default()
        }
        .sanitize(
            true, // require_static_program_ids
        )
        .is_ok());
    }

    #[test]
    fn test_sanitize_with_too_many_account_keys() {
        assert!(Message {
            header: MessageHeader {
                num_required_signatures: 1,
                ..MessageHeader::default()
            },
            account_keys: (0..=256).map(|_| Pubkey::new_unique()).collect(),
            ..Message::default()
        }
        .sanitize(
            true, // require_static_program_ids
        )
        .is_err());
    }

    #[test]
    fn test_sanitize_with_max_table_loaded_keys() {
        assert!(Message {
            header: MessageHeader {
                num_required_signatures: 1,
                ..MessageHeader::default()
            },
            account_keys: vec![Pubkey::new_unique()],
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: Pubkey::new_unique(),
                writable_indexes: (0..=254).step_by(2).collect(),
                readonly_indexes: (1..=254).step_by(2).collect(),
            }],
            ..Message::default()
        }
        .sanitize(
            true, // require_static_program_ids
        )
        .is_ok());
    }

    #[test]
    fn test_sanitize_with_too_many_table_loaded_keys() {
        assert!(Message {
            header: MessageHeader {
                num_required_signatures: 1,
                ..MessageHeader::default()
            },
            account_keys: vec![Pubkey::new_unique()],
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: Pubkey::new_unique(),
                writable_indexes: (0..=255).step_by(2).collect(),
                readonly_indexes: (1..=255).step_by(2).collect(),
            }],
            ..Message::default()
        }
        .sanitize(
            true, // require_static_program_ids
        )
        .is_err());
    }

    #[test]
    fn test_sanitize_with_invalid_ix_program_id() {
        let message = Message {
            header: MessageHeader {
                num_required_signatures: 1,
                ..MessageHeader::default()
            },
            account_keys: vec![Pubkey::new_unique()],
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: Pubkey::new_unique(),
                writable_indexes: vec![0],
                readonly_indexes: vec![],
            }],
            instructions: vec![CompiledInstruction {
                program_id_index: 2,
                accounts: vec![],
                data: vec![],
            }],
            ..Message::default()
        };

        assert!(message
            .sanitize(true /* require_static_program_ids */)
            .is_err());
        assert!(message
            .sanitize(false /* require_static_program_ids */)
            .is_err());
    }

    #[test]
    fn test_sanitize_with_invalid_ix_account() {
        assert!(Message {
            header: MessageHeader {
                num_required_signatures: 1,
                ..MessageHeader::default()
            },
            account_keys: vec![Pubkey::new_unique(), Pubkey::new_unique()],
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: Pubkey::new_unique(),
                writable_indexes: vec![],
                readonly_indexes: vec![0],
            }],
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![3],
                data: vec![]
            }],
            ..Message::default()
        }
        .sanitize(
            true, // require_static_program_ids
        )
        .is_err());
    }

    #[test]
    fn test_serialize() {
        let message = Message::default();
        let versioned_msg = VersionedMessage::V0(message.clone());
        assert_eq!(message.serialize(), versioned_msg.serialize());
    }

    #[test]
    fn test_try_compile() {
        let mut keys = vec![];
        keys.resize_with(7, Pubkey::new_unique);

        let payer = keys[0];
        let program_id = keys[6];
        let instructions = vec![Instruction {
            program_id,
            accounts: vec![
                AccountMeta::new(keys[1], true),
                AccountMeta::new_readonly(keys[2], true),
                AccountMeta::new(keys[3], false),
                AccountMeta::new(keys[4], false), // loaded from lut
                AccountMeta::new_readonly(keys[5], false), // loaded from lut
            ],
            data: vec![],
        }];
        let address_lookup_table_accounts = vec![
            AddressLookupTableAccount {
                key: Pubkey::new_unique(),
                addresses: vec![keys[4], keys[5], keys[6]],
            },
            AddressLookupTableAccount {
                key: Pubkey::new_unique(),
                addresses: vec![],
            },
        ];

        let recent_blockhash = Hash::new_unique();
        assert_eq!(
            Message::try_compile(
                &payer,
                &instructions,
                &address_lookup_table_accounts,
                recent_blockhash
            ),
            Ok(Message {
                header: MessageHeader {
                    num_required_signatures: 3,
                    num_readonly_signed_accounts: 1,
                    num_readonly_unsigned_accounts: 1
                },
                recent_blockhash,
                account_keys: vec![keys[0], keys[1], keys[2], keys[3], program_id],
                instructions: vec![CompiledInstruction {
                    program_id_index: 4,
                    accounts: vec![1, 2, 3, 5, 6],
                    data: vec![],
                },],
                address_table_lookups: vec![MessageAddressTableLookup {
                    account_key: address_lookup_table_accounts[0].key,
                    writable_indexes: vec![0],
                    readonly_indexes: vec![1],
                }],
            })
        );
    }
}
