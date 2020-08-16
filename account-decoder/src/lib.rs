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
    LegacyBinary(String), // Old way of expressing base-58, retained for RPC backwards compatibility
    Json(ParsedAccount),
    Binary(String, UiAccountEncoding),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum UiAccountEncoding {
    Binary, // base-58 encoded string. SLOW! Avoid this encoding
    JsonParsed,
    Binary64, // base-64 encoded string.
}

impl UiAccount {
    pub fn encode(
        pubkey: &Pubkey,
        account: Account,
        encoding: UiAccountEncoding,
        additional_data: Option<AccountAdditionalData>,
        data_slice_config: Option<UiDataSliceConfig>,
    ) -> Self {
        let data = match encoding {
            UiAccountEncoding::Binary => UiAccountData::LegacyBinary(
                bs58::encode(slice_data(&account.data, data_slice_config)).into_string(),
            ),
            UiAccountEncoding::Binary64 => UiAccountData::Binary(
                base64::encode(slice_data(&account.data, data_slice_config)),
                encoding,
            ),
            UiAccountEncoding::JsonParsed => {
                if let Ok(parsed_data) =
                    parse_account_data(pubkey, &account.owner, &account.data, additional_data)
                {
                    UiAccountData::Json(parsed_data)
                } else {
                    UiAccountData::Binary(
                        base64::encode(&account.data),
                        UiAccountEncoding::Binary64,
                    )
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
            UiAccountData::LegacyBinary(blob) => bs58::decode(blob).into_vec().ok(),
            UiAccountData::Binary(blob, encoding) => match encoding {
                UiAccountEncoding::Binary => bs58::decode(blob).into_vec().ok(),
                UiAccountEncoding::Binary64 => base64::decode(blob).ok(),
                UiAccountEncoding::JsonParsed => None,
            },
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiDataSliceConfig {
    pub offset: usize,
    pub length: usize,
}

fn slice_data(data: &[u8], data_slice_config: Option<UiDataSliceConfig>) -> &[u8] {
    if let Some(UiDataSliceConfig { offset, length }) = data_slice_config {
        if offset >= data.len() {
            &[]
        } else if length > data.len() - offset {
            &data[offset..]
        } else {
            &data[offset..offset + length]
        }
    } else {
        data
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_slice_data() {
        let data = vec![1, 2, 3, 4, 5];
        let slice_config = Some(UiDataSliceConfig {
            offset: 0,
            length: 5,
        });
        assert_eq!(slice_data(&data, slice_config), &data[..]);

        let slice_config = Some(UiDataSliceConfig {
            offset: 0,
            length: 10,
        });
        assert_eq!(slice_data(&data, slice_config), &data[..]);

        let slice_config = Some(UiDataSliceConfig {
            offset: 1,
            length: 2,
        });
        assert_eq!(slice_data(&data, slice_config), &data[1..3]);

        let slice_config = Some(UiDataSliceConfig {
            offset: 10,
            length: 2,
        });
        assert_eq!(slice_data(&data, slice_config), &[] as &[u8]);
    }
}
