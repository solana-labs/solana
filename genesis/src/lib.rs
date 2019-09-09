use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct PrimordialAccountDetails {
    pub balance: u64,
    pub owner: String,
    pub data: String,
    pub executable: bool,
}
