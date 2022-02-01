use crate::{
    hash::Hash,
    instruction::{AccountMeta, CompiledInstruction, Instruction},
    message::{MessageHeader, MESSAGE_VERSION_PREFIX},
    pubkey::Pubkey,
    sanitize::{Sanitize, SanitizeError},
    short_vec,
};
use std::collections::BTreeSet;

mod loaded;

pub use loaded::*;

fn position(keys: &[Pubkey], key: &Pubkey) -> u8 {
    keys.iter().position(|k| k == key).unwrap() as u8
}

fn compile_instruction(ix: &Instruction, keys: &[Pubkey]) -> CompiledInstruction {
    let accounts: Vec<_> = ix
        .accounts
        .iter()
        .map(|account_meta| position(keys, &account_meta.pubkey))
        .collect();

    CompiledInstruction {
        program_id_index: position(keys, &ix.program_id),
        data: ix.data.clone(),
        accounts,
    }
}

fn compile_instructions(ixs: &[Instruction], keys: &[Pubkey]) -> Vec<CompiledInstruction> {
    ixs.iter().map(|ix| compile_instruction(ix, keys)).collect()
}

/// Information about address lookup tables to use
pub struct AddressLookupTable {
    pub account_key: Pubkey,
    pub addresses: Vec<Pubkey>,
}

/// A helper struct to collect pubkeys referenced by a set of instructions and read-only counts
#[derive(Debug, PartialEq, Eq)]
struct InstructionKeys {
    pub signed_keys: Vec<Pubkey>,
    pub unsigned_keys: Vec<Pubkey>,
    pub address_table_keys: Vec<Pubkey>,
    pub address_table_lookups: Vec<MessageAddressTableLookup>,
    pub num_readonly_signed_accounts: u8,
    pub num_readonly_unsigned_accounts: u8,
}

impl InstructionKeys {
    fn new(
        signed_keys: Vec<Pubkey>,
        unsigned_keys: Vec<Pubkey>,
        address_table_keys: Vec<Pubkey>,
        address_table_lookups: Vec<MessageAddressTableLookup>,
        num_readonly_signed_accounts: u8,
        num_readonly_unsigned_accounts: u8,
    ) -> Self {
        Self {
            signed_keys,
            unsigned_keys,
            address_table_keys,
            address_table_lookups,
            num_readonly_signed_accounts,
            num_readonly_unsigned_accounts,
        }
    }
}

/// Return pubkeys referenced by all instructions, with the ones needing signatures first. If the
/// payer key is provided, it is always placed first in the list of signed keys. Read-only signed
/// accounts are placed last in the set of signed accounts. Read-only unsigned accounts,
/// including program ids, are placed last in the set. No duplicates and order is preserved.
fn get_keys(
    instructions: &[Instruction],
    payer: Option<&Pubkey>,
    address_lookup_tables: Option<&[AddressLookupTable]>,
) -> InstructionKeys {
    let programs: Vec<_> = get_program_ids(instructions)
        .iter()
        .map(|program_id| AccountMeta {
            pubkey: *program_id,
            is_signer: false,
            is_writable: false,
        })
        .collect();
    let mut keys_and_signed: Vec<_> = instructions
        .iter()
        .flat_map(|ix| ix.accounts.iter())
        .collect();
    keys_and_signed.extend(&programs);
    keys_and_signed.sort_by(|x, y| {
        y.is_signer
            .cmp(&x.is_signer)
            .then(y.is_writable.cmp(&x.is_writable))
    });

    let payer_account_meta;
    if let Some(payer) = payer {
        payer_account_meta = AccountMeta {
            pubkey: *payer,
            is_signer: true,
            is_writable: true,
        };
        keys_and_signed.insert(0, &payer_account_meta);
    }

    let mut unique_metas: Vec<AccountMeta> = vec![];
    for account_meta in keys_and_signed {
        // Promote to writable if a later AccountMeta requires it
        if let Some(x) = unique_metas
            .iter_mut()
            .find(|x| x.pubkey == account_meta.pubkey)
        {
            x.is_writable |= account_meta.is_writable;
            continue;
        }
        unique_metas.push(account_meta.clone());
    }

    let mut signed_keys = vec![];
    let mut unsigned_keys = vec![];
    let mut address_table_keys = vec![];
    let mut address_table_lookups: Vec<MessageAddressTableLookup> = address_lookup_tables
        .unwrap_or(&vec![])
        .iter()
        .map(|table| MessageAddressTableLookup {
            account_key: table.account_key,
            writable_indexes: vec![],
            readonly_indexes: vec![],
        })
        .collect();
    let mut num_readonly_signed_accounts = 0;
    let mut num_readonly_unsigned_accounts = 0;
    for account_meta in &unique_metas {
        if account_meta.is_signer {
            signed_keys.push(account_meta.pubkey);
            if !account_meta.is_writable {
                num_readonly_signed_accounts += 1;
            }
        } else {
            if let Some((table_index, key_index)) =
                find_first_address_table_pubkey(&account_meta.pubkey, address_lookup_tables)
            {
                if account_meta.is_writable {
                    address_table_lookups[table_index]
                        .writable_indexes
                        .push(key_index);
                } else {
                    address_table_lookups[table_index]
                        .readonly_indexes
                        .push(key_index);
                }
                address_table_keys.push(account_meta.pubkey);
            } else {
                unsigned_keys.push(account_meta.pubkey);
                if !account_meta.is_writable {
                    num_readonly_unsigned_accounts += 1;
                }
            }
        }
    }
    InstructionKeys::new(
        signed_keys,
        unsigned_keys,
        address_table_keys,
        address_table_lookups, // TODO: remove empty ones
        num_readonly_signed_accounts,
        num_readonly_unsigned_accounts,
    )
}

