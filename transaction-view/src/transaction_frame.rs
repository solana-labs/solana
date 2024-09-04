use {
    crate::{
        address_table_lookup_frame::{AddressTableLookupFrame, AddressTableLookupIterator},
        bytes::advance_offset_for_type,
        instructions_frame::{InstructionsFrame, InstructionsIterator},
        message_header_frame::{MessageHeaderFrame, TransactionVersion},
        result::{Result, TransactionViewError},
        signature_frame::SignatureFrame,
        static_account_keys_frame::StaticAccountKeysFrame,
    },
    solana_sdk::{hash::Hash, pubkey::Pubkey, signature::Signature},
};

pub(crate) struct TransactionFrame {
    /// Signature framing data.
    signature: SignatureFrame,
    /// Message header framing data.
    message_header: MessageHeaderFrame,
    /// Static account keys framing data.
    static_account_keys: StaticAccountKeysFrame,
    /// Recent blockhash offset.
    recent_blockhash_offset: u16,
    /// Instructions framing data.
    instructions: InstructionsFrame,
    /// Address table lookup framing data.
    address_table_lookup: AddressTableLookupFrame,
}

impl TransactionFrame {
    /// Parse a serialized transaction and verify basic structure.
    /// The `bytes` parameter must have no trailing data.
    pub(crate) fn try_new(bytes: &[u8]) -> Result<Self> {
        let mut offset = 0;
        let signature = SignatureFrame::try_new(bytes, &mut offset)?;
        let message_header = MessageHeaderFrame::try_new(bytes, &mut offset)?;
        let static_account_keys = StaticAccountKeysFrame::try_new(bytes, &mut offset)?;

        // The recent blockhash is the first account key after the static
        // account keys. The recent blockhash is always present in a valid
        // transaction and has a fixed size of 32 bytes.
        let recent_blockhash_offset = offset as u16;
        advance_offset_for_type::<Hash>(bytes, &mut offset)?;

        let instructions = InstructionsFrame::try_new(bytes, &mut offset)?;
        let address_table_lookup = match message_header.version {
            TransactionVersion::Legacy => AddressTableLookupFrame {
                num_address_table_lookups: 0,
                offset: 0,
                total_writable_lookup_accounts: 0,
                total_readonly_lookup_accounts: 0,
            },
            TransactionVersion::V0 => AddressTableLookupFrame::try_new(bytes, &mut offset)?,
        };

        // Verify that the entire transaction was parsed.
        if offset != bytes.len() {
            return Err(TransactionViewError::ParseError);
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
    #[inline]
    pub(crate) fn num_signatures(&self) -> u8 {
        self.signature.num_signatures
    }

    /// Return the version of the transaction.
    #[inline]
    pub(crate) fn version(&self) -> TransactionVersion {
        self.message_header.version
    }

    /// Return the number of required signatures in the transaction.
    #[inline]
    pub(crate) fn num_required_signatures(&self) -> u8 {
        self.message_header.num_required_signatures
    }

    /// Return the number of readonly signed accounts in the transaction.
    #[inline]
    pub(crate) fn num_readonly_signed_accounts(&self) -> u8 {
        self.message_header.num_readonly_signed_accounts
    }

    /// Return the number of readonly unsigned accounts in the transaction.
    #[inline]
    pub(crate) fn num_readonly_unsigned_accounts(&self) -> u8 {
        self.message_header.num_readonly_unsigned_accounts
    }

    /// Return the number of static account keys in the transaction.
    #[inline]
    pub(crate) fn num_static_account_keys(&self) -> u8 {
        self.static_account_keys.num_static_accounts
    }

    /// Return the number of instructions in the transaction.
    #[inline]
    pub(crate) fn num_instructions(&self) -> u16 {
        self.instructions.num_instructions
    }

    /// Return the number of address table lookups in the transaction.
    #[inline]
    pub(crate) fn num_address_table_lookups(&self) -> u8 {
        self.address_table_lookup.num_address_table_lookups
    }

    /// Return the number of writable lookup accounts in the transaction.
    #[inline]
    pub(crate) fn total_writable_lookup_accounts(&self) -> u16 {
        self.address_table_lookup.total_writable_lookup_accounts
    }

    /// Return the number of readonly lookup accounts in the transaction.
    #[inline]
    pub(crate) fn total_readonly_lookup_accounts(&self) -> u16 {
        self.address_table_lookup.total_readonly_lookup_accounts
    }

    /// Return the offset to the message.
    #[inline]
    pub(crate) fn message_offset(&self) -> u16 {
        self.message_header.offset
    }
}

// Separate implementation for `unsafe` accessor methods.
impl TransactionFrame {
    /// Return the slice of signatures in the transaction.
    /// # Safety
    ///   - This function must be called with the same `bytes` slice that was
    ///     used to create the `TransactionFrame` instance.
    #[inline]
    pub(crate) unsafe fn signatures<'a>(&self, bytes: &'a [u8]) -> &'a [Signature] {
        // Verify at compile time there are no alignment constraints.
        const _: () = assert!(
            core::mem::align_of::<Signature>() == 1,
            "Signature alignment"
        );
        // The length of the slice is not greater than isize::MAX.
        const _: () =
            assert!(u8::MAX as usize * core::mem::size_of::<Signature>() <= isize::MAX as usize);

        // SAFETY:
        // - If this `TransactionFrame` was created from `bytes`:
        //     - the pointer is valid for the range and is properly aligned.
        // - `num_signatures` has been verified against the bounds if
        //   `TransactionFrame` was created successfully.
        // - `Signature` are just byte arrays; there is no possibility the
        //   `Signature` are not initialized properly.
        // - The lifetime of the returned slice is the same as the input
        //   `bytes`. This means it will not be mutated or deallocated while
        //   holding the slice.
        // - The length does not overflow `isize`.
        core::slice::from_raw_parts(
            bytes.as_ptr().add(usize::from(self.signature.offset)) as *const Signature,
            usize::from(self.signature.num_signatures),
        )
    }

