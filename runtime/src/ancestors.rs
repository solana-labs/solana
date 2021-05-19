use solana_sdk::clock::Slot;
use std::collections::HashMap;

pub type Ancestors = HashMap<Slot, usize>;
