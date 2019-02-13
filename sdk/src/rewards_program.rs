use crate::pubkey::Pubkey;
use bincode::serialized_size;

pub const REWARDS_PROGRAM_ID: [u8; 32] = [
    133, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0,
];

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == REWARDS_PROGRAM_ID
}

pub fn id() -> Pubkey {
    Pubkey::new(&REWARDS_PROGRAM_ID)
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct RewardsState {}

/// Upper limit on the size of the Rewards State.
pub fn get_max_size() -> usize {
    let rewards_state = RewardsState::default();
    serialized_size(&rewards_state).unwrap() as usize
}
