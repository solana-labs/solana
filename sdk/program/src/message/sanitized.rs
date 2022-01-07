use {
    crate::{
        hash::Hash,
        instruction::{CompiledInstruction, Instruction},
        message::{
            legacy::Message as LegacyMessage,
            v0::{self, LoadedAddresses},
            MessageHeader,
        },
        pubkey::Pubkey,
        sanitize::{Sanitize, SanitizeError},
        serialize_utils::{append_slice, append_u16, append_u8},
    },
    bitflags::bitflags,
    std::convert::TryFrom,
    thiserror::Error,
};

/// Sanitized message of a transaction which includes a set of atomic
/// instructions to be executed on-chain
#[derive(Debug, Clone)]
pub enum SanitizedMessage {
    /// Sanitized legacy message
    Legacy(LegacyMessage),
    /// Sanitized version #0 message with dynamically loaded addresses
    V0(v0::LoadedMessage),
}

#[derive(PartialEq, Debug, Error, Eq, Clone)]
pub enum SanitizeMessageError {
    #[error("index out of bounds")]
    IndexOutOfBounds,
    #[error("value out of bounds")]
    ValueOutOfBounds,
    #[error("invalid value")]
    InvalidValue,
}

impl From<SanitizeError> for SanitizeMessageError {
    fn from(err: SanitizeError) -> Self {
        match err {
            SanitizeError::IndexOutOfBounds => Self::IndexOutOfBounds,
            SanitizeError::ValueOutOfBounds => Self::ValueOutOfBounds,
            SanitizeError::InvalidValue => Self::InvalidValue,
        }
    }
}

impl TryFrom<LegacyMessage> for SanitizedMessage {
    type Error = SanitizeMessageError;
    fn try_from(message: LegacyMessage) -> Result<Self, Self::Error> {
        message.sanitize()?;
        Ok(Self::Legacy(message))
    }
}

bitflags! {
    struct InstructionsSysvarAccountMeta: u8 {
        const NONE = 0b00000000;
        const IS_SIGNER = 0b00000001;
        const IS_WRITABLE = 0b00000010;
    }
}

impl SanitizedMessage {
    /// Return true if this message contains duplicate account keys
    pub fn has_duplicates(&self) -> bool {
        match self {
            SanitizedMessage::Legacy(message) => message.has_duplicates(),
            SanitizedMessage::V0(message) => message.has_duplicates(),
        }
    }

    /// Message header which identifies the number of signer and writable or
    /// readonly accounts
    pub fn header(&self) -> &MessageHeader {
        match self {
            Self::Legacy(message) => &message.header,
            Self::V0(message) => &message.header,
        }
    }

    /// Returns a legacy message if this sanitized message wraps one
    pub fn legacy_message(&self) -> Option<&LegacyMessage> {
        if let Self::Legacy(message) = &self {
            Some(message)
        } else {
            None
        }
    }

    /// Returns the fee payer for the transaction
    pub fn fee_payer(&self) -> &Pubkey {
        self.get_account_key(0)
            .expect("sanitized message always has non-program fee payer at index 0")
    }

    /// The hash of a recent block, used for timing out a transaction
    pub fn recent_blockhash(&self) -> &Hash {
        match self {
            Self::Legacy(message) => &message.recent_blockhash,
            Self::V0(message) => &message.recent_blockhash,
        }
    }

    /// Program instructions that will be executed in sequence and committed in
    /// one atomic transaction if all succeed.
    pub fn instructions(&self) -> &[CompiledInstruction] {
        match self {
            Self::Legacy(message) => &message.instructions,
            Self::V0(message) => &message.instructions,
        }
    }

    /// Program instructions iterator which includes each instruction's program
    /// id.
    pub fn program_instructions_iter(
        &self,
    ) -> impl Iterator<Item = (&Pubkey, &CompiledInstruction)> {
        match self {
            Self::Legacy(message) => message.instructions.iter(),
            Self::V0(message) => message.instructions.iter(),
        }
        .map(move |ix| {
            (
                self.get_account_key(usize::from(ix.program_id_index))
                    .expect("program id index is sanitized"),
                ix,
            )
        })
    }

