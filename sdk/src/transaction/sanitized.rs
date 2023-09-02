#![cfg(feature = "full")]

pub use crate::message::{AddressLoader, SimpleAddressLoader};
use {
    super::SanitizedVersionedTransaction,
    crate::{
        hash::Hash,
        message::{
            legacy,
            v0::{self, LoadedAddresses},
            LegacyMessage, SanitizedMessage, VersionedMessage,
        },
        precompiles::verify_if_precompile,
        pubkey::Pubkey,
        sanitize::Sanitize,
        signature::Signature,
        solana_sdk::feature_set,
        transaction::{Result, Transaction, TransactionError, VersionedTransaction},
    },
    solana_program::message::SanitizedVersionedMessage,
};

/// Maximum number of accounts that a transaction may lock.
/// 128 was chosen because it is the minimum number of accounts
/// needed for the Neon EVM implementation.
pub const MAX_TX_ACCOUNT_LOCKS: usize = 128;

/// Sanitized transaction and the hash of its message
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SanitizedTransaction {
    message: SanitizedMessage,
    message_hash: Hash,
    is_simple_vote_tx: bool,
    signatures: Vec<Signature>,
}

/// Set of accounts that must be locked for safe transaction processing
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct TransactionAccountLocks<'a> {
    /// List of readonly account key locks
    pub readonly: Vec<&'a Pubkey>,
    /// List of writable account key locks
    pub writable: Vec<&'a Pubkey>,
}

/// Type that represents whether the transaction message has been precomputed or
/// not.
pub enum MessageHash {
    Precomputed(Hash),
    Compute,
}

impl From<Hash> for MessageHash {
    fn from(hash: Hash) -> Self {
        Self::Precomputed(hash)
    }
}

impl SanitizedTransaction {
    /// Create a sanitized transaction from a sanitized versioned transaction.
    /// If the input transaction uses address tables, attempt to lookup the
    /// address for each table index.
    pub fn try_new(
        tx: SanitizedVersionedTransaction,
        message_hash: Hash,
        is_simple_vote_tx: bool,
        address_loader: impl AddressLoader,
    ) -> Result<Self> {
        let signatures = tx.signatures;
        let SanitizedVersionedMessage { message } = tx.message;
        let message = match message {
            VersionedMessage::Legacy(message) => {
                SanitizedMessage::Legacy(LegacyMessage::new(message))
            }
            VersionedMessage::V0(message) => {
                let loaded_addresses =
                    address_loader.load_addresses(&message.address_table_lookups)?;
                SanitizedMessage::V0(v0::LoadedMessage::new(message, loaded_addresses))
            }
        };

        Ok(Self {
            message,
            message_hash,
            is_simple_vote_tx,
            signatures,
        })
    }

    /// Create a sanitized transaction from an un-sanitized versioned
    /// transaction.  If the input transaction uses address tables, attempt to
    /// lookup the address for each table index.
    pub fn try_create(
        tx: VersionedTransaction,
        message_hash: impl Into<MessageHash>,
        is_simple_vote_tx: Option<bool>,
        address_loader: impl AddressLoader,
    ) -> Result<Self> {
        tx.sanitize()?;

        let message_hash = match message_hash.into() {
            MessageHash::Compute => tx.message.hash(),
            MessageHash::Precomputed(hash) => hash,
        };

        let signatures = tx.signatures;
        let message = match tx.message {
            VersionedMessage::Legacy(message) => {
                SanitizedMessage::Legacy(LegacyMessage::new(message))
            }
            VersionedMessage::V0(message) => {
                let loaded_addresses =
                    address_loader.load_addresses(&message.address_table_lookups)?;
                SanitizedMessage::V0(v0::LoadedMessage::new(message, loaded_addresses))
            }
        };

        let is_simple_vote_tx = is_simple_vote_tx.unwrap_or_else(|| {
            if signatures.len() < 3
                && message.instructions().len() == 1
                && matches!(message, SanitizedMessage::Legacy(_))
            {
                let mut ix_iter = message.program_instructions_iter();
                ix_iter.next().map(|(program_id, _ix)| program_id)
                    == Some(&crate::vote::program::id())
            } else {
                false
            }
        });

        Ok(Self {
            message,
            message_hash,
            is_simple_vote_tx,
            signatures,
        })
    }

