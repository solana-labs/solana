use {
    crate::{
        address_table_lookup_meta::AddressTableLookupMeta,
        bytes::advance_offset_for_type,
        instructions_meta::{InstructionsIterator, InstructionsMeta},
        message_header_meta::{MessageHeaderMeta, TransactionVersion},
        result::{Result, TransactionParsingError},
        signature_meta::SignatureMeta,
        static_account_keys_meta::StaticAccountKeysMeta,
    },
    solana_sdk::{hash::Hash, pubkey::Pubkey, signature::Signature},
};

pub struct TransactionMeta {
    /// Signature metadata.
    pub(crate) signature: SignatureMeta,
    /// Message header metadata.
    pub(crate) message_header: MessageHeaderMeta,
    /// Static account keys metadata.
    pub(crate) static_account_keys: StaticAccountKeysMeta,
    /// Recent blockhash offset.
    pub(crate) recent_blockhash_offset: u16,
    /// Instructions metadata.
    pub(crate) instructions: InstructionsMeta,
    /// Address table lookup metadata.
    pub(crate) address_table_lookup: AddressTableLookupMeta,
}

impl TransactionMeta {
    /// Parse a serialized transaction and verify basic structure.
    /// The `bytes` parameter must have no trailing data.
    pub fn try_new(bytes: &[u8]) -> Result<Self> {
        let mut offset = 0;
        let signature = SignatureMeta::try_new(bytes, &mut offset)?;
        let message_header = MessageHeaderMeta::try_new(bytes, &mut offset)?;
        let static_account_keys = StaticAccountKeysMeta::try_new(bytes, &mut offset)?;

        // The recent blockhash is the first account key after the static
        // account keys. The recent blockhash is always present in a valid
        // transaction and has a fixed size of 32 bytes.
        let recent_blockhash_offset = offset as u16;
        advance_offset_for_type::<Hash>(bytes, &mut offset)?;

        let instructions = InstructionsMeta::try_new(bytes, &mut offset)?;
        let address_table_lookup = match message_header.version {
            TransactionVersion::Legacy => AddressTableLookupMeta {
                num_address_table_lookup: 0,
                offset: 0,
            },
            TransactionVersion::V0 => AddressTableLookupMeta::try_new(bytes, &mut offset)?,
        };

        // Verify that the entire transaction was parsed.
        if offset != bytes.len() {
            return Err(TransactionParsingError);
        }

        Ok(Self {
            signature,
            message_header,
            static_account_keys,
            recent_blockhash_offset,
            instructions,
            address_table_lookup,
        })
    }

    /// Return the number of signatures in the transaction.
    pub fn num_signatures(&self) -> u8 {
        self.signature.num_signatures
    }

    /// Return the version of the transaction.
    pub fn version(&self) -> TransactionVersion {
        self.message_header.version
    }

    /// Return the number of required signatures in the transaction.
    pub fn num_required_signatures(&self) -> u8 {
        self.message_header.num_required_signatures
    }

    /// Return the number of readonly signed accounts in the transaction.
    pub fn num_readonly_signed_accounts(&self) -> u8 {
        self.message_header.num_readonly_signed_accounts
    }

    /// Return the number of readonly unsigned accounts in the transaction.
    pub fn num_readonly_unsigned_accounts(&self) -> u8 {
        self.message_header.num_readonly_unsigned_accounts
    }

    /// Return the number of static account keys in the transaction.
    pub fn num_static_account_keys(&self) -> u8 {
        self.static_account_keys.num_static_accounts
    }

    /// Return the number of instructions in the transaction.
    pub fn num_instructions(&self) -> u16 {
        self.instructions.num_instructions
    }

    /// Return the number of address table lookups in the transaction.
    pub fn num_address_table_lookups(&self) -> u8 {
        self.address_table_lookup.num_address_table_lookup
    }
}

// Separate implementation for `unsafe` accessor methods.
impl TransactionMeta {
    /// Return the slice of signatures in the transaction.
    /// # Safety
    ///   - This function must be called with the same `bytes` slice that was
    ///     used to create the `TransactionMeta` instance.
    pub unsafe fn signatures(&self, bytes: &[u8]) -> &[Signature] {
        core::slice::from_raw_parts(
            bytes.as_ptr().add(usize::from(self.signature.offset)) as *const Signature,
            usize::from(self.signature.num_signatures),
        )
    }

    /// Return the slice of static account keys in the transaction.
    ///
    /// # Safety
    ///  - This function must be called with the same `bytes` slice that was
    ///    used to create the `TransactionMeta` instance.
    pub unsafe fn static_account_keys(&self, bytes: &[u8]) -> &[Pubkey] {
        core::slice::from_raw_parts(
            bytes
                .as_ptr()
                .add(usize::from(self.static_account_keys.offset)) as *const Pubkey,
            usize::from(self.static_account_keys.num_static_accounts),
        )
    }

