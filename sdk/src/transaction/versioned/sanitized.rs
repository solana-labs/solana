use {
    super::VersionedTransaction,
    crate::{
        compute_budget_processor::process_compute_budget_instruction, feature_set::FeatureSet,
        sanitize::SanitizeError, signature::Signature, transaction_meta::TransactionMeta,
    },
    solana_program::message::SanitizedVersionedMessage,
};

/// Wraps a sanitized `VersionedTransaction` to provide a safe API
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SanitizedVersionedTransaction {
    /// List of signatures
    pub(crate) signatures: Vec<Signature>,
    /// Message to sign.
    pub(crate) message: SanitizedVersionedMessage,
    /// Meta data
    pub(crate) transaction_meta: TransactionMeta,
}

impl SanitizedVersionedTransaction {
    // TODO 0 - basic impl to require feature_set to create SVT from VT
    pub fn try_new(
        tx: VersionedTransaction,
        feature_set: &FeatureSet,
    ) -> Result<Self, SanitizeError> {
        tx.sanitize_signatures()?;
        let sanitized_versioned_message = SanitizedVersionedMessage::try_from(tx.message)?;
        let transaction_meta = process_compute_budget_instruction(
            sanitized_versioned_message.program_instructions_iter(),
            feature_set,
            None, //cluster type
        )?;
        // TODO 1 - big problem: now we need bank/feature_set to create SVT (and ST)?
        // A potential solution is to create a `builder` that constructed with a ref to bank_fork
        // or whatever can quickly provides ref to current bank, therefore its feature_set,
        // cluster_info etc that are necessary for process CB ix.
        // The `Builder` needs to be a singlton-kind, so anywhere in the code, such as from here
        // one can call:
        // let transaction_meta = TransactionMetaBuilder::get_transaction_meta(&sanitized_versioned_message)?;
        Ok(Self {
            signatures: tx.signatures,
            message: sanitized_versioned_message,
            transaction_meta,
        })
    }

    pub fn get_message(&self) -> &SanitizedVersionedMessage {
        &self.message
    }

    // TODO 2 - the second solution is to be lazy:
    // Transaction_meta is Option that init to None, being procoessed with
    // get_transaction_meta(&self, &feature_set) -> Result<TransactionMeta, SanitizeError>
    // The part I dislkikes are:
    // 1. now "sanitize" is done outside of "try_create", messy flow.
    // 2. worse: it takesd a `&mut self`
    /*
    pub fn get_transaction_meta(
        &mut self,
        feature_set: &FeatureSet,
    ) -> Result<&TransactionMeta, SanitizeError> {
        if self.transaction_meta.is_none() {
            self.transaction_meta =
        Some(process_compute_budget_instruction(
            self.get_message().program_instructions_iter(),
            feature_set,
            None,
        )?);
        }
        self.transaction_meta.ok_or()
    }
    // */

    // TODO - prefer solution #1 so I can have:
    //*
    pub fn get_transaction_meta(&self) -> &TransactionMeta {
        &self.transaction_meta
    }
    // */
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_program::{
            hash::Hash,
            message::{v0, VersionedMessage},
            pubkey::Pubkey,
        },
    };

    #[test]
    fn test_try_new_with_invalid_signatures() {
        let tx = VersionedTransaction {
            signatures: vec![],
            message: VersionedMessage::V0(
                v0::Message::try_compile(&Pubkey::new_unique(), &[], &[], Hash::default()).unwrap(),
            ),
        };

        assert_eq!(
            SanitizedVersionedTransaction::try_new(tx),
            Err(SanitizeError::IndexOutOfBounds)
        );
    }

    #[test]
    fn test_try_new() {
        let mut message =
            v0::Message::try_compile(&Pubkey::new_unique(), &[], &[], Hash::default()).unwrap();
        message.header.num_readonly_signed_accounts += 1;

        let tx = VersionedTransaction {
            signatures: vec![Signature::default()],
            message: VersionedMessage::V0(message),
        };

        assert_eq!(
            SanitizedVersionedTransaction::try_new(tx),
            Err(SanitizeError::InvalidValue)
        );
    }
}
