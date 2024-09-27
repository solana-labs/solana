use {
    crate::{
        address_table_lookup_frame::AddressTableLookupIterator,
        instructions_frame::InstructionsIterator, message_header_frame::TransactionVersion,
        result::Result, sanitize::sanitize, transaction_data::TransactionData,
        transaction_frame::TransactionFrame,
    },
    core::fmt::{Debug, Formatter},
    solana_sdk::{hash::Hash, pubkey::Pubkey, signature::Signature},
    solana_svm_transaction::instruction::SVMInstruction,
};

// alias for convenience
pub type UnsanitizedTransactionView<D> = TransactionView<false, D>;
pub type SanitizedTransactionView<D> = TransactionView<true, D>;

/// A view into a serialized transaction.
///
/// This struct provides access to the transaction data without
/// deserializing it. This is done by parsing and caching metadata
/// about the layout of the serialized transaction.
/// The owned `data` is abstracted through the `TransactionData` trait,
/// so that different containers for the serialized transaction can be used.
pub struct TransactionView<const SANITIZED: bool, D: TransactionData> {
    data: D,
    frame: TransactionFrame,
}

impl<D: TransactionData> TransactionView<false, D> {
    /// Creates a new `TransactionView` without running sanitization checks.
    pub fn try_new_unsanitized(data: D) -> Result<Self> {
        let frame = TransactionFrame::try_new(data.data())?;
        Ok(Self { data, frame })
    }

    /// Sanitizes the transaction view, returning a sanitized view on success.
    pub fn sanitize(self) -> Result<SanitizedTransactionView<D>> {
        sanitize(&self)?;
        Ok(SanitizedTransactionView {
            data: self.data,
            frame: self.frame,
        })
    }
}

impl<D: TransactionData> TransactionView<true, D> {
    /// Creates a new `TransactionView`, running sanitization checks.
    pub fn try_new_sanitized(data: D) -> Result<Self> {
        let unsanitized_view = TransactionView::try_new_unsanitized(data)?;
        unsanitized_view.sanitize()
    }
}

impl<const SANITIZED: bool, D: TransactionData> TransactionView<SANITIZED, D> {
    /// Return the number of signatures in the transaction.
    #[inline]
    pub fn num_signatures(&self) -> u8 {
        self.frame.num_signatures()
    }

    /// Return the version of the transaction.
    #[inline]
    pub fn version(&self) -> TransactionVersion {
        self.frame.version()
    }

    /// Return the number of required signatures in the transaction.
    #[inline]
    pub fn num_required_signatures(&self) -> u8 {
        self.frame.num_required_signatures()
    }

    /// Return the number of readonly signed static accounts in the transaction.
    #[inline]
    pub fn num_readonly_signed_static_accounts(&self) -> u8 {
        self.frame.num_readonly_signed_static_accounts()
    }

    /// Return the number of readonly unsigned static accounts in the transaction.
    #[inline]
    pub fn num_readonly_unsigned_static_accounts(&self) -> u8 {
        self.frame.num_readonly_unsigned_static_accounts()
    }

    /// Return the number of static account keys in the transaction.
    #[inline]
    pub fn num_static_account_keys(&self) -> u8 {
        self.frame.num_static_account_keys()
    }

    /// Return the number of instructions in the transaction.
    #[inline]
    pub fn num_instructions(&self) -> u16 {
        self.frame.num_instructions()
    }

    /// Return the number of address table lookups in the transaction.
    #[inline]
    pub fn num_address_table_lookups(&self) -> u8 {
        self.frame.num_address_table_lookups()
    }

    /// Return the number of writable lookup accounts in the transaction.
    #[inline]
    pub fn total_writable_lookup_accounts(&self) -> u16 {
        self.frame.total_writable_lookup_accounts()
    }

    /// Return the number of readonly lookup accounts in the transaction.
    #[inline]
    pub fn total_readonly_lookup_accounts(&self) -> u16 {
        self.frame.total_readonly_lookup_accounts()
    }

    /// Return the slice of signatures in the transaction.
    #[inline]
    pub fn signatures(&self) -> &[Signature] {
        let data = self.data();
        // SAFETY: `frame` was created from `data`.
        unsafe { self.frame.signatures(data) }
    }

    /// Return the slice of static account keys in the transaction.
    #[inline]
    pub fn static_account_keys(&self) -> &[Pubkey] {
        let data = self.data();
        // SAFETY: `frame` was created from `data`.
        unsafe { self.frame.static_account_keys(data) }
    }

