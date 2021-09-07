use crate::{
    hash::Hash,
    instruction::CompiledInstruction,
    message::{MessageHeader, MESSAGE_VERSION_PREFIX},
    pubkey::Pubkey,
    sanitize::{Sanitize, SanitizeError},
    short_vec,
};

/// Indexes that are mapped to addresses using an on-chain address map for
/// succinctly loading readonly and writable accounts.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, AbiExample)]
#[serde(rename_all = "camelCase")]
pub struct AddressMapIndexes {
    #[serde(with = "short_vec")]
    pub writable: Vec<u8>,
    #[serde(with = "short_vec")]
    pub readonly: Vec<u8>,
}

/// Transaction message format which supports succinct account loading with
/// indexes for on-chain address maps.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, AbiExample)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// The message header, identifying signed and read-only `account_keys`
    pub header: MessageHeader,

    /// List of accounts loaded by this transaction.
    #[serde(with = "short_vec")]
    pub account_keys: Vec<Pubkey>,

    /// List of address map indexes used to succinctly load additional accounts
    /// for this transaction.
    ///
    /// # Notes
    ///
    /// The last `address_map_indexes.len()` accounts of the read-only unsigned
    /// accounts are loaded as address maps.
    #[serde(with = "short_vec")]
    pub address_map_indexes: Vec<AddressMapIndexes>,

    /// The blockhash of a recent block.
    pub recent_blockhash: Hash,

    /// Instructions that invoke a designated program, are executed in sequence,
    /// and committed in one atomic transaction if all succeed.
    ///
    /// # Notes
    ///
    /// Account and program indexes will index into the list of addresses
    /// constructed from the concatenation of `account_keys`, flattened list of
    /// `writable` address map indexes, and the flattened `readonly` address
    /// map indexes.
    #[serde(with = "short_vec")]
    pub instructions: Vec<CompiledInstruction>,
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
            return Err(SanitizeError::IndexOutOfBounds);
        }

        // there cannot be more address maps than read-only unsigned accounts.
        let num_address_map_indexes = self.address_map_indexes.len();
        if num_address_map_indexes > usize::from(self.header.num_readonly_unsigned_accounts) {
            return Err(SanitizeError::IndexOutOfBounds);
        }

        // each map must load at least one entry
        let mut num_loaded_accounts = self.account_keys.len();
        for indexes in &self.address_map_indexes {
            let num_loaded_map_entries = indexes
                .writable
                .len()
                .saturating_add(indexes.readonly.len());

            if num_loaded_map_entries == 0 {
                return Err(SanitizeError::InvalidValue);
            }

            num_loaded_accounts = num_loaded_accounts.saturating_add(num_loaded_map_entries);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::VersionedMessage;

    fn simple_message() -> Message {
        Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![Pubkey::new_unique(), Pubkey::new_unique()],
            address_map_indexes: vec![AddressMapIndexes {
                writable: vec![],
                readonly: vec![0],
            }],
            ..Message::default()
        }
    }

    fn two_map_message() -> Message {
        Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 2,
            },
            account_keys: vec![
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                Pubkey::new_unique(),
            ],
            address_map_indexes: vec![
                AddressMapIndexes {
                    writable: vec![1],
                    readonly: vec![0],
                },
                AddressMapIndexes {
                    writable: vec![0],
                    readonly: vec![1],
                },
            ],
            ..Message::default()
        }
    }

    #[test]
    fn test_sanitize_account_indices() {
        assert!(Message {
            account_keys: (0..=u8::MAX).map(|_| Pubkey::new_unique()).collect(),
            address_map_indexes: vec![],
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![u8::MAX],
                data: vec![],
            }],
            ..simple_message()
        }
        .sanitize()
        .is_ok());

        assert!(Message {
            account_keys: (0..u8::MAX).map(|_| Pubkey::new_unique()).collect(),
            address_map_indexes: vec![],
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![u8::MAX],
                data: vec![],
            }],
            ..simple_message()
        }
        .sanitize()
        .is_err());

        assert!(Message {
            account_keys: (0..u8::MAX).map(|_| Pubkey::new_unique()).collect(),
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![u8::MAX],
                data: vec![],
            }],
            ..simple_message()
        }
        .sanitize()
        .is_ok());

        assert!(Message {
            account_keys: (0..u8::MAX - 1).map(|_| Pubkey::new_unique()).collect(),
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![u8::MAX],
                data: vec![],
            }],
            ..simple_message()
        }
        .sanitize()
        .is_err());

        assert!(Message {
            address_map_indexes: vec![
                AddressMapIndexes {
                    writable: (0..200).step_by(2).collect(),
                    readonly: (1..200).step_by(2).collect(),
                },
                AddressMapIndexes {
                    writable: (0..53).step_by(2).collect(),
                    readonly: (1..53).step_by(2).collect(),
                },
            ],
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![u8::MAX],
                data: vec![],
            }],
            ..two_map_message()
        }
        .sanitize()
        .is_ok());

        assert!(Message {
            address_map_indexes: vec![
                AddressMapIndexes {
                    writable: (0..200).step_by(2).collect(),
                    readonly: (1..200).step_by(2).collect(),
                },
                AddressMapIndexes {
                    writable: (0..52).step_by(2).collect(),
                    readonly: (1..52).step_by(2).collect(),
                },
            ],
            instructions: vec![CompiledInstruction {
                program_id_index: 1,
                accounts: vec![u8::MAX],
                data: vec![],
            }],
            ..two_map_message()
        }
        .sanitize()
        .is_err());
    }

    #[test]
    fn test_sanitize_excessive_loaded_accounts() {
        assert!(Message {
            account_keys: (0..=u8::MAX).map(|_| Pubkey::new_unique()).collect(),
            address_map_indexes: vec![],
            ..simple_message()
        }
        .sanitize()
        .is_ok());

        assert!(Message {
            account_keys: (0..257).map(|_| Pubkey::new_unique()).collect(),
            address_map_indexes: vec![],
            ..simple_message()
        }
        .sanitize()
        .is_err());

        assert!(Message {
            account_keys: (0..u8::MAX).map(|_| Pubkey::new_unique()).collect(),
            ..simple_message()
        }
        .sanitize()
        .is_ok());

        assert!(Message {
            account_keys: (0..256).map(|_| Pubkey::new_unique()).collect(),
            ..simple_message()
        }
        .sanitize()
        .is_err());

        assert!(Message {
            address_map_indexes: vec![
                AddressMapIndexes {
                    writable: (0..200).step_by(2).collect(),
                    readonly: (1..200).step_by(2).collect(),
                },
                AddressMapIndexes {
                    writable: (0..53).step_by(2).collect(),
                    readonly: (1..53).step_by(2).collect(),
                }
            ],
            ..two_map_message()
        }
        .sanitize()
        .is_ok());

        assert!(Message {
            address_map_indexes: vec![
                AddressMapIndexes {
                    writable: (0..200).step_by(2).collect(),
                    readonly: (1..200).step_by(2).collect(),
                },
                AddressMapIndexes {
                    writable: (0..200).step_by(2).collect(),
                    readonly: (1..200).step_by(2).collect(),
                }
            ],
            ..two_map_message()
        }
        .sanitize()
        .is_err());
    }

    #[test]
    fn test_sanitize_excessive_maps() {
        assert!(Message {
            header: MessageHeader {
                num_readonly_unsigned_accounts: 1,
                ..simple_message().header
            },
            ..simple_message()
        }
        .sanitize()
        .is_ok());

        assert!(Message {
            header: MessageHeader {
                num_readonly_unsigned_accounts: 0,
                ..simple_message().header
            },
            ..simple_message()
        }
        .sanitize()
        .is_err());
    }

    #[test]
    fn test_sanitize_address_map() {
        assert!(Message {
            address_map_indexes: vec![AddressMapIndexes {
                writable: vec![0],
                readonly: vec![],
            }],
            ..simple_message()
        }
        .sanitize()
        .is_ok());

        assert!(Message {
            address_map_indexes: vec![AddressMapIndexes {
                writable: vec![],
                readonly: vec![0],
            }],
            ..simple_message()
        }
        .sanitize()
        .is_ok());

        assert!(Message {
            address_map_indexes: vec![AddressMapIndexes {
                writable: vec![],
                readonly: vec![],
            }],
            ..simple_message()
        }
        .sanitize()
        .is_err());
    }

    #[test]
    fn test_serialize() {
        let message = simple_message();
        let versioned_msg = VersionedMessage::V0(message.clone());
        assert_eq!(message.serialize(), versioned_msg.serialize());
    }
}
