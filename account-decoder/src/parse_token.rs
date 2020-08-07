use crate::{
    parse_account_data::{ParsableAccount, ParseAccountError},
    StringAmount,
};
use solana_sdk::pubkey::Pubkey;
use spl_token_v1_0::{
    option::COption,
    solana_sdk::pubkey::Pubkey as SplTokenPubkey,
    state::{unpack, Account, Mint, Multisig},
};
use std::{mem::size_of, str::FromStr};

// A helper function to convert spl_token_v1_0::id() as spl_sdk::pubkey::Pubkey to
// solana_sdk::pubkey::Pubkey
pub fn spl_token_id_v1_0() -> Pubkey {
    Pubkey::from_str(&spl_token_v1_0::id().to_string()).unwrap()
}

// A helper function to convert spl_token_v1_0::native_mint::id() as spl_sdk::pubkey::Pubkey to
// solana_sdk::pubkey::Pubkey
pub fn spl_token_v1_0_native_mint() -> Pubkey {
    Pubkey::from_str(&spl_token_v1_0::native_mint::id().to_string()).unwrap()
}

pub fn parse_token(
    data: &[u8],
    mint_decimals: Option<u8>,
) -> Result<TokenAccountType, ParseAccountError> {
    let mut data = data.to_vec();
    if data.len() == size_of::<Account>() {
        let account: Account = *unpack(&mut data)
            .map_err(|_| ParseAccountError::AccountNotParsable(ParsableAccount::SplToken))?;
        let decimals = mint_decimals.ok_or_else(|| {
            ParseAccountError::AdditionalDataMissing(
                "no mint_decimals provided to parse spl-token account".to_string(),
            )
        })?;
        let ui_token_amount = token_amount_to_ui_amount(account.amount, decimals);
        Ok(TokenAccountType::Account(UiTokenAccount {
            mint: account.mint.to_string(),
            owner: account.owner.to_string(),
            token_amount: ui_token_amount,
            delegate: match account.delegate {
                COption::Some(pubkey) => Some(pubkey.to_string()),
                COption::None => None,
            },
            is_initialized: account.is_initialized,
            is_native: account.is_native,
            delegated_amount: account.delegated_amount,
        }))
    } else if data.len() == size_of::<Mint>() {
        let mint: Mint = *unpack(&mut data)
            .map_err(|_| ParseAccountError::AccountNotParsable(ParsableAccount::SplToken))?;
        Ok(TokenAccountType::Mint(UiMint {
            owner: match mint.owner {
                COption::Some(pubkey) => Some(pubkey.to_string()),
                COption::None => None,
            },
            decimals: mint.decimals,
            is_initialized: mint.is_initialized,
        }))
    } else if data.len() == size_of::<Multisig>() {
        let multisig: Multisig = *unpack(&mut data)
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
    pub delegate: Option<String>,
    pub is_initialized: bool,
    pub is_native: bool,
    pub delegated_amount: u64,
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
    pub owner: Option<String>,
    pub decimals: u8,
    pub is_initialized: bool,
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
    if data.len() == size_of::<Account>() {
        Some(Pubkey::new(&data[0..32]))
    } else {
        None
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use spl_token_v1_0::state::unpack_unchecked;

    #[test]
    fn test_parse_token() {
        let mint_pubkey = SplTokenPubkey::new(&[2; 32]);
        let owner_pubkey = SplTokenPubkey::new(&[3; 32]);
        let mut account_data = [0; size_of::<Account>()];
        let mut account: &mut Account = unpack_unchecked(&mut account_data).unwrap();
        account.mint = mint_pubkey;
        account.owner = owner_pubkey;
        account.amount = 42;
        account.is_initialized = true;
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
                is_initialized: true,
                is_native: false,
                delegated_amount: 0,
            }),
        );

        let mut mint_data = [0; size_of::<Mint>()];
        let mut mint: &mut Mint = unpack_unchecked(&mut mint_data).unwrap();
        mint.owner = COption::Some(owner_pubkey);
        mint.decimals = 3;
        mint.is_initialized = true;
        assert_eq!(
            parse_token(&mint_data, None).unwrap(),
            TokenAccountType::Mint(UiMint {
                owner: Some(owner_pubkey.to_string()),
                decimals: 3,
                is_initialized: true,
            }),
        );

        let signer1 = SplTokenPubkey::new(&[1; 32]);
        let signer2 = SplTokenPubkey::new(&[2; 32]);
        let signer3 = SplTokenPubkey::new(&[3; 32]);
        let mut multisig_data = [0; size_of::<Multisig>()];
        let mut multisig: &mut Multisig = unpack_unchecked(&mut multisig_data).unwrap();
        let mut signers = [SplTokenPubkey::default(); 11];
        signers[0] = signer1;
        signers[1] = signer2;
        signers[2] = signer3;
        multisig.m = 2;
        multisig.n = 3;
        multisig.is_initialized = true;
        multisig.signers = signers;
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
        let mut account_data = [0; size_of::<Account>()];
        let mut account: &mut Account = unpack_unchecked(&mut account_data).unwrap();
        account.mint = mint_pubkey;

        let expected_mint_pubkey = Pubkey::new(&[2; 32]);
        assert_eq!(
            get_token_account_mint(&account_data),
            Some(expected_mint_pubkey)
        );
    }
}
