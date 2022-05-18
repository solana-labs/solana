use {
    crate::{fee_calculator::FeeCalculator, hash::Hash, pubkey::Pubkey},
    serde_derive::{Deserialize, Serialize},
};

/// Initialized data of a durable transaction nonce account.
///
/// This is stored within [`State`] for initialized nonce accounts.
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Clone)]
pub struct Data {
    /// Address of the account that signs transactions using the nonce account.
    pub authority: Pubkey,
    /// A valid previous blockhash.
    pub blockhash: Hash,
    /// The fee calculator associated with the blockhash.
    pub fee_calculator: FeeCalculator,
}

impl Data {
    /// Create new durable transaction nonce data.
    pub fn new(authority: Pubkey, blockhash: Hash, lamports_per_signature: u64) -> Self {
        Data {
            authority,
            blockhash,
            fee_calculator: FeeCalculator::new(lamports_per_signature),
        }
    }

    /// Get the cost per signature for the next transaction to use this nonce.
    pub fn get_lamports_per_signature(&self) -> u64 {
        self.fee_calculator.lamports_per_signature
    }
}

/// The state of a durable transaction nonce account.
///
/// When created in memory with [`State::default`] or when deserialized from an
/// uninitialized account, a nonce account will be [`State::Uninitialized`].
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
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
    /// Create new durable transaction nonce state.
    pub fn new_initialized(
        authority: &Pubkey,
        blockhash: &Hash,
        lamports_per_signature: u64,
    ) -> Self {
        Self::Initialized(Data::new(*authority, *blockhash, lamports_per_signature))
    }

    /// Get the serialized size of the nonce state.
    pub const fn size() -> usize {
        80 // see test_nonce_state_size.
    }
}

#[cfg(test)]
mod test {
    use {super::*, crate::nonce::state::Versions};

    #[test]
    fn default_is_uninitialized() {
        assert_eq!(State::default(), State::Uninitialized)
    }

    #[test]
    fn test_nonce_state_size() {
        let data = Versions::new_current(State::Initialized(Data::default()));
        let size = bincode::serialized_size(&data).unwrap();
        assert_eq!(State::size() as u64, size);
    }
}