    pub fn try_from_legacy_transaction(tx: Transaction) -> Result<Self> {
        tx.sanitize()?;

        Ok(Self {
            message_hash: tx.message.hash(),
            message: SanitizedMessage::Legacy(LegacyMessage::new(tx.message)),
            is_simple_vote_tx: false,
            signatures: tx.signatures,
        })
    }

    /// Create a sanitized transaction from a legacy transaction. Used for tests only.
    pub fn from_transaction_for_tests(tx: Transaction) -> Self {
        Self::try_from_legacy_transaction(tx).unwrap()
    }

    /// Return the first signature for this transaction.
    ///
    /// Notes:
    ///
    /// Sanitized transactions must have at least one signature because the
    /// number of signatures must be greater than or equal to the message header
    /// value `num_required_signatures` which must be greater than 0 itself.
    pub fn signature(&self) -> &Signature {
        &self.signatures[0]
    }

    /// Return the list of signatures for this transaction
    pub fn signatures(&self) -> &[Signature] {
        &self.signatures
    }

    /// Return the signed message
    pub fn message(&self) -> &SanitizedMessage {
        &self.message
    }

    /// Return the hash of the signed message
    pub fn message_hash(&self) -> &Hash {
        &self.message_hash
    }

    /// Returns true if this transaction is a simple vote
    pub fn is_simple_vote_transaction(&self) -> bool {
        self.is_simple_vote_tx
    }

    /// Convert this sanitized transaction into a versioned transaction for
    /// recording in the ledger.
    pub fn to_versioned_transaction(&self) -> VersionedTransaction {
        let signatures = self.signatures.clone();
        match &self.message {
            SanitizedMessage::V0(sanitized_msg) => VersionedTransaction {
                signatures,
                message: VersionedMessage::V0(v0::Message::clone(&sanitized_msg.message)),
            },
            SanitizedMessage::Legacy(legacy_message) => VersionedTransaction {
                signatures,
                message: VersionedMessage::Legacy(legacy::Message::clone(&legacy_message.message)),
            },
        }
    }

    /// Validate and return the account keys locked by this transaction
    pub fn get_account_locks(
        &self,
        tx_account_lock_limit: usize,
    ) -> Result<TransactionAccountLocks> {
        Self::validate_account_locks(self.message(), tx_account_lock_limit)?;
        Ok(self.get_account_locks_unchecked())
    }

    /// Return the list of accounts that must be locked during processing this transaction.
    pub fn get_account_locks_unchecked(&self) -> TransactionAccountLocks {
        let message = &self.message;
        let account_keys = message.account_keys();
        let num_readonly_accounts = message.num_readonly_accounts();
        let num_writable_accounts = account_keys.len().saturating_sub(num_readonly_accounts);

        let mut account_locks = TransactionAccountLocks {
            writable: Vec::with_capacity(num_writable_accounts),
            readonly: Vec::with_capacity(num_readonly_accounts),
        };

        for (i, key) in account_keys.iter().enumerate() {
            if message.is_writable(i) {
                account_locks.writable.push(key);
            } else {
                account_locks.readonly.push(key);
            }
        }

        account_locks
    }

    /// Return the list of addresses loaded from on-chain address lookup tables
    pub fn get_loaded_addresses(&self) -> LoadedAddresses {
        match &self.message {
            SanitizedMessage::Legacy(_) => LoadedAddresses::default(),
            SanitizedMessage::V0(message) => LoadedAddresses::clone(&message.loaded_addresses),
        }
    }

    /// If the transaction uses a durable nonce, return the pubkey of the nonce account
    pub fn get_durable_nonce(&self) -> Option<&Pubkey> {
        self.message.get_durable_nonce()
    }

