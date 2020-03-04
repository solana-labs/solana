mod current;
pub use current::{Data, State};

pub mod v0;

use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub enum Versions {
    V0(Box<v0::State>),
    Current(Box<State>),
}

impl Versions {
    pub fn new_current(state: State) -> Self {
        Self::Current(Box::new(state))
    }

    pub fn convert_to_current(self) -> State {
        match self {
            Self::Current(state) => *state,
            Self::V0(_) => panic!("incompatible nonce account"),
        }
    }
}
