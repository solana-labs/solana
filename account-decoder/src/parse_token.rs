use crate::{
    parse_account_data::{ParsableAccount, ParseAccountError},
    StringAmount,
};
use solana_sdk::pubkey::Pubkey;
use spl_token_v2_0::{
    option::COption,
    pack::Pack,
    solana_sdk::pubkey::Pubkey as SplTokenPubkey,
    state::{Account, AccountState, Mint, Multisig},
};
use std::str::FromStr;

// A helper function to convert spl_token_v2_0::id() as spl_sdk::pubkey::Pubkey to
// solana_sdk::pubkey::Pubkey
pub fn spl_token_id_v2_0() -> Pubkey {
    Pubkey::from_str(&spl_token_v2_0::id().to_string()).unwrap()
}

// A helper function to convert spl_token_v2_0::native_mint::id() as spl_sdk::pubkey::Pubkey to
// solana_sdk::pubkey::Pubkey
pub fn spl_token_v2_0_native_mint() -> Pubkey {
    Pubkey::from_str(&spl_token_v2_0::native_mint::id().to_string()).unwrap()
}

pub fn parse_token(
    data: &[u8],
    mint_decimals: Option<u8>,
) -> Result<TokenAccountType, ParseAccountError> {
    if data.len() == Account::get_packed_len() {
        let account = Account::unpack(data)
            .map_err(|_| ParseAccountError::AccountNotParsable(ParsableAccount::SplToken))?;
        let decimals = mint_decimals.ok_or_else(|| {
            ParseAccountError::AdditionalDataMissing(
                "no mint_decimals provided to parse spl-token account".to_string(),
            )
        })?;
        Ok(TokenAccountType::Account(UiTokenAccount {
            mint: account.mint.to_string(),
            owner: account.owner.to_string(),
            token_amount: token_amount_to_ui_amount(account.amount, decimals),
            delegate: match account.delegate {
                COption::Some(pubkey) => Some(pubkey.to_string()),
                COption::None => None,
            },
            state: account.state.into(),
            is_native: account.is_native(),
            rent_exempt_reserve: match account.is_native {
                COption::Some(reserve) => Some(token_amount_to_ui_amount(reserve, decimals)),
                COption::None => None,
            },
            delegated_amount: if account.delegate.is_none() {
                None
            } else {
                Some(token_amount_to_ui_amount(
                    account.delegated_amount,
                    decimals,
                ))
            },
            close_authority: match account.close_authority {
                COption::Some(pubkey) => Some(pubkey.to_string()),
                COption::None => None,
            },
        }))
    } else if data.len() == Mint::get_packed_len() {
        let mint = Mint::unpack(data)
            .map_err(|_| ParseAccountError::AccountNotParsable(ParsableAccount::SplToken))?;
        Ok(TokenAccountType::Mint(UiMint {
            mint_authority: match mint.mint_authority {
                COption::Some(pubkey) => Some(pubkey.to_string()),
                COption::None => None,
            },
            supply: mint.supply.to_string(),
            decimals: mint.decimals,
            is_initialized: mint.is_initialized,
            freeze_authority: match mint.freeze_authority {
                COption::Some(pubkey) => Some(pubkey.to_string()),
                COption::None => None,
            },
        }))
    } else if data.len() == Multisig::get_packed_len() {
        let multisig = Multisig::unpack(data)
            .map_err(|_| ParseAccountError::AccountNotParsable(ParsableAccount::SplToken))?;
        Ok(TokenAccountType::Multisig(UiMultisig {
            num_required_signers: multisig.m,
            num_valid_signers: multisig.n,
            is_initialized: multisig.is_initialized,
            signers: multisig
                .signers
                .iter()
                .filter_map(|pubkey| {
                    if pubkey != &SplTokenPubkey::default() {
                        Some(pubkey.to_string())
                    } else {
                        None
                    }
                })
                .collect(),
        }))
    } else {
        Err(ParseAccountError::AccountNotParsable(
            ParsableAccount::SplToken,
        ))
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type", content = "info")]
pub enum TokenAccountType {
    Account(UiTokenAccount),
    Mint(UiMint),
    Multisig(UiMultisig),
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiTokenAccount {
    pub mint: String,
    pub owner: String,
    pub token_amount: UiTokenAmount,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegate: Option<String>,
    pub state: UiAccountState,
    pub is_native: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rent_exempt_reserve: Option<UiTokenAmount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegated_amount: Option<UiTokenAmount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_authority: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum UiAccountState {
    Uninitialized,
    Initialized,
    Frozen,
}

impl From<AccountState> for UiAccountState {
    fn from(state: AccountState) -> Self {
        match state {
            AccountState::Uninitialized => UiAccountState::Uninitialized,
            AccountState::Initialized => UiAccountState::Initialized,
            AccountState::Frozen => UiAccountState::Frozen,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiTokenAmount {
    pub ui_amount: f64,
    pub decimals: u8,
    pub amount: StringAmount,
}

pub fn token_amount_to_ui_amount(amount: u64, decimals: u8) -> UiTokenAmount {
    // Use `amount_to_ui_amount()` once spl_token is bumped to a version that supports it: https://github.com/solana-labs/solana-program-library/pull/211
    let amount_decimals = amount as f64 / 10_usize.pow(decimals as u32) as f64;
    UiTokenAmount {
        ui_amount: amount_decimals,
        decimals,
        amount: amount.to_string(),
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiMint {
    pub mint_authority: Option<String>,
    pub supply: StringAmount,
    pub decimals: u8,
    pub is_initialized: bool,
    pub freeze_authority: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiMultisig {
    pub num_required_signers: u8,
    pub num_valid_signers: u8,
    pub is_initialized: bool,
    pub signers: Vec<String>,
}

pub fn get_token_account_mint(data: &[u8]) -> Option<Pubkey> {
    if data.len() == Account::get_packed_len() {
        Some(Pubkey::new(&data[0..32]))
    } else {
        None
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_token() {
        let mint_pubkey = SplTokenPubkey::new(&[2; 32]);
        let owner_pubkey = SplTokenPubkey::new(&[3; 32]);
        let mut account_data = vec![0; Account::get_packed_len()];
        Account::unpack_unchecked_mut(&mut account_data, &mut |account: &mut Account| {
            account.mint = mint_pubkey;
            account.owner = owner_pubkey;
            account.amount = 42;
            account.state = AccountState::Initialized;
            account.is_native = COption::None;
            account.close_authority = COption::Some(owner_pubkey);
            Ok(())
        })
        .unwrap();

        assert!(parse_token(&account_data, None).is_err());
        assert_eq!(
            parse_token(&account_data, Some(2)).unwrap(),
            TokenAccountType::Account(UiTokenAccount {
                mint: mint_pubkey.to_string(),
                owner: owner_pubkey.to_string(),
                token_amount: UiTokenAmount {
                    ui_amount: 0.42,
                    decimals: 2,
                    amount: "42".to_string()
                },
                delegate: None,
                state: UiAccountState::Initialized,
                is_native: false,
                rent_exempt_reserve: None,
                delegated_amount: None,
                close_authority: Some(owner_pubkey.to_string()),
            }),
        );

        let mut mint_data = vec![0; Mint::get_packed_len()];
        Mint::unpack_unchecked_mut(&mut mint_data, &mut |mint: &mut Mint| {
            mint.mint_authority = COption::Some(owner_pubkey);
            mint.supply = 42;
            mint.decimals = 3;
            mint.is_initialized = true;
            mint.freeze_authority = COption::Some(owner_pubkey);
            Ok(())
        })
        .unwrap();

        assert_eq!(
            parse_token(&mint_data, None).unwrap(),
            TokenAccountType::Mint(UiMint {
                mint_authority: Some(owner_pubkey.to_string()),
                supply: 42.to_string(),
                decimals: 3,
                is_initialized: true,
                freeze_authority: Some(owner_pubkey.to_string()),
            }),
        );

        let signer1 = SplTokenPubkey::new(&[1; 32]);
        let signer2 = SplTokenPubkey::new(&[2; 32]);
        let signer3 = SplTokenPubkey::new(&[3; 32]);
        let mut multisig_data = vec![0; Multisig::get_packed_len()];
        let mut signers = [SplTokenPubkey::default(); 11];
        signers[0] = signer1;
        signers[1] = signer2;
        signers[2] = signer3;
        Multisig::unpack_unchecked_mut(&mut multisig_data, &mut |multisig: &mut Multisig| {
            multisig.m = 2;
            multisig.n = 3;
            multisig.is_initialized = true;
            multisig.signers = signers;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            parse_token(&multisig_data, None).unwrap(),
            TokenAccountType::Multisig(UiMultisig {
                num_required_signers: 2,
                num_valid_signers: 3,
                is_initialized: true,
                signers: vec![
                    signer1.to_string(),
                    signer2.to_string(),
                    signer3.to_string()
                ],
            }),
        );

        let bad_data = vec![0; 4];
        assert!(parse_token(&bad_data, None).is_err());
    }

    #[test]
    fn test_get_token_account_mint() {
        let mint_pubkey = SplTokenPubkey::new(&[2; 32]);
        let mut account_data = vec![0; Account::get_packed_len()];
        Account::unpack_unchecked_mut(&mut account_data, &mut |account: &mut Account| {
            account.mint = mint_pubkey;
            Ok(())
        })
        .unwrap();

        let expected_mint_pubkey = Pubkey::new(&[2; 32]);
        assert_eq!(
            get_token_account_mint(&account_data),
            Some(expected_mint_pubkey)
        );
    }
}