    /// Return the recent blockhash in the transaction.
    #[inline]
    pub fn recent_blockhash(&self) -> &Hash {
        let data = self.data();
        // SAFETY: `frame` was created from `data`.
        unsafe { self.frame.recent_blockhash(data) }
    }

    /// Return an iterator over the instructions in the transaction.
    #[inline]
    pub fn instructions_iter(&self) -> InstructionsIterator {
        let data = self.data();
        // SAFETY: `frame` was created from `data`.
        unsafe { self.frame.instructions_iter(data) }
    }

    /// Return an iterator over the address table lookups in the transaction.
    #[inline]
    pub fn address_table_lookup_iter(&self) -> AddressTableLookupIterator {
        let data = self.data();
        // SAFETY: `frame` was created from `data`.
        unsafe { self.frame.address_table_lookup_iter(data) }
    }

    /// Return the full serialized transaction data.
    #[inline]
    pub fn data(&self) -> &[u8] {
        self.data.data()
    }

    /// Return the serialized **message** data.
    /// This does not include the signatures.
    #[inline]
    pub fn message_data(&self) -> &[u8] {
        &self.data()[usize::from(self.frame.message_offset())..]
    }
}

// Implementation that relies on sanitization checks having been run.
impl<D: TransactionData> TransactionView<true, D> {
    /// Return an iterator over the instructions paired with their program ids.
    pub fn program_instructions_iter(&self) -> impl Iterator<Item = (&Pubkey, SVMInstruction)> {
        self.instructions_iter().map(|ix| {
            let program_id_index = usize::from(ix.program_id_index);
            let program_id = &self.static_account_keys()[program_id_index];
            (program_id, ix)
        })
    }

    /// Return the number of unsigned static account keys.
    #[inline]
    pub(crate) fn num_static_unsigned_static_accounts(&self) -> u8 {
        self.num_static_account_keys()
            .wrapping_sub(self.num_required_signatures())
    }

    /// Return the number of writable unsigned static accounts.
    #[inline]
    pub(crate) fn num_writable_unsigned_static_accounts(&self) -> u8 {
        self.num_static_unsigned_static_accounts()
            .wrapping_sub(self.num_readonly_unsigned_static_accounts())
    }

    /// Return the number of writable unsigned static accounts.
    #[inline]
    pub(crate) fn num_writable_signed_static_accounts(&self) -> u8 {
        self.num_required_signatures()
            .wrapping_sub(self.num_readonly_signed_static_accounts())
    }
}

// Manual implementation of `Debug` - avoids bound on `D`.
// Prints nicely formatted struct-ish fields even for the iterator fields.
impl<const SANITIZED: bool, D: TransactionData> Debug for TransactionView<SANITIZED, D> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TransactionView")
            .field("frame", &self.frame)
            .field("signatures", &self.signatures())
            .field("static_account_keys", &self.static_account_keys())
            .field("recent_blockhash", &self.recent_blockhash())
            .field("instructions", &self.instructions_iter())
            .field("address_table_lookups", &self.address_table_lookup_iter())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{
            message::{Message, VersionedMessage},
            pubkey::Pubkey,
            signature::Signature,
            system_instruction::{self},
            transaction::VersionedTransaction,
        },
    };

    fn verify_transaction_view_frame(tx: &VersionedTransaction) {
        let bytes = bincode::serialize(tx).unwrap();
        let view = TransactionView::try_new_unsanitized(bytes.as_ref()).unwrap();

        assert_eq!(view.num_signatures(), tx.signatures.len() as u8);

        assert_eq!(
            view.num_required_signatures(),
            tx.message.header().num_required_signatures
        );
        assert_eq!(
            view.num_readonly_signed_static_accounts(),
            tx.message.header().num_readonly_signed_accounts
        );
        assert_eq!(
            view.num_readonly_unsigned_static_accounts(),
            tx.message.header().num_readonly_unsigned_accounts
        );

        assert_eq!(
            view.num_static_account_keys(),
            tx.message.static_account_keys().len() as u8
        );
        assert_eq!(
            view.num_instructions(),
            tx.message.instructions().len() as u16
        );
        assert_eq!(
            view.num_address_table_lookups(),
            tx.message
                .address_table_lookups()
                .map(|x| x.len() as u8)
                .unwrap_or(0)
        );
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

    #[test]
    fn test_multiple_transfers() {
        verify_transaction_view_frame(&multiple_transfers());
    }
}
