use {
    crate::{
        simple_vote_transaction_checker::is_simple_vote_transaction, transaction_meta::TransactionMeta,
    },
    solana_sdk::{
        hash::Hash,
        message::{
            AddressLoader, SanitizedMessage, SanitizedVersionedMessage,
        },
        signature::Signature,
        transaction::{Result, SanitizedVersionedTransaction},
    },
};

/// RuntimeTransaction is runtime face representation of transaction, as
/// SanitizedTransaction is client facing rep.
/// The lifetime of RuntimeTransaction within banking_stage is roughly this: 
/// 1. Receive `packet` from sigverify, deserialize it into i`solana_sdk::VersioedTransaction`
/// 2. sanitize into `solana_sdk::SanitizedVersionedTransaction`
/// 3. create `RuntimeTransaction` from it, which does:
///    3.1 sets `RuntimeMessage` to `StaticallyLoaded` state
///    3.2 extracts static meta: message hash, simple vote flag, compute budget limits, payer account etc
///        Perhaps evencalculates transaction cost at this point (need upgrade cost-model a bit)
/// 4. banking_stage/scheduler process `RuntimeTransaction` with its static metadata, untill when
///    it becomes necessary to call `load_addresses(...)` load accounts address from on-chain ALT
///    4.1 `RuntimeMessage` transits to `DynamicallyLoaded` state
///    4.2 extracts dynamic metadata, such as nonce account etc
/// 5. `RuntimeTransaction` is ready for "load_and_execute"
///

/// runtime message has two stats:
/// 1. Initial state when message
/// is created from solana_sdk::SanitizedVersionedTransaction,
/// which is a super-light-wieghted op; During this state,
/// static metadata (eg those solely based on message itself)
/// can be cheaply resolved and used.
/// 2. DynamicallyLoaded state is when account addresses loaded from on-chain
/// address-lookup-table, a operation involves loading accounts from
/// accounts db, deserialize accounts' data. During this state,
/// accounts related metadata can be resolved, such as Nonce account,
/// transaction costs, and transaction (base) fee
///
/// Following events can transit runtime message from Initial state to DynamicallyLoaded state:
/// 1. `pub fn load_addresses(address_loader)` is called, or
///
/// New Error:
/// If Getter of account-based metadata is called while in Initial state,
/// a new Error shall return to indicate RuntimeTransaction is not
/// furnished with necessary data yet.
///
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub enum RuntimeMessage {
    #[default]
    Uninitialized,
    StaticallyLoaded(SanitizedVersionedMessage),
    DynamicallyLoaded(SanitizedMessage),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RuntimeTransaction {
    // sanitized signatures
    signatures: Vec<Signature>,

    // sanitized message that can be in static/dynamic state
    message: RuntimeMessage,

    // transaction meta is a collection of fields, it is updated
    // during message state transition
    meta: TransactionMeta,
}

impl Default for RuntimeTransaction {
    fn default() -> Self {
        Self {
            signatures: vec![],
            message: RuntimeMessage::Uninitialized,
            meta: TransactionMeta::default(),
        }
    }
}

impl RuntimeTransaction {
    pub fn try_from(
        sanitized_versioned_tx: SanitizedVersionedTransaction,
        message_hash: Option<Hash>,
        is_simple_vote_tx: Option<bool>,
    ) -> Result<Self> {
        let mut tx = RuntimeTransaction::default();
        tx.to_statically_loaded(sanitized_versioned_tx, message_hash, is_simple_vote_tx)?;
        Ok(tx)
    }

    // transit from Uninitialized to StaticallyLoaded state, where static metadata are available
    pub fn to_statically_loaded(
        &mut self,
        sanitized_versioned_tx: SanitizedVersionedTransaction,
        message_hash: Option<Hash>,
        is_simple_vote_tx: Option<bool>,
    ) -> Result<()> {
        if let RuntimeMessage::Uninitialized = self.message {
            self.load_static_metadata(&sanitized_versioned_tx, message_hash, is_simple_vote_tx)?;
            self.signatures = sanitized_versioned_tx.signatures;
            self.message = RuntimeMessage::StaticallyLoaded(sanitized_versioned_tx.message);
        } else {
            todo!();
        }
        Ok(())
    }
    
    pub fn to_dynamically_loaded(
        &mut self,
        address_loader: impl AddressLoader,
    ) -> Result<()> {
        let message = std::mem::take(&mut self.message);
        if let RuntimeMessage::StaticallyLoaded(sanitized_versioned_message) = message {
            // load dynamic address accounts
            let sanitized_message = SanitizedMessage::try_new(sanitized_versioned_message, address_loader)?;
            let _ = std::mem::replace(&mut self.message, RuntimeMessage::DynamicallyLoaded(sanitized_message));
            self.load_dynamic_metadata()?;
        } else {
            todo!();
        }
        Ok(())
    }

    // Getter
    pub fn get_message_hash(&self) -> &Hash {
        if let RuntimeMessage::Uninitialized = self.message {
            panic!("Runtime transaction is uninitialized");
        }
        &self.meta.message_hash
    }

    pub fn get_is_simple_vote_tx(&self) -> bool {
        if let RuntimeMessage::Uninitialized = self.message {
            panic!("Runtime transaction is uninitialized");
        }
        self.meta.is_simple_vote_tx
    }


    // private helpers
    fn load_static_metadata(
        &mut self,
        sanitized_versioned_tx: &SanitizedVersionedTransaction,
        message_hash: Option<Hash>,
        is_simple_vote_tx: Option<bool>,
    ) ->Result<()> {
        self.meta.set_is_simple_vote_tx(is_simple_vote_tx.unwrap_or_else(|| {
            is_simple_vote_transaction(&sanitized_versioned_tx)
        }));
        self.meta.set_message_hash(message_hash.unwrap_or_else(|| {
            sanitized_versioned_tx.message.message.hash()
        }));

        Ok(())
    }

    fn load_dynamic_metadata(
        &mut self,
    ) -> Result<()> {
        Ok(())
    }
}
