//! A future Solana message format.
//!
//! This crate defines two versions of `Message` in their own modules:
//! [`legacy`] and [`v0`]. `legacy` is the current version as of Solana 1.10.0.
//! `v0` is a [future message format] that encodes more account keys into a
//! transaction than the legacy format.
//!
//! [`legacy`]: crate::message::legacy
//! [`v0`]: crate::message::v0
//! [future message format]: https://docs.solana.com/proposals/transactions-v2

use crate::{
    bpf_loader_upgradeable,
    hash::Hash,
    instruction::CompiledInstruction,
    message::{legacy::BUILTIN_PROGRAMS_KEYS, MessageHeader, MESSAGE_VERSION_PREFIX},
    pubkey::Pubkey,
    sanitize::{Sanitize, SanitizeError},
    short_vec, sysvar,
};

mod loaded;

pub use loaded::*;

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
    /// The message header, identifying signed and read-only `account_keys`
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
    /// Account and program indexes will index into the list of addresses
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

impl Sanitize for Message {
    fn sanitize(&self) -> Result<(), SanitizeError> {
        // signing area and read-only non-signing area should not
        // overlap
        if usize::from(self.header.num_required_signatures)
            .saturating_add(usize::from(self.header.num_readonly_unsigned_accounts))
            > self.account_keys.len()
        {
            return Err(SanitizeError::IndexOutOfBounds);
        }

        // there should be at least 1 RW fee-payer account.
        if self.header.num_readonly_signed_accounts >= self.header.num_required_signatures {
            return Err(SanitizeError::InvalidValue);
        }

        let mut num_loaded_accounts = self.account_keys.len();
        for lookup in &self.address_table_lookups {
            let num_table_loaded_accounts = lookup
                .writable_indexes
                .len()
                .saturating_add(lookup.readonly_indexes.len());

            // each lookup table must be used to load at least one account
            if num_table_loaded_accounts == 0 {
                return Err(SanitizeError::InvalidValue);
            }

            num_loaded_accounts = num_loaded_accounts.saturating_add(num_table_loaded_accounts);
        }

        // the number of loaded accounts must be <= 256 since account indices are
        // encoded as `u8`
        if num_loaded_accounts > 256 {
            return Err(SanitizeError::IndexOutOfBounds);
        }

        for ci in &self.instructions {
            if usize::from(ci.program_id_index) >= num_loaded_accounts {
                return Err(SanitizeError::IndexOutOfBounds);
            }
            // A program cannot be a payer.
            if ci.program_id_index == 0 {
                return Err(SanitizeError::IndexOutOfBounds);
            }
            for ai in &ci.accounts {
                if usize::from(*ai) >= num_loaded_accounts {
                    return Err(SanitizeError::IndexOutOfBounds);
                }
            }
        }

        Ok(())
    }
}

impl Message {
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
                    .map(|key| sysvar::is_sysvar_id(key) || BUILTIN_PROGRAMS_KEYS.contains(key))
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
    use {super::*, crate::message::VersionedMessage};

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
        .sanitize()
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
        .sanitize()
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
        .sanitize()
        .is_ok());
    }

    #[test]
    fn test_sanitize_with_table_lookup_and_ix() {
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
            instructions: vec![CompiledInstruction {
                program_id_index: 4,
                accounts: vec![0, 1, 2, 3],
                data: vec![]
            }],
            ..Message::default()
        }
        .sanitize()
        .is_ok());
    }

    #[test]
    fn test_sanitize_without_signer() {
        assert!(Message {
            header: MessageHeader::default(),
            account_keys: vec![Pubkey::new_unique()],
            ..Message::default()
        }
        .sanitize()
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
        .sanitize()
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
        .sanitize()
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
        .sanitize()
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
        .sanitize()
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
        .sanitize()
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
        .sanitize()
        .is_err());
    }

    #[test]
    fn test_sanitize_with_invalid_ix_program_id() {
        assert!(Message {
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
                data: vec![]
            }],
            ..Message::default()
        }
        .sanitize()
        .is_err());
    }

    #[test]
    fn test_sanitize_with_invalid_ix_account() {
        assert!(Message {
            header: MessageHeader {
                num_required_signatures: 1,
                ..MessageHeader::default()
            },
            account_keys: vec![Pubkey::new_unique()],
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: Pubkey::new_unique(),
                writable_indexes: vec![],
                readonly_indexes: vec![0],
            }],
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![2],
                data: vec![]
            }],
            ..Message::default()
        }
        .sanitize()
        .is_err());
    }
    #[test]
    fn test_serialize() {
        let message = Message::default();
        let versioned_msg = VersionedMessage::V0(message.clone());
        assert_eq!(message.serialize(), versioned_msg.serialize());
    }
}