    /// Iterator of all account keys referenced in this message, including dynamically loaded keys.
    pub fn account_keys_iter(&self) -> Box<dyn Iterator<Item = &Pubkey> + '_> {
        match self {
            Self::Legacy(message) => Box::new(message.account_keys.iter()),
            Self::V0(message) => Box::new(message.account_keys_iter()),
        }
    }

    /// Length of all account keys referenced in this message, including dynamically loaded keys.
    pub fn account_keys_len(&self) -> usize {
        match self {
            Self::Legacy(message) => message.account_keys.len(),
            Self::V0(message) => message.account_keys_len(),
        }
    }

    /// Returns the address of the account at the specified index.
    pub fn get_account_key(&self, index: usize) -> Option<&Pubkey> {
        match self {
            Self::Legacy(message) => message.account_keys.get(index),
            Self::V0(message) => message.get_account_key(index),
        }
    }

    /// Returns true if the account at the specified index is an input to some
    /// program instruction in this message.
    fn is_key_passed_to_program(&self, key_index: usize) -> bool {
        if let Ok(key_index) = u8::try_from(key_index) {
            self.instructions()
                .iter()
                .any(|ix| ix.accounts.contains(&key_index))
        } else {
            false
        }
    }

    /// Returns true if the account at the specified index is invoked as a
    /// program in this message.
    pub fn is_invoked(&self, key_index: usize) -> bool {
        match self {
            Self::Legacy(message) => message.is_key_called_as_program(key_index),
            Self::V0(message) => message.is_key_called_as_program(key_index),
        }
    }

    /// Returns true if the account at the specified index is not invoked as a
    /// program or, if invoked, is passed to a program.
    pub fn is_non_loader_key(&self, key_index: usize) -> bool {
        !self.is_invoked(key_index) || self.is_key_passed_to_program(key_index)
    }

    /// Returns true if the account at the specified index is writable by the
    /// instructions in this message.
    pub fn is_writable(&self, index: usize) -> bool {
        match self {
            Self::Legacy(message) => message.is_writable(index),
            Self::V0(message) => message.is_writable(index),
        }
    }

    /// Returns true if the account at the specified index signed this
    /// message.
    pub fn is_signer(&self, index: usize) -> bool {
        index < usize::from(self.header().num_required_signatures)
    }

    // First encode the number of instructions:
    // [0..2 - num_instructions
    //
    // Then a table of offsets of where to find them in the data
    //  3..2 * num_instructions table of instruction offsets
    //
    // Each instruction is then encoded as:
    //   0..2 - num_accounts
    //   2 - meta_byte -> (bit 0 signer, bit 1 is_writable)
    //   3..35 - pubkey - 32 bytes
    //   35..67 - program_id
    //   67..69 - data len - u16
    //   69..data_len - data
    #[allow(clippy::integer_arithmetic)]
    pub fn serialize_instructions(&self) -> Vec<u8> {
        // 64 bytes is a reasonable guess, calculating exactly is slower in benchmarks
        let mut data = Vec::with_capacity(self.instructions().len() * (32 * 2));
        append_u16(&mut data, self.instructions().len() as u16);
        for _ in 0..self.instructions().len() {
            append_u16(&mut data, 0);
        }
        for (i, (program_id, instruction)) in self.program_instructions_iter().enumerate() {
            let start_instruction_offset = data.len() as u16;
            let start = 2 + (2 * i);
            data[start..start + 2].copy_from_slice(&start_instruction_offset.to_le_bytes());
            append_u16(&mut data, instruction.accounts.len() as u16);
            for account_index in &instruction.accounts {
                let account_index = *account_index as usize;
                let is_signer = self.is_signer(account_index);
                let is_writable = self.is_writable(account_index);
                let mut account_meta = InstructionsSysvarAccountMeta::NONE;
                if is_signer {
                    account_meta |= InstructionsSysvarAccountMeta::IS_SIGNER;
                }
                if is_writable {
                    account_meta |= InstructionsSysvarAccountMeta::IS_WRITABLE;
                }
                append_u8(&mut data, account_meta.bits());
                append_slice(
                    &mut data,
                    self.get_account_key(account_index).unwrap().as_ref(),
                );
            }

            append_slice(&mut data, program_id.as_ref());
            append_u16(&mut data, instruction.data.len() as u16);
            append_slice(&mut data, &instruction.data);
        }
        data
    }

    /// Return the resolved addresses for this message if it has any.
    fn loaded_lookup_table_addresses(&self) -> Option<&LoadedAddresses> {
        match &self {
            SanitizedMessage::V0(message) => Some(&message.loaded_addresses),
            _ => None,
        }
    }

    /// Return the number of readonly accounts loaded by this message.
    pub fn num_readonly_accounts(&self) -> usize {
        let loaded_readonly_addresses = self
            .loaded_lookup_table_addresses()
            .map(|keys| keys.readonly.len())
            .unwrap_or_default();
        loaded_readonly_addresses
            .saturating_add(usize::from(self.header().num_readonly_signed_accounts))
            .saturating_add(usize::from(self.header().num_readonly_unsigned_accounts))
    }

    fn try_position(&self, key: &Pubkey) -> Option<u8> {
        u8::try_from(self.account_keys_iter().position(|k| k == key)?).ok()
    }

    /// Try to compile an instruction using the account keys in this message.
    pub fn try_compile_instruction(&self, ix: &Instruction) -> Option<CompiledInstruction> {
        let accounts: Vec<_> = ix
            .accounts
            .iter()
            .map(|account_meta| self.try_position(&account_meta.pubkey))
            .collect::<Option<_>>()?;

        Some(CompiledInstruction {
            program_id_index: self.try_position(&ix.program_id)?,
            data: ix.data.clone(),
            accounts,
        })
    }

    /// Inspect all message keys for the bpf upgradeable loader
    pub fn is_upgradeable_loader_present(&self) -> bool {
        match self {
            Self::Legacy(message) => message.is_upgradeable_loader_present(),
            Self::V0(message) => message.is_upgradeable_loader_present(),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            instruction::{AccountMeta, Instruction},
            message::v0,
        },
    };

    #[test]
    fn test_try_from_message() {
        let legacy_message_with_no_signers = LegacyMessage {
            account_keys: vec![Pubkey::new_unique()],
            ..LegacyMessage::default()
        };

        assert_eq!(
            SanitizedMessage::try_from(legacy_message_with_no_signers).err(),
            Some(SanitizeMessageError::IndexOutOfBounds),
        );
    }

    #[test]
    fn test_is_non_loader_key() {
        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let loader_key = Pubkey::new_unique();
        let instructions = vec![
            CompiledInstruction::new(1, &(), vec![0]),
            CompiledInstruction::new(2, &(), vec![0, 1]),
        ];

        let message = SanitizedMessage::try_from(LegacyMessage::new_with_compiled_instructions(
            1,
            0,
            2,
            vec![key0, key1, loader_key],
            Hash::default(),
            instructions,
        ))
        .unwrap();

        assert!(message.is_non_loader_key(0));
        assert!(message.is_non_loader_key(1));
        assert!(!message.is_non_loader_key(2));
    }

    #[test]
    fn test_num_readonly_accounts() {
        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let key3 = Pubkey::new_unique();
        let key4 = Pubkey::new_unique();
        let key5 = Pubkey::new_unique();

        let legacy_message = SanitizedMessage::try_from(LegacyMessage {
            header: MessageHeader {
                num_required_signatures: 2,
                num_readonly_signed_accounts: 1,
                num_readonly_unsigned_accounts: 1,
            },
            account_keys: vec![key0, key1, key2, key3],
            ..LegacyMessage::default()
        })
        .unwrap();

        assert_eq!(legacy_message.num_readonly_accounts(), 2);

        let v0_message = SanitizedMessage::V0(v0::LoadedMessage {
            message: v0::Message {
                header: MessageHeader {
                    num_required_signatures: 2,
                    num_readonly_signed_accounts: 1,
                    num_readonly_unsigned_accounts: 1,
                },
                account_keys: vec![key0, key1, key2, key3],
                ..v0::Message::default()
            },
            loaded_addresses: LoadedAddresses {
                writable: vec![key4],
                readonly: vec![key5],
            },
        });

        assert_eq!(v0_message.num_readonly_accounts(), 3);
    }

    #[test]
    #[allow(deprecated)]
    fn test_serialize_instructions() {
        let program_id0 = Pubkey::new_unique();
        let program_id1 = Pubkey::new_unique();
        let id0 = Pubkey::new_unique();
        let id1 = Pubkey::new_unique();
        let id2 = Pubkey::new_unique();
        let id3 = Pubkey::new_unique();
        let instructions = vec![
            Instruction::new_with_bincode(program_id0, &0, vec![AccountMeta::new(id0, false)]),
            Instruction::new_with_bincode(program_id0, &0, vec![AccountMeta::new(id1, true)]),
            Instruction::new_with_bincode(
                program_id1,
                &0,
                vec![AccountMeta::new_readonly(id2, false)],
            ),
            Instruction::new_with_bincode(
                program_id1,
                &0,
                vec![AccountMeta::new_readonly(id3, true)],
            ),
        ];

        let message = LegacyMessage::new(&instructions, Some(&id1));
        let sanitized_message = SanitizedMessage::try_from(message.clone()).unwrap();
        let serialized = sanitized_message.serialize_instructions();

        // assert that SanitizedMessage::serialize_instructions has the same behavior as the
        // deprecated Message::serialize_instructions method
        assert_eq!(serialized, message.serialize_instructions());

        // assert that Message::deserialize_instruction is compatible with SanitizedMessage::serialize_instructions
        for (i, instruction) in instructions.iter().enumerate() {
            assert_eq!(
                LegacyMessage::deserialize_instruction(i, &serialized).unwrap(),
                *instruction
            );
        }
    }

    #[test]
    fn test_try_compile_instruction() {
        let key0 = Pubkey::new_unique();
        let key1 = Pubkey::new_unique();
        let key2 = Pubkey::new_unique();
        let program_id = Pubkey::new_unique();

        let valid_instruction = Instruction {
            program_id,
            accounts: vec![
                AccountMeta::new_readonly(key0, false),
                AccountMeta::new_readonly(key1, false),
                AccountMeta::new_readonly(key2, false),
            ],
            data: vec![],
        };

        let invalid_program_id_instruction = Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![
                AccountMeta::new_readonly(key0, false),
                AccountMeta::new_readonly(key1, false),
                AccountMeta::new_readonly(key2, false),
            ],
            data: vec![],
        };

        let invalid_account_key_instruction = Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![
                AccountMeta::new_readonly(key0, false),
                AccountMeta::new_readonly(key1, false),
                AccountMeta::new_readonly(Pubkey::new_unique(), false),
            ],
            data: vec![],
        };

        let legacy_message = SanitizedMessage::try_from(LegacyMessage {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 0,
            },
            account_keys: vec![key0, key1, key2, program_id],
            ..LegacyMessage::default()
        })
        .unwrap();

        let v0_message = SanitizedMessage::V0(v0::LoadedMessage {
            message: v0::Message {
                header: MessageHeader {
                    num_required_signatures: 1,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                account_keys: vec![key0, key1],
                ..v0::Message::default()
            },
            loaded_addresses: LoadedAddresses {
                writable: vec![key2],
                readonly: vec![program_id],
            },
        });

        for message in vec![legacy_message, v0_message] {
            assert_eq!(
                message.try_compile_instruction(&valid_instruction),
                Some(CompiledInstruction {
                    program_id_index: 3,
                    accounts: vec![0, 1, 2],
                    data: vec![],
                })
            );

            assert!(message
                .try_compile_instruction(&invalid_program_id_instruction)
                .is_none());
            assert!(message
                .try_compile_instruction(&invalid_account_key_instruction)
                .is_none());
        }
    }
}