/// Returns the address table index, and the index of the pubkey in the table
fn find_first_address_table_pubkey(
    pubkey: &Pubkey,
    address_lookup_tables: Option<&[AddressLookupTable]>,
) -> Option<(usize, u8)> {
    if address_lookup_tables.is_none() {
        return None;
    }
    address_lookup_tables
        .unwrap()
        .iter()
        .enumerate()
        .find_map(|(table_index, table)| {
            table
                .addresses
                .iter()
                .enumerate()
                .find_map(|(key_index, key)| {
                    if key == pubkey {
                        Some((table_index, key_index as u8))
                    } else {
                        None
                    }
                })
        })
}

/// Return program ids referenced by all instructions.  No duplicates and order is preserved.
fn get_program_ids(instructions: &[Instruction]) -> Vec<Pubkey> {
    let mut set = BTreeSet::new();
    instructions
        .iter()
        .map(|ix| ix.program_id)
        .filter(|&program_id| set.insert(program_id))
        .collect()
}

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

/// Transaction message format which supports succinct account loading with
/// on-chain address lookup tables.
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
    pub fn new_with_compiled_instructions(
        num_required_signatures: u8,
        num_readonly_signed_accounts: u8,
        num_readonly_unsigned_accounts: u8,
        account_keys: Vec<Pubkey>,
        address_table_lookups: Vec<MessageAddressTableLookup>,
        recent_blockhash: Hash,
        instructions: Vec<CompiledInstruction>,
    ) -> Self {
        Self {
            header: MessageHeader {
                num_required_signatures,
                num_readonly_signed_accounts,
                num_readonly_unsigned_accounts,
            },
            account_keys,
            address_table_lookups,
            recent_blockhash,
            instructions,
        }
    }

    pub fn new_with_payer(
        instructions: &[Instruction],
        payer: Option<&Pubkey>,
        address_lookup_tables: Option<&[AddressLookupTable]>,
    ) -> Self {
        Self::new_with_blockhash(instructions, payer, address_lookup_tables, &Hash::default())
    }

    pub fn new_with_blockhash(
        instructions: &[Instruction],
        payer: Option<&Pubkey>,
        address_lookup_tables: Option<&[AddressLookupTable]>,
        blockhash: &Hash,
    ) -> Self {
        let InstructionKeys {
            mut signed_keys,
            unsigned_keys,
            address_table_keys,
            address_table_lookups,
            num_readonly_signed_accounts,
            num_readonly_unsigned_accounts,
        } = get_keys(instructions, payer, address_lookup_tables);
        let num_required_signatures = signed_keys.len() as u8;
        signed_keys.extend(&unsigned_keys);
        let mut all_keys = signed_keys.clone();
        all_keys.extend(&address_table_keys);
        let instructions = compile_instructions(instructions, &all_keys);
        Self::new_with_compiled_instructions(
            num_required_signatures,
            num_readonly_signed_accounts,
            num_readonly_unsigned_accounts,
            signed_keys,
            address_table_lookups,
            *blockhash,
            instructions,
        )
    }

    /// Serialize this message with a version #0 prefix using bincode encoding.
    pub fn serialize(&self) -> Vec<u8> {
        bincode::serialize(&(MESSAGE_VERSION_PREFIX, self)).unwrap()
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
