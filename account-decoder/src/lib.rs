#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde_derive;

pub mod parse_account_data;
pub mod parse_config;
pub mod parse_nonce;
pub mod parse_stake;
pub mod parse_sysvar;
pub mod parse_token;
pub mod parse_vote;
pub mod validator_info;

use crate::parse_account_data::{parse_account_data, AccountAdditionalData, ParsedAccount};
use solana_sdk::{account::Account, clock::Epoch, fee_calculator::FeeCalculator, pubkey::Pubkey};
use std::str::FromStr;

pub type StringAmount = String;

/// A duplicate representation of an Account for pretty JSON serialization
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UiAccount {
    pub lamports: u64,
    pub data: UiAccountData,
    pub owner: String,
    pub executable: bool,
    pub rent_epoch: Epoch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum UiAccountData {
    Binary(String),
    Json(ParsedAccount),
    Binary64(String),
}

impl From<Vec<u8>> for UiAccountData {
    fn from(data: Vec<u8>) -> Self {
        Self::Binary(bs58::encode(data).into_string())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum UiAccountEncoding {
    Binary,
    JsonParsed,
    Binary64,
}

impl UiAccount {
    pub fn encode(
        pubkey: &Pubkey,
        account: Account,
        encoding: UiAccountEncoding,
        additional_data: Option<AccountAdditionalData>,
    ) -> Self {
        let data = match encoding {
            UiAccountEncoding::Binary => account.data.into(),
            UiAccountEncoding::Binary64 => UiAccountData::Binary64(base64::encode(account.data)),
            UiAccountEncoding::JsonParsed => {
                if let Ok(parsed_data) =
                    parse_account_data(pubkey, &account.owner, &account.data, additional_data)
                {
                    UiAccountData::Json(parsed_data)
                } else {
                    account.data.into()
                }
            }
        };
        UiAccount {
            lamports: account.lamports,
            data,
            owner: account.owner.to_string(),
            executable: account.executable,
            rent_epoch: account.rent_epoch,
        }
    }

    pub fn decode(&self) -> Option<Account> {
        let data = match &self.data {
            UiAccountData::Json(_) => None,
            UiAccountData::Binary(blob) => bs58::decode(blob).into_vec().ok(),
            UiAccountData::Binary64(blob) => base64::decode(blob).ok(),
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiFeeCalculator {
    pub lamports_per_signature: StringAmount,
}

impl From<FeeCalculator> for UiFeeCalculator {
    fn from(fee_calculator: FeeCalculator) -> Self {
        Self {
            lamports_per_signature: fee_calculator.lamports_per_signature.to_string(),
        }
    }
}

impl Default for UiFeeCalculator {
    fn default() -> Self {
        Self {
            lamports_per_signature: "0".to_string(),
        }
    }
}
