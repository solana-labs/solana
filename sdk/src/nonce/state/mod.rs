mod current;
pub use current::{Meta, State};

use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub enum Versions {
    Current(Box<State>),
}

impl Versions {
    pub fn new_current(state: State) -> Self {
        Self::Current(Box::new(state))
    }

    pub fn convert_to_current(self) -> State {
        match self {
            Self::Current(state) => *state,
        }
    }
}
