use super::Versions;
use crate::{hash::Hash, pubkey::Pubkey};
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Clone, Copy)]
pub struct Meta {
    pub nonce_authority: Pubkey,
}

impl Meta {
    pub fn new(nonce_authority: &Pubkey) -> Self {
        Self {
            nonce_authority: *nonce_authority,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Copy)]
pub enum State {
    Uninitialized,
    Initialized(Meta, Hash),
}

impl Default for State {
    fn default() -> Self {
        State::Uninitialized
    }
}

impl State {
    pub fn size() -> usize {
        let data = Versions::new_current(State::Initialized(Meta::default(), Hash::default()));
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
