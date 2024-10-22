#![allow(clippy::arithmetic_side_effects)]
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde_derive;

pub mod parse_account_data;
pub mod parse_address_lookup_table;
pub mod parse_bpf_loader;
#[allow(deprecated)]
pub mod parse_config;
pub mod parse_nonce;
pub mod parse_stake;
pub mod parse_sysvar;
pub mod parse_token;
pub mod parse_token_extension;
pub mod parse_vote;
pub mod validator_info;

pub use solana_account_decoder_client_types::{
    UiAccount, UiAccountData, UiAccountEncoding, UiDataSliceConfig,
};
use {
    crate::parse_account_data::{parse_account_data_v2, AccountAdditionalDataV2},
    base64::{prelude::BASE64_STANDARD, Engine},
    solana_sdk::{account::ReadableAccount, fee_calculator::FeeCalculator, pubkey::Pubkey},
    std::io::Write,
};

pub type StringAmount = String;
pub type StringDecimals = String;
pub const MAX_BASE58_BYTES: usize = 128;

fn encode_bs58<T: ReadableAccount>(
    account: &T,
    data_slice_config: Option<UiDataSliceConfig>,
) -> String {
    let slice = slice_data(account.data(), data_slice_config);
    if slice.len() <= MAX_BASE58_BYTES {
        bs58::encode(slice).into_string()
    } else {
        "error: data too large for bs58 encoding".to_string()
    }
}

pub fn encode_ui_account<T: ReadableAccount>(
    pubkey: &Pubkey,
    account: &T,
    encoding: UiAccountEncoding,
    additional_data: Option<AccountAdditionalDataV2>,
    data_slice_config: Option<UiDataSliceConfig>,
) -> UiAccount {
    let space = account.data().len();
    let data = match encoding {
        UiAccountEncoding::Binary => {
            let data = encode_bs58(account, data_slice_config);
            UiAccountData::LegacyBinary(data)
        }
        UiAccountEncoding::Base58 => {
            let data = encode_bs58(account, data_slice_config);
            UiAccountData::Binary(data, encoding)
        }
        UiAccountEncoding::Base64 => UiAccountData::Binary(
            BASE64_STANDARD.encode(slice_data(account.data(), data_slice_config)),
            encoding,
        ),
        UiAccountEncoding::Base64Zstd => {
            let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 0).unwrap();
            match encoder
                .write_all(slice_data(account.data(), data_slice_config))
                .and_then(|()| encoder.finish())
            {
                Ok(zstd_data) => UiAccountData::Binary(BASE64_STANDARD.encode(zstd_data), encoding),
                Err(_) => UiAccountData::Binary(
                    BASE64_STANDARD.encode(slice_data(account.data(), data_slice_config)),
                    UiAccountEncoding::Base64,
                ),
            }
        }
        UiAccountEncoding::JsonParsed => {
            if let Ok(parsed_data) =
                parse_account_data_v2(pubkey, account.owner(), account.data(), additional_data)
            {
                UiAccountData::Json(parsed_data)
            } else {
                UiAccountData::Binary(
                    BASE64_STANDARD.encode(slice_data(account.data(), data_slice_config)),
                    UiAccountEncoding::Base64,
                )
            }
        }
    };
    UiAccount {
        lamports: account.lamports(),
        data,
        owner: account.owner().to_string(),
        executable: account.executable(),
        rent_epoch: account.rent_epoch(),
        space: Some(space as u64),
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
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
    use {
        super::*,
        assert_matches::assert_matches,
        solana_sdk::account::{Account, AccountSharedData},
    };

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

    #[test]
    fn test_encode_account_when_data_exceeds_base58_byte_limit() {
        let data = vec![42; MAX_BASE58_BYTES + 2];
        let account = AccountSharedData::from(Account {
            data,
            ..Account::default()
        });

        // Whole account
        assert_eq!(
            encode_bs58(&account, None),
            "error: data too large for bs58 encoding"
        );

        // Slice of account that's still too large
        assert_eq!(
            encode_bs58(
                &account,
                Some(UiDataSliceConfig {
                    length: MAX_BASE58_BYTES + 1,
                    offset: 1
                })
            ),
            "error: data too large for bs58 encoding"
        );

        // Slice of account that fits inside `MAX_BASE58_BYTES`
        assert_ne!(
            encode_bs58(
                &account,
                Some(UiDataSliceConfig {
                    length: MAX_BASE58_BYTES,
                    offset: 1
                })
            ),
            "error: data too large for bs58 encoding"
        );

        // Slice of account that's too large, but whose intersection with the account still fits
        assert_ne!(
            encode_bs58(
                &account,
                Some(UiDataSliceConfig {
                    length: MAX_BASE58_BYTES + 1,
                    offset: 2
                })
            ),
            "error: data too large for bs58 encoding"
        );
    }

    #[test]
    fn test_base64_zstd() {
        let encoded_account = encode_ui_account(
            &Pubkey::default(),
            &AccountSharedData::from(Account {
                data: vec![0; 1024],
                ..Account::default()
            }),
            UiAccountEncoding::Base64Zstd,
            None,
            None,
        );
        assert_matches!(
            encoded_account.data,
            UiAccountData::Binary(_, UiAccountEncoding::Base64Zstd)
        );

        let decoded_account = encoded_account.decode::<Account>().unwrap();
        assert_eq!(decoded_account.data(), &vec![0; 1024]);
        let decoded_account = encoded_account.decode::<AccountSharedData>().unwrap();
        assert_eq!(decoded_account.data(), &vec![0; 1024]);
    }
}
