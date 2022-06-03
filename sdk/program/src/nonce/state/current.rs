use {
<<<<<<< HEAD
    super::Versions,
    crate::{fee_calculator::FeeCalculator, hash::Hash, pubkey::Pubkey},
    serde_derive::{Deserialize, Serialize},
};

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Clone)]
=======
    crate::{
        fee_calculator::FeeCalculator,
        hash::{hashv, Hash},
        pubkey::Pubkey,
    },
    serde_derive::{Deserialize, Serialize},
};

const DURABLE_NONCE_HASH_PREFIX: &[u8] = "DURABLE_NONCE".as_bytes();

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct DurableNonce(Hash);

/// Initialized data of a durable transaction nonce account.
///
/// This is stored within [`State`] for initialized nonce accounts.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq, Clone)]
>>>>>>> 5ee157f43 (separates durable nonce and blockhash domains)
pub struct Data {
    pub authority: Pubkey,
<<<<<<< HEAD
    pub blockhash: Hash,
=======
    /// Durable nonce value derived from a valid previous blockhash.
    pub durable_nonce: DurableNonce,
    /// The fee calculator associated with the blockhash.
>>>>>>> 5ee157f43 (separates durable nonce and blockhash domains)
    pub fee_calculator: FeeCalculator,
}

impl Data {
<<<<<<< HEAD
    pub fn new(authority: Pubkey, blockhash: Hash, lamports_per_signature: u64) -> Self {
=======
    /// Create new durable transaction nonce data.
    pub fn new(
        authority: Pubkey,
        durable_nonce: DurableNonce,
        lamports_per_signature: u64,
    ) -> Self {
>>>>>>> 5ee157f43 (separates durable nonce and blockhash domains)
        Data {
            authority,
            durable_nonce,
            fee_calculator: FeeCalculator::new(lamports_per_signature),
        }
    }
<<<<<<< HEAD
=======

    /// Hash value used as recent_blockhash field in Transactions.
    /// Named blockhash for legacy reasons, but durable nonce and blockhash
    /// have separate domains.
    pub fn blockhash(&self) -> Hash {
        self.durable_nonce.0
    }

    /// Get the cost per signature for the next transaction to use this nonce.
>>>>>>> 5ee157f43 (separates durable nonce and blockhash domains)
    pub fn get_lamports_per_signature(&self) -> u64 {
        self.fee_calculator.lamports_per_signature
    }
}

<<<<<<< HEAD
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
=======
impl DurableNonce {
    pub fn from_blockhash(blockhash: &Hash, separate_domains: bool) -> Self {
        Self(if separate_domains {
            hashv(&[DURABLE_NONCE_HASH_PREFIX, blockhash.as_ref()])
        } else {
            *blockhash
        })
    }

    /// Hash value used as recent_blockhash field in Transactions.
    pub fn as_hash(&self) -> &Hash {
        &self.0
    }
}

/// The state of a durable transaction nonce account.
///
/// When created in memory with [`State::default`] or when deserialized from an
/// uninitialized account, a nonce account will be [`State::Uninitialized`].
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
>>>>>>> 5ee157f43 (separates durable nonce and blockhash domains)
pub enum State {
    Uninitialized,
    Initialized(Data),
}

impl Default for State {
    fn default() -> Self {
        State::Uninitialized
    }
}

impl State {
    pub fn new_initialized(
        authority: &Pubkey,
        durable_nonce: DurableNonce,
        lamports_per_signature: u64,
    ) -> Self {
        Self::Initialized(Data::new(*authority, durable_nonce, lamports_per_signature))
    }
    pub fn size() -> usize {
        let data = Versions::new_current(State::Initialized(Data::default()));
        bincode::serialized_size(&data).unwrap() as usize
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn default_is_uninitialized() {
        assert_eq!(State::default(), State::Uninitialized)
    }
}
