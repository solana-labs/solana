use super::Versions;
use crate::{fee_calculator::FeeCalculator, hash::Hash, pubkey::Pubkey};
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Clone)]
pub struct Data {
    pub authority: Pubkey,
    pub blockhash: Hash,
    pub fee_calculator: FeeCalculator,
}

impl Data {
    pub fn new(authority: Pubkey, blockhash: Hash, lamports_per_signature: u64) -> Self {
        Data {
            authority,
            blockhash,
            fee_calculator: FeeCalculator::new(lamports_per_signature),
        }
    }
    pub fn get_lamports_per_signature(&self) -> u64 {
        self.fee_calculator.lamports_per_signature
    }
}

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