    /// Return the slice of static account keys in the transaction.
    ///
    /// # Safety
    ///  - This function must be called with the same `bytes` slice that was
    ///    used to create the `TransactionFrame` instance.
    #[inline]
    pub(crate) unsafe fn static_account_keys<'a>(&self, bytes: &'a [u8]) -> &'a [Pubkey] {
        // Verify at compile time there are no alignment constraints.
        const _: () = assert!(core::mem::align_of::<Pubkey>() == 1, "Pubkey alignment");
        // The length of the slice is not greater than isize::MAX.
        const _: () =
            assert!(u8::MAX as usize * core::mem::size_of::<Pubkey>() <= isize::MAX as usize);

        // SAFETY:
        // - If this `TransactionFrame` was created from `bytes`:
        //     - the pointer is valid for the range and is properly aligned.
        // - `num_static_accounts` has been verified against the bounds if
        //   `TransactionFrame` was created successfully.
        // - `Pubkey` are just byte arrays; there is no possibility the
        //   `Pubkey` are not initialized properly.
        // - The lifetime of the returned slice is the same as the input
        //   `bytes`. This means it will not be mutated or deallocated while
        //   holding the slice.
        // - The length does not overflow `isize`.
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
    ///   used to create the `TransactionFrame` instance.
    #[inline]
    pub(crate) unsafe fn recent_blockhash<'a>(&self, bytes: &'a [u8]) -> &'a Hash {
        // Verify at compile time there are no alignment constraints.
        const _: () = assert!(core::mem::align_of::<Hash>() == 1, "Hash alignment");

        // SAFETY:
        // - The pointer is correctly aligned (no alignment constraints).
        // - `Hash` is just a byte array; there is no possibility the `Hash`
        //   is not initialized properly.
        // - Aliasing rules are respected because the lifetime of the returned
        //   reference is the same as the input/source `bytes`.
        &*(bytes
            .as_ptr()
            .add(usize::from(self.recent_blockhash_offset)) as *const Hash)
    }

    /// Return an iterator over the instructions in the transaction.
    /// # Safety
    /// - This function must be called with the same `bytes` slice that was
    ///   used to create the `TransactionFrame` instance.
    #[inline]
    pub(crate) unsafe fn instructions_iter<'a>(&self, bytes: &'a [u8]) -> InstructionsIterator<'a> {
        InstructionsIterator {
            bytes,
            offset: usize::from(self.instructions.offset),
            num_instructions: self.instructions.num_instructions,
            index: 0,
        }
    }

    /// Return an iterator over the address table lookups in the transaction.
    /// # Safety
    /// - This function must be called with the same `bytes` slice that was
    ///   used to create the `TransactionFrame` instance.
    #[inline]
    pub(crate) unsafe fn address_table_lookup_iter<'a>(
        &self,
        bytes: &'a [u8],
    ) -> AddressTableLookupIterator<'a> {
        AddressTableLookupIterator {
            bytes,
            offset: usize::from(self.address_table_lookup.offset),
            num_address_table_lookups: self.address_table_lookup.num_address_table_lookups,
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

    fn verify_transaction_view_frame(tx: &VersionedTransaction) {
        let bytes = bincode::serialize(tx).unwrap();
        let frame = TransactionFrame::try_new(&bytes).unwrap();

        assert_eq!(frame.signature.num_signatures, tx.signatures.len() as u8);
        assert_eq!(frame.signature.offset as usize, 1);

        assert_eq!(
            frame.message_header.num_required_signatures,
            tx.message.header().num_required_signatures
        );
        assert_eq!(
            frame.message_header.num_readonly_signed_accounts,
            tx.message.header().num_readonly_signed_accounts
        );
        assert_eq!(
            frame.message_header.num_readonly_unsigned_accounts,
            tx.message.header().num_readonly_unsigned_accounts
        );

        assert_eq!(
            frame.static_account_keys.num_static_accounts,
            tx.message.static_account_keys().len() as u8
        );
        assert_eq!(
            frame.instructions.num_instructions,
            tx.message.instructions().len() as u16
        );
        assert_eq!(
            frame.address_table_lookup.num_address_table_lookups,
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

    fn v0_with_single_lookup() -> VersionedTransaction {
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

    fn v0_with_multiple_lookups() -> VersionedTransaction {
        let payer = Pubkey::new_unique();
        let to1 = Pubkey::new_unique();
        let to2 = Pubkey::new_unique();
        VersionedTransaction {
            signatures: vec![Signature::default()], // 1 signature to be valid.
            message: VersionedMessage::V0(
                v0::Message::try_compile(
                    &payer,
                    &[
                        system_instruction::transfer(&payer, &to1, 1),
                        system_instruction::transfer(&payer, &to2, 1),
                    ],
                    &[
                        AddressLookupTableAccount {
                            key: Pubkey::new_unique(),
                            addresses: vec![to1],
                        },
                        AddressLookupTableAccount {
                            key: Pubkey::new_unique(),
                            addresses: vec![to2],
                        },
                    ],
                    Hash::default(),
                )
                .unwrap(),
            ),
        }
    }

    #[test]
    fn test_minimal_sized_transaction() {
        verify_transaction_view_frame(&minimally_sized_transaction());
    }

    #[test]
    fn test_simple_transfer() {
        verify_transaction_view_frame(&simple_transfer());
    }

    #[test]
    fn test_simple_transfer_v0() {
        verify_transaction_view_frame(&simple_transfer_v0());
    }

    #[test]
    fn test_v0_with_lookup() {
        verify_transaction_view_frame(&v0_with_single_lookup());
    }

    #[test]
    fn test_trailing_byte() {
        let tx = simple_transfer();
        let mut bytes = bincode::serialize(&tx).unwrap();
        bytes.push(0);
        assert!(TransactionFrame::try_new(&bytes).is_err());
    }

    #[test]
    fn test_insufficient_bytes() {
        let tx = simple_transfer();
        let bytes = bincode::serialize(&tx).unwrap();
        assert!(TransactionFrame::try_new(&bytes[..bytes.len().wrapping_sub(1)]).is_err());
    }

    #[test]
    fn test_signature_overflow() {
        let tx = simple_transfer();
        let mut bytes = bincode::serialize(&tx).unwrap();
        // Set the number of signatures to u16::MAX
        bytes[0] = 0xff;
        bytes[1] = 0xff;
        bytes[2] = 0xff;
        assert!(TransactionFrame::try_new(&bytes).is_err());
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
        assert!(TransactionFrame::try_new(&bytes).is_err());
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
        assert!(TransactionFrame::try_new(&bytes).is_err());
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
        assert!(TransactionFrame::try_new(&bytes).is_err());
    }

    #[test]
    fn test_basic_accessors() {
        let tx = simple_transfer();
        let bytes = bincode::serialize(&tx).unwrap();
        let frame = TransactionFrame::try_new(&bytes).unwrap();

        assert_eq!(frame.num_signatures(), 1);
        assert!(matches!(frame.version(), TransactionVersion::Legacy));
        assert_eq!(frame.num_required_signatures(), 1);
        assert_eq!(frame.num_readonly_signed_accounts(), 0);
        assert_eq!(frame.num_readonly_unsigned_accounts(), 1);
        assert_eq!(frame.num_static_account_keys(), 3);
        assert_eq!(frame.num_instructions(), 1);
        assert_eq!(frame.num_address_table_lookups(), 0);

        // SAFETY: `bytes` is the same slice used to create `frame`.
        unsafe {
            let signatures = frame.signatures(&bytes);
            assert_eq!(signatures, &tx.signatures);

            let static_account_keys = frame.static_account_keys(&bytes);
            assert_eq!(static_account_keys, tx.message.static_account_keys());

            let recent_blockhash = frame.recent_blockhash(&bytes);
            assert_eq!(recent_blockhash, tx.message.recent_blockhash());
        }
    }

    #[test]
    fn test_instructions_iter_empty() {
        let tx = minimally_sized_transaction();
        let bytes = bincode::serialize(&tx).unwrap();
        let frame = TransactionFrame::try_new(&bytes).unwrap();

        // SAFETY: `bytes` is the same slice used to create `frame`.
        unsafe {
            let mut iter = frame.instructions_iter(&bytes);
            assert!(iter.next().is_none());
        }
    }

    #[test]
    fn test_instructions_iter_single() {
        let tx = simple_transfer();
        let bytes = bincode::serialize(&tx).unwrap();
        let frame = TransactionFrame::try_new(&bytes).unwrap();

        // SAFETY: `bytes` is the same slice used to create `frame`.
        unsafe {
            let mut iter = frame.instructions_iter(&bytes);
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

    #[test]
    fn test_instructions_iter_multiple() {
        let tx = multiple_transfers();
        let bytes = bincode::serialize(&tx).unwrap();
        let frame = TransactionFrame::try_new(&bytes).unwrap();

        // SAFETY: `bytes` is the same slice used to create `frame`.
        unsafe {
            let mut iter = frame.instructions_iter(&bytes);
            let ix = iter.next().unwrap();
            assert_eq!(ix.program_id_index, 3);
            assert_eq!(ix.accounts, &[0, 1]);
            assert_eq!(
                ix.data,
                &bincode::serialize(&SystemInstruction::Transfer { lamports: 1 }).unwrap()
            );
            let ix = iter.next().unwrap();
            assert_eq!(ix.program_id_index, 3);
            assert_eq!(ix.accounts, &[0, 2]);
            assert_eq!(
                ix.data,
                &bincode::serialize(&SystemInstruction::Transfer { lamports: 1 }).unwrap()
            );
            assert!(iter.next().is_none());
        }
    }

    #[test]
    fn test_address_table_lookup_iter_empty() {
        let tx = simple_transfer();
        let bytes = bincode::serialize(&tx).unwrap();
        let frame = TransactionFrame::try_new(&bytes).unwrap();

        // SAFETY: `bytes` is the same slice used to create `frame`.
        unsafe {
            let mut iter = frame.address_table_lookup_iter(&bytes);
            assert!(iter.next().is_none());
        }
    }

    #[test]
    fn test_address_table_lookup_iter_single() {
        let tx = v0_with_single_lookup();
        let bytes = bincode::serialize(&tx).unwrap();
        let frame = TransactionFrame::try_new(&bytes).unwrap();

        let atls_actual = tx.message.address_table_lookups().unwrap();
        // SAFETY: `bytes` is the same slice used to create `frame`.
        unsafe {
            let mut iter = frame.address_table_lookup_iter(&bytes);
            let lookup = iter.next().unwrap();
            assert_eq!(lookup.account_key, &atls_actual[0].account_key);
            assert_eq!(lookup.writable_indexes, atls_actual[0].writable_indexes);
            assert_eq!(lookup.readonly_indexes, atls_actual[0].readonly_indexes);
            assert!(iter.next().is_none());
        }
    }

    #[test]
    fn test_address_table_lookup_iter_multiple() {
        let tx = v0_with_multiple_lookups();
        let bytes = bincode::serialize(&tx).unwrap();
        let frame = TransactionFrame::try_new(&bytes).unwrap();

        let atls_actual = tx.message.address_table_lookups().unwrap();
        // SAFETY: `bytes` is the same slice used to create `frame`.
        unsafe {
            let mut iter = frame.address_table_lookup_iter(&bytes);

            let lookup = iter.next().unwrap();
            assert_eq!(lookup.account_key, &atls_actual[0].account_key);
            assert_eq!(lookup.writable_indexes, atls_actual[0].writable_indexes);
            assert_eq!(lookup.readonly_indexes, atls_actual[0].readonly_indexes);

            let lookup = iter.next().unwrap();
            assert_eq!(lookup.account_key, &atls_actual[1].account_key);
            assert_eq!(lookup.writable_indexes, atls_actual[1].writable_indexes);
            assert_eq!(lookup.readonly_indexes, atls_actual[1].readonly_indexes);

            assert!(iter.next().is_none());
        }
    }
}
