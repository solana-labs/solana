use bincode::serialized_size;
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RewardsState {}

impl RewardsState {
    /// Upper limit on the serialized size of RewardsState.
    pub fn max_size() -> usize {
        let rewards_state = RewardsState::default();
        serialized_size(&rewards_state).unwrap() as usize
    }
}
