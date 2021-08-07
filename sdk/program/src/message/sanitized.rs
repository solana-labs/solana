use {
    crate::{
        hash::Hash,
        instruction::{CompiledInstruction, Instruction},
        message::{MappedAddresses, MappedMessage, Message, MessageHeader},
        pubkey::Pubkey,
        sanitize::{Sanitize, SanitizeError},
        serialize_utils::{append_slice, append_u16, append_u8},
    },
    std::convert::TryFrom,
};

/// Sanitized message of a transaction which includes a set of atomic
/// instructions to be executed on-chain
#[derive(Debug, Clone)]
pub enum SanitizedMessage {
    /// Sanitized legacy message
    Legacy(Message),
    /// Sanitized version #0 message with mapped addresses
    V0(MappedMessage),
}

impl TryFrom<Message> for SanitizedMessage {
    type Error = SanitizeError;
    fn try_from(message: Message) -> Result<Self, Self::Error> {
        message.sanitize()?;
        Ok(Self::Legacy(message))
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
            Self::V0(mapped_msg) => &mapped_msg.message.header,
        }
    }

    /// Returns a legacy message if this sanitized message wraps one
    pub fn legacy_message(&self) -> Option<&Message> {
        if let Self::Legacy(message) = &self {
            Some(message)
        } else {
            None
        }
    }

    /// Returns the fee payer for the transaction
    pub fn fee_payer(&self) -> &Pubkey {
        self.get_account_key(0)
            .expect("sanitized message always has payer at index 0")
    }

    /// The hash of a recent block, used for timing out a transaction
    pub fn recent_blockhash(&self) -> &Hash {
        match self {
            Self::Legacy(message) => &message.recent_blockhash,
            Self::V0(mapped_msg) => &mapped_msg.message.recent_blockhash,
        }
    }

    /// Program instructions that will be executed in sequence and committed in
    /// one atomic transaction if all succeed.
    pub fn instructions(&self) -> &[CompiledInstruction] {
        match self {
            Self::Legacy(message) => &message.instructions,
            Self::V0(mapped_msg) => &mapped_msg.message.instructions,
        }
    }

    /// Program instructions iterator which includes each instruction's program
    /// id.
    pub fn program_instructions_iter(
        &self,
    ) -> impl Iterator<Item = (&Pubkey, &CompiledInstruction)> {
        match self {
            Self::Legacy(message) => message.instructions.iter(),
            Self::V0(mapped_msg) => mapped_msg.message.instructions.iter(),
        }
        .map(move |ix| {
            (
                self.get_account_key(ix.program_id_index as usize)
                    .expect("program id index is sanitized"),
                ix,
            )
        })
    }

    /// Iterator of all account keys referenced in this message, included mapped keys.
    pub fn account_keys_iter(&self) -> Box<dyn Iterator<Item = &Pubkey> + '_> {
        match self {
            Self::Legacy(message) => Box::new(message.account_keys.iter()),
            Self::V0(mapped_msg) => Box::new(mapped_msg.account_keys_iter()),
        }
    }

    /// Length of all account keys referenced in this message, included mapped keys.
    pub fn account_keys_len(&self) -> usize {
        match self {
            Self::Legacy(message) => message.account_keys.len(),
            Self::V0(mapped_msg) => mapped_msg.account_keys_len(),
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
        if let Ok(key_index) = u8::try_from(key_index) {
            self.instructions()
                .iter()
                .any(|ix| ix.program_id_index == key_index)
        } else {
            false
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
        index < self.header().num_required_signatures as usize
    }

    const IS_SIGNER_BIT: usize = 0;
    const IS_WRITABLE_BIT: usize = 1;

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
                let mut meta_byte = 0;
                if is_signer {
                    meta_byte |= 1 << Self::IS_SIGNER_BIT;
                }
                if is_writable {
                    meta_byte |= 1 << Self::IS_WRITABLE_BIT;
                }
                append_u8(&mut data, meta_byte);
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

    /// Return the mapped addresses for this message if it has any.
    pub fn mapped_addresses(&self) -> Option<&MappedAddresses> {
        match &self {
            SanitizedMessage::V0(message) => Some(&message.mapped_addresses),
            _ => None,
        }
    }

    /// Return the number of readonly accounts loaded by this message.
    pub fn num_readonly_accounts(&self) -> usize {
        let mapped_readonly_addresses = self
            .mapped_addresses()
            .map(|keys| keys.readonly.len())
            .unwrap_or_default();
        mapped_readonly_addresses
            .saturating_add(self.header().num_readonly_signed_accounts as usize)
            .saturating_add(self.header().num_readonly_unsigned_accounts as usize)
    }

    fn try_position(&self, key: &Pubkey) -> Option<u8> {
        Some(self.account_keys_iter().position(|k| k == key)? as u8)
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
}