    /// Return the serialized message data to sign.
    fn message_data(&self) -> Vec<u8> {
        match &self.message {
            SanitizedMessage::Legacy(legacy_message) => legacy_message.message.serialize(),
            SanitizedMessage::V0(loaded_msg) => loaded_msg.message.serialize(),
        }
    }

    /// Verify the transaction signatures
    pub fn verify(&self) -> Result<()> {
        let message_bytes = self.message_data();
        if self
            .signatures
            .iter()
            .zip(self.message.account_keys().iter())
            .map(|(signature, pubkey)| signature.verify(pubkey.as_ref(), &message_bytes))
            .any(|verified| !verified)
        {
            Err(TransactionError::SignatureFailure)
        } else {
            Ok(())
        }
    }

    /// Verify the precompiled programs in this transaction
    pub fn verify_precompiles(&self, feature_set: &feature_set::FeatureSet) -> Result<()> {
        for (program_id, instruction) in self.message.program_instructions_iter() {
            verify_if_precompile(
                program_id,
                instruction,
                self.message().instructions(),
                feature_set,
            )
            .map_err(|_| TransactionError::InvalidAccountIndex)?;
        }
        Ok(())
    }

    /// Validate a transaction message against locked accounts
    pub fn validate_account_locks(
        message: &SanitizedMessage,
        tx_account_lock_limit: usize,
    ) -> Result<()> {
        if message.has_duplicates() {
            Err(TransactionError::AccountLoadedTwice)
        } else if message.account_keys().len() > tx_account_lock_limit {
            Err(TransactionError::TooManyAccountLocks)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects)]
mod tests {
    use {
        super::*,
        crate::signer::{keypair::Keypair, Signer},
        solana_program::vote::{self, state::Vote},
    };

    #[test]
    fn test_try_create_simple_vote_tx() {
        let bank_hash = Hash::default();
        let block_hash = Hash::default();
        let vote_keypair = Keypair::new();
        let node_keypair = Keypair::new();
        let auth_keypair = Keypair::new();
        let votes = Vote::new(vec![1, 2, 3], bank_hash);
        let vote_ix =
            vote::instruction::vote(&vote_keypair.pubkey(), &auth_keypair.pubkey(), votes);
        let mut vote_tx = Transaction::new_with_payer(&[vote_ix], Some(&node_keypair.pubkey()));
        vote_tx.partial_sign(&[&node_keypair], block_hash);
        vote_tx.partial_sign(&[&auth_keypair], block_hash);

        // single legacy vote ix, 2 signatures
        {
            let vote_transaction = SanitizedTransaction::try_create(
                VersionedTransaction::from(vote_tx.clone()),
                MessageHash::Compute,
                None,
                SimpleAddressLoader::Disabled,
            )
            .unwrap();
            assert!(vote_transaction.is_simple_vote_transaction());
        }

        {
            // call side says it is not a vote
            let vote_transaction = SanitizedTransaction::try_create(
                VersionedTransaction::from(vote_tx.clone()),
                MessageHash::Compute,
                Some(false),
                SimpleAddressLoader::Disabled,
            )
            .unwrap();
            assert!(!vote_transaction.is_simple_vote_transaction());
        }

        // single legacy vote ix, 3 signatures
        vote_tx.signatures.push(Signature::default());
        vote_tx.message.header.num_required_signatures = 3;
        {
            let vote_transaction = SanitizedTransaction::try_create(
                VersionedTransaction::from(vote_tx.clone()),
                MessageHash::Compute,
                None,
                SimpleAddressLoader::Disabled,
            )
            .unwrap();
            assert!(!vote_transaction.is_simple_vote_transaction());
        }

        {
            // call site says it is simple vote
            let vote_transaction = SanitizedTransaction::try_create(
                VersionedTransaction::from(vote_tx),
                MessageHash::Compute,
                Some(true),
                SimpleAddressLoader::Disabled,
            )
            .unwrap();
            assert!(vote_transaction.is_simple_vote_transaction());
        }
    }
}
