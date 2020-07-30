use crate::parse_account_data::{ParsableAccount, ParseAccountError};
use solana_sdk::pubkey::Pubkey;
use spl_token_v1_0::{
    option::COption,
    solana_sdk::pubkey::Pubkey as SplTokenPubkey,
    state::{Account, Mint, Multisig, State},
};
use std::{mem::size_of, str::FromStr};

// A helper function to convert spl_token_v1_0::id() as spl_sdk::pubkey::Pubkey to solana_sdk::pubkey::Pubkey
pub fn spl_token_id_v1_0() -> Pubkey {
    Pubkey::from_str(&spl_token_v1_0::id().to_string()).unwrap()
}

pub fn parse_token(data: &[u8]) -> Result<TokenAccountType, ParseAccountError> {
    let mut data = data.to_vec();
    if data.len() == size_of::<Account>() {
        let account: Account = *State::unpack(&mut data)
            .map_err(|_| ParseAccountError::AccountNotParsable(ParsableAccount::Token))?;
        Ok(TokenAccountType::Account(UiTokenAccount {
            mint: account.mint.to_string(),
            owner: account.owner.to_string(),
            amount: account.amount,
            delegate: match account.delegate {
                COption::Some(pubkey) => Some(pubkey.to_string()),
                COption::None => None,
            },
            is_initialized: account.is_initialized,
            is_native: account.is_native,
            delegated_amount: account.delegated_amount,
        }))
    } else if data.len() == size_of::<Mint>() {
        let mint: Mint = *State::unpack(&mut data)
            .map_err(|_| ParseAccountError::AccountNotParsable(ParsableAccount::Token))?;
        Ok(TokenAccountType::Mint(UiMint {
            owner: match mint.owner {
                COption::Some(pubkey) => Some(pubkey.to_string()),
                COption::None => None,
            },
            decimals: mint.decimals,
            is_initialized: mint.is_initialized,
        }))
    } else if data.len() == size_of::<Multisig>() {
        let multisig: Multisig = *State::unpack(&mut data)
            .map_err(|_| ParseAccountError::AccountNotParsable(ParsableAccount::Token))?;
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
            ParsableAccount::Token,
        ))
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
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
    pub amount: u64,
    pub delegate: Option<String>,
    pub is_initialized: bool,
    pub is_native: bool,
    pub delegated_amount: u64,
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_token() {
        let mint_pubkey = SplTokenPubkey::new(&[2; 32]);
        let owner_pubkey = SplTokenPubkey::new(&[3; 32]);
        let mut account_data = [0; size_of::<Account>()];
        let mut account: &mut Account = State::unpack_unchecked(&mut account_data).unwrap();
        account.mint = mint_pubkey;
        account.owner = owner_pubkey;
        account.amount = 42;
        account.is_initialized = true;
        assert_eq!(
            parse_token(&account_data).unwrap(),
            TokenAccountType::Account(UiTokenAccount {
                mint: mint_pubkey.to_string(),
                owner: owner_pubkey.to_string(),
                amount: 42,
                delegate: None,
                is_initialized: true,
                is_native: false,
                delegated_amount: 0,
            }),
        );

        let mut mint_data = [0; size_of::<Mint>()];
        let mut mint: &mut Mint = State::unpack_unchecked(&mut mint_data).unwrap();
        mint.owner = COption::Some(owner_pubkey);
        mint.decimals = 3;
        mint.is_initialized = true;
        assert_eq!(
            parse_token(&mint_data).unwrap(),
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
        let mut multisig: &mut Multisig = State::unpack_unchecked(&mut multisig_data).unwrap();
        let mut signers = [SplTokenPubkey::default(); 11];
        signers[0] = signer1;
        signers[1] = signer2;
        signers[2] = signer3;
        multisig.m = 2;
        multisig.n = 3;
        multisig.is_initialized = true;
        multisig.signers = signers;
        assert_eq!(
            parse_token(&multisig_data).unwrap(),
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
        assert!(parse_token(&bad_data).is_err());
    }
}