    /// Return the recent blockhash in the transaction.
    /// # Safety
    /// - This function must be called with the same `bytes` slice that was
    ///   used to create the `TransactionMeta` instance.
    pub unsafe fn recent_blockhash(&self, bytes: &[u8]) -> &Hash {
        &*(bytes
            .as_ptr()
            .add(usize::from(self.recent_blockhash_offset)) as *const Hash)
    }

    /// Return an iterator over the instructions in the transaction.
    /// # Safety
    /// - This function must be called with the same `bytes` slice that was
    ///   used to create the `TransactionMeta` instance.
    pub unsafe fn instructions_iter<'a>(&self, bytes: &'a [u8]) -> InstructionsIterator<'a> {
        InstructionsIterator {
            bytes,
            offset: usize::from(self.instructions.offset),
            num_instructions: self.instructions.num_instructions,
            index: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            address_lookup_table::AddressLookupTableAccount,
            message::{v0, Message, MessageHeader, VersionedMessage},
            pubkey::Pubkey,
            signature::Signature,
            system_instruction::{self, SystemInstruction},
            transaction::VersionedTransaction,
        },
    };

    fn verify_transaction_view_meta(tx: &VersionedTransaction) {
        let bytes = bincode::serialize(tx).unwrap();
        let meta = TransactionMeta::try_new(&bytes).unwrap();

        assert_eq!(meta.signature.num_signatures, tx.signatures.len() as u8);
        assert_eq!(meta.signature.offset as usize, 1);

        assert_eq!(
            meta.message_header.num_required_signatures,
            tx.message.header().num_required_signatures
        );
        assert_eq!(
            meta.message_header.num_readonly_signed_accounts,
            tx.message.header().num_readonly_signed_accounts
        );
        assert_eq!(
            meta.message_header.num_readonly_unsigned_accounts,
            tx.message.header().num_readonly_unsigned_accounts
        );

        assert_eq!(
            meta.static_account_keys.num_static_accounts,
            tx.message.static_account_keys().len() as u8
        );
        assert_eq!(
            meta.instructions.num_instructions,
            tx.message.instructions().len() as u16
        );
        assert_eq!(
            meta.address_table_lookup.num_address_table_lookup,
            tx.message
                .address_table_lookups()
                .map(|x| x.len() as u8)
                .unwrap_or(0)
        );
    }

    fn minimally_sized_transaction() -> VersionedTransaction {
        VersionedTransaction {
            signatures: vec![Signature::default()], // 1 signature to be valid.
            message: VersionedMessage::Legacy(Message {
                header: MessageHeader {
                    num_required_signatures: 1,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 0,
                },
                account_keys: vec![Pubkey::default()],
                recent_blockhash: Hash::default(),
                instructions: vec![],
            }),
        }
    }

    fn simple_transfer() -> VersionedTransaction {
        let payer = Pubkey::new_unique();
        VersionedTransaction {
            signatures: vec![Signature::default()], // 1 signature to be valid.
            message: VersionedMessage::Legacy(Message::new(
                &[system_instruction::transfer(
                    &payer,
                    &Pubkey::new_unique(),
                    1,
                )],
                Some(&payer),
            )),
        }
    }

    fn simple_transfer_v0() -> VersionedTransaction {
        let payer = Pubkey::new_unique();
        VersionedTransaction {
            signatures: vec![Signature::default()], // 1 signature to be valid.
            message: VersionedMessage::V0(
                v0::Message::try_compile(
                    &payer,
                    &[system_instruction::transfer(
                        &payer,
                        &Pubkey::new_unique(),
                        1,
                    )],
                    &[],
                    Hash::default(),
                )
                .unwrap(),
            ),
        }
    }

    fn v0_with_lookup() -> VersionedTransaction {
        let payer = Pubkey::new_unique();
        let to = Pubkey::new_unique();
        VersionedTransaction {
            signatures: vec![Signature::default()], // 1 signature to be valid.
            message: VersionedMessage::V0(
                v0::Message::try_compile(
                    &payer,
                    &[system_instruction::transfer(&payer, &to, 1)],
                    &[AddressLookupTableAccount {
                        key: Pubkey::new_unique(),
                        addresses: vec![to],
                    }],
                    Hash::default(),
                )
                .unwrap(),
            ),
        }
    }

    #[test]
    fn test_minimal_sized_transaction() {
        verify_transaction_view_meta(&minimally_sized_transaction());
    }

    #[test]
    fn test_simple_transfer() {
        verify_transaction_view_meta(&simple_transfer());
    }

    #[test]
    fn test_simple_transfer_v0() {
        verify_transaction_view_meta(&simple_transfer_v0());
    }

    #[test]
    fn test_v0_with_lookup() {
        verify_transaction_view_meta(&v0_with_lookup());
    }

    #[test]
    fn test_trailing_byte() {
        let tx = simple_transfer();
        let mut bytes = bincode::serialize(&tx).unwrap();
        bytes.push(0);
        assert!(TransactionMeta::try_new(&bytes).is_err());
    }

    #[test]
    fn test_insufficient_bytes() {
        let tx = simple_transfer();
        let bytes = bincode::serialize(&tx).unwrap();
        assert!(TransactionMeta::try_new(&bytes[..bytes.len().wrapping_sub(1)]).is_err());
    }

    #[test]
    fn test_signature_overflow() {
        let tx = simple_transfer();
        let mut bytes = bincode::serialize(&tx).unwrap();
        // Set the number of signatures to u16::MAX
        bytes[0] = 0xff;
        bytes[1] = 0xff;
        bytes[2] = 0xff;
        assert!(TransactionMeta::try_new(&bytes).is_err());
    }

    #[test]
    fn test_account_key_overflow() {
        let tx = simple_transfer();
        let mut bytes = bincode::serialize(&tx).unwrap();
        // Set the number of accounts to u16::MAX
        let offset = 1 + core::mem::size_of::<Signature>() + 3;
        bytes[offset] = 0xff;
        bytes[offset + 1] = 0xff;
        bytes[offset + 2] = 0xff;
        assert!(TransactionMeta::try_new(&bytes).is_err());
    }

    #[test]
    fn test_instructions_overflow() {
        let tx = simple_transfer();
        let mut bytes = bincode::serialize(&tx).unwrap();
        // Set the number of instructions to u16::MAX
        let offset = 1
            + core::mem::size_of::<Signature>()
            + 3
            + 1
            + 3 * core::mem::size_of::<Pubkey>()
            + core::mem::size_of::<Hash>();
        bytes[offset] = 0xff;
        bytes[offset + 1] = 0xff;
        bytes[offset + 2] = 0xff;
        assert!(TransactionMeta::try_new(&bytes).is_err());
    }

    #[test]
    fn test_alt_overflow() {
        let tx = simple_transfer_v0();
        let ix_bytes = tx.message.instructions()[0].data.len();
        let mut bytes = bincode::serialize(&tx).unwrap();
        // Set the number of instructions to u16::MAX
        let offset = 1 // byte for num signatures
            + core::mem::size_of::<Signature>() // signature
            + 1 // version byte
            + 3 // message header
            + 1 // byte for num account keys
            + 3 * core::mem::size_of::<Pubkey>() // account keys
            + core::mem::size_of::<Hash>() // recent blockhash
            + 1 // byte for num instructions
            + 1 // program index
            + 1 // byte for num accounts
            + 2 // bytes for account index
            + 1 // byte for data length
            + ix_bytes;
        bytes[offset] = 0x01;
        assert!(TransactionMeta::try_new(&bytes).is_err());
    }

    #[test]
    fn test_basic_accessors() {
        let tx = simple_transfer();
        let bytes = bincode::serialize(&tx).unwrap();
        let meta = TransactionMeta::try_new(&bytes).unwrap();

        assert_eq!(meta.num_signatures(), 1);
        assert!(matches!(meta.version(), TransactionVersion::Legacy));
        assert_eq!(meta.num_required_signatures(), 1);
        assert_eq!(meta.num_readonly_signed_accounts(), 0);
        assert_eq!(meta.num_readonly_unsigned_accounts(), 1);
        assert_eq!(meta.num_static_account_keys(), 3);
        assert_eq!(meta.num_instructions(), 1);
        assert_eq!(meta.num_address_table_lookups(), 0);

        // SAFETY: `bytes` is the same slice used to create `meta`.
        unsafe {
            let signatures = meta.signatures(&bytes);
            assert_eq!(signatures, &tx.signatures);

            let static_account_keys = meta.static_account_keys(&bytes);
            assert_eq!(static_account_keys, tx.message.static_account_keys());

            let recent_blockhash = meta.recent_blockhash(&bytes);
            assert_eq!(recent_blockhash, tx.message.recent_blockhash());
        }
    }

    #[test]
    fn test_instructions_iter() {
        let tx = simple_transfer();
        let bytes = bincode::serialize(&tx).unwrap();
        let meta = TransactionMeta::try_new(&bytes).unwrap();

        // SAFETY: `bytes` is the same slice used to create `meta`.
        unsafe {
            let mut iter = meta.instructions_iter(&bytes);
            let ix = iter.next().unwrap();
            assert_eq!(ix.program_id_index, 2);
            assert_eq!(ix.accounts, &[0, 1]);
            assert_eq!(
                ix.data,
                &bincode::serialize(&SystemInstruction::Transfer { lamports: 1 }).unwrap()
            );
            assert!(iter.next().is_none());
        }
    }
}
