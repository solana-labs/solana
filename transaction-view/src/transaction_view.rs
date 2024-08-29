use {
    crate::{
        address_table_lookup_meta::AddressTableLookupIterator,
        instructions_meta::InstructionsIterator, message_header_meta::TransactionVersion,
        result::Result, sanitize::sanitize, transaction_data::TransactionData,
        transaction_meta::TransactionMeta,
    },
    solana_sdk::{hash::Hash, pubkey::Pubkey, signature::Signature},
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
    meta: TransactionMeta,
}

impl<D: TransactionData> TransactionView<false, D> {
    /// Creates a new `TransactionView` without running sanitization checks.
    pub fn try_new_unsanitized(data: D) -> Result<Self> {
        let meta = TransactionMeta::try_new(data.data())?;
        Ok(Self { data, meta })
    }

    /// Sanitizes the transaction view, returning a sanitized view on success.
    pub fn sanitize(self) -> Result<SanitizedTransactionView<D>> {
        sanitize(&self)?;
        Ok(SanitizedTransactionView {
            data: self.data,
            meta: self.meta,
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
    pub fn num_signatures(&self) -> u8 {
        self.meta.num_signatures()
    }

    /// Return the version of the transaction.
    pub fn version(&self) -> TransactionVersion {
        self.meta.version()
    }

    /// Return the number of required signatures in the transaction.
    pub fn num_required_signatures(&self) -> u8 {
        self.meta.num_required_signatures()
    }

    /// Return the number of readonly signed accounts in the transaction.
    pub fn num_readonly_signed_accounts(&self) -> u8 {
        self.meta.num_readonly_signed_accounts()
    }

    /// Return the number of readonly unsigned accounts in the transaction.
    pub fn num_readonly_unsigned_accounts(&self) -> u8 {
        self.meta.num_readonly_unsigned_accounts()
    }

    /// Return the number of static account keys in the transaction.
    pub fn num_static_account_keys(&self) -> u8 {
        self.meta.num_static_account_keys()
    }

    /// Return the number of instructions in the transaction.
    pub fn num_instructions(&self) -> u16 {
        self.meta.num_instructions()
    }

    /// Return the number of address table lookups in the transaction.
    pub fn num_address_table_lookups(&self) -> u8 {
        self.meta.num_address_table_lookups()
    }

    /// Return the number of writable lookup accounts in the transaction.
    pub fn total_writable_lookup_accounts(&self) -> u16 {
        self.meta.total_writable_lookup_accounts()
    }

    /// Return the number of readonly lookup accounts in the transaction.
    pub fn total_readonly_lookup_accounts(&self) -> u16 {
        self.meta.total_readonly_lookup_accounts()
    }

    /// Return the slice of signatures in the transaction.
    pub fn signatures(&self) -> &[Signature] {
        let data = self.data();
        // SAFETY: `meta` was created from `data`.
        unsafe { self.meta.signatures(data) }
    }

    /// Return the slice of static account keys in the transaction.
    pub fn static_account_keys(&self) -> &[Pubkey] {
        let data = self.data();
        // SAFETY: `meta` was created from `data`.
        unsafe { self.meta.static_account_keys(data) }
    }

    /// Return the recent blockhash in the transaction.
    pub fn recent_blockhash(&self) -> &Hash {
        let data = self.data();
        // SAFETY: `meta` was created from `data`.
        unsafe { self.meta.recent_blockhash(data) }
    }

    /// Return an iterator over the instructions in the transaction.
    pub fn instructions_iter(&self) -> InstructionsIterator {
        let data = self.data();
        // SAFETY: `meta` was created from `data`.
        unsafe { self.meta.instructions_iter(data) }
    }

    /// Return an iterator over the address table lookups in the transaction.
    pub fn address_table_lookup_iter(&self) -> AddressTableLookupIterator {
        let data = self.data();
        // SAFETY: `meta` was created from `data`.
        unsafe { self.meta.address_table_lookup_iter(data) }
    }

    /// Return the full serialized transaction data.
    #[inline]
    pub fn data(&self) -> &[u8] {
        self.data.data()
    }

    /// Return the serialized **message** data.
    /// This does not include the signatures.
    pub fn message_data(&self) -> &[u8] {
        &self.data()[usize::from(self.meta.message_offset())..]
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

    fn verify_transaction_view_meta(tx: &VersionedTransaction) {
        let bytes = bincode::serialize(tx).unwrap();
        let view = TransactionView::try_new_unsanitized(bytes.as_ref()).unwrap();

        assert_eq!(view.num_signatures(), tx.signatures.len() as u8);

        assert_eq!(
            view.num_required_signatures(),
            tx.message.header().num_required_signatures
        );
        assert_eq!(
            view.num_readonly_signed_accounts(),
            tx.message.header().num_readonly_signed_accounts
        );
        assert_eq!(
            view.num_readonly_unsigned_accounts(),
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
        verify_transaction_view_meta(&multiple_transfers());
    }
}
