#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde_derive;

pub mod parse_account_data;
pub mod parse_nonce;
pub mod parse_vote;

use crate::parse_account_data::parse_account_data;
use serde_json::Value;
use solana_sdk::{account::Account, clock::Epoch, pubkey::Pubkey};
use std::str::FromStr;

/// A duplicate representation of a Message for pretty JSON serialization
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EncodedAccount {
    pub lamports: u64,
    pub data: EncodedAccountData,
    pub owner: String,
    pub executable: bool,
    pub rent_epoch: Epoch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum EncodedAccountData {
    Binary(String),
    Json(Value),
}

impl From<Vec<u8>> for EncodedAccountData {
    fn from(data: Vec<u8>) -> Self {
        Self::Binary(bs58::encode(data).into_string())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum AccountEncoding {
    Binary,
    Json,
}

impl EncodedAccount {
    pub fn encode(account: Account, encoding: AccountEncoding) -> Self {
        let data = match encoding {
            AccountEncoding::Binary => account.data.into(),
            AccountEncoding::Json => {
                if let Ok(parsed_data) = parse_account_data(&account.owner, &account.data) {
                    EncodedAccountData::Json(parsed_data)
                } else {
                    account.data.into()
                }
            }
        };
        EncodedAccount {
            lamports: account.lamports,
            data,
            owner: account.owner.to_string(),
            executable: account.executable,
            rent_epoch: account.rent_epoch,
        }
    }

    pub fn decode(&self) -> Option<Account> {
        let data = match &self.data {
            EncodedAccountData::Json(_) => None,
            EncodedAccountData::Binary(blob) => bs58::decode(blob).into_vec().ok(),
        }?;
        Some(Account {
            lamports: self.lamports,
            data,
            owner: Pubkey::from_str(&self.owner).ok()?,
            executable: self.executable,
            rent_epoch: self.rent_epoch,
        })
    }
}
