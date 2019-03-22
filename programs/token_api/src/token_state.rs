use bincode;
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::account::KeyedAccount;
use solana_sdk::pubkey::Pubkey;
use std;

#[derive(Serialize, Debug, PartialEq)]
pub enum Error {
    InvalidArgument,
    InsufficentFunds,
    NotOwner,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "error")
    }
}
impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TokenInfo {
    /**
     * Total supply of tokens
     */
    supply: u64,

    /**
     * Number of base 10 digits to the right of the decimal place in the total supply
     */
    decimals: u8,

    /**
     * Descriptive name of this token
     */
    name: String,

    /**
     * Symbol for this token
     */
    symbol: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenAccountDelegateInfo {
    /**
     * The source account for the tokens
     */
    source: Pubkey,

    /**
     * The original amount that this delegate account was authorized to spend up to
     */
    original_amount: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenAccountInfo {
    /**
     * The kind of token this account holds
     */
    token: Pubkey,

    /**
     * Owner of this account
     */
    owner: Pubkey,

    /**
     * Amount of tokens this account holds
     */
    amount: u64,

    /**
     * If `delegate` None, `amount` belongs to this account.
     * If `delegate` is Option<_>, `amount` represents the remaining allowance
     * of tokens that may be transferred from the `source` account.
     */
    delegate: Option<TokenAccountDelegateInfo>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
enum Command {
    NewToken(TokenInfo),
    NewTokenAccount,
    Transfer(u64),
    Approve(u64),
    SetOwner,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum TokenState {
    Unallocated,
    Token(TokenInfo),
    Account(TokenAccountInfo),
    Invalid,
}
impl Default for TokenState {
    fn default() -> TokenState {
        TokenState::Unallocated
    }
}

impl TokenState {
    #[allow(clippy::needless_pass_by_value)]
    fn map_to_invalid_args(err: std::boxed::Box<bincode::ErrorKind>) -> Error {
        warn!("invalid argument: {:?}", err);
        Error::InvalidArgument
    }

    pub fn deserialize(input: &[u8]) -> Result<TokenState> {
        if input.is_empty() {
            Err(Error::InvalidArgument)?;
        }
        match input[0] {
            0 => Ok(TokenState::Unallocated),
            1 => Ok(TokenState::Token(
                bincode::deserialize(&input[1..]).map_err(Self::map_to_invalid_args)?,
            )),
            2 => Ok(TokenState::Account(
                bincode::deserialize(&input[1..]).map_err(Self::map_to_invalid_args)?,
            )),
            _ => Err(Error::InvalidArgument),
        }
    }

    fn serialize(self: &TokenState, output: &mut [u8]) -> Result<()> {
        if output.is_empty() {
            warn!("serialize fail: ouput.len is 0");
            Err(Error::InvalidArgument)?;
        }
        match self {
            TokenState::Unallocated | TokenState::Invalid => Err(Error::InvalidArgument),
            TokenState::Token(token_info) => {
                output[0] = 1;
                let writer = std::io::BufWriter::new(&mut output[1..]);
                bincode::serialize_into(writer, &token_info).map_err(Self::map_to_invalid_args)
            }
            TokenState::Account(account_info) => {
                output[0] = 2;
                let writer = std::io::BufWriter::new(&mut output[1..]);
                bincode::serialize_into(writer, &account_info).map_err(Self::map_to_invalid_args)
            }
        }
    }

    #[allow(dead_code)]
    pub fn amount(&self) -> Result<u64> {
        if let TokenState::Account(account_info) = self {
            Ok(account_info.amount)
        } else {
            Err(Error::InvalidArgument)
        }
    }

    #[allow(dead_code)]
    pub fn only_owner(&self, key: &Pubkey) -> Result<()> {
        if *key != Pubkey::default() {
            if let TokenState::Account(account_info) = self {
                if account_info.owner == *key {
                    return Ok(());
                }
            }
        }
        warn!("TokenState: non-owner rejected");
        Err(Error::NotOwner)
    }

    pub fn process_command_newtoken(
        info: &mut [KeyedAccount],
        token_info: TokenInfo,
        input_program_accounts: &[TokenState],
        output_program_accounts: &mut Vec<(usize, TokenState)>,
    ) -> Result<()> {
        if input_program_accounts.len() != 2 {
            error!("Expected 2 accounts");
            Err(Error::InvalidArgument)?;
        }

        if let TokenState::Account(dest_account) = &input_program_accounts[1] {
            if info[0].signer_key().unwrap() != &dest_account.token {
                error!("account 1 token mismatch");
                Err(Error::InvalidArgument)?;
            }

            if dest_account.delegate.is_some() {
                error!("account 1 is a delegate and cannot accept tokens");
                Err(Error::InvalidArgument)?;
            }

            let mut output_dest_account = dest_account.clone();
            output_dest_account.amount = token_info.supply;
            output_program_accounts.push((1, TokenState::Account(output_dest_account)));
        } else {
            error!("account 1 invalid");
            Err(Error::InvalidArgument)?;
        }

        if input_program_accounts[0] != TokenState::Unallocated {
            error!("account 0 not available");
            Err(Error::InvalidArgument)?;
        }
        output_program_accounts.push((0, TokenState::Token(token_info)));
        Ok(())
    }

    pub fn process_command_newaccount(
        info: &mut [KeyedAccount],
        input_program_accounts: &[TokenState],
        output_program_accounts: &mut Vec<(usize, TokenState)>,
    ) -> Result<()> {
        // key 0 - Destination new token account
        // key 1 - Owner of the account
        // key 2 - Token this account is associated with
        // key 3 - Source account that this account is a delegate for (optional)
        if input_program_accounts.len() < 3 {
            error!("Expected 3 accounts");
            Err(Error::InvalidArgument)?;
        }
        if input_program_accounts[0] != TokenState::Unallocated {
            error!("account 0 is already allocated");
            Err(Error::InvalidArgument)?;
        }
        let mut token_account_info = TokenAccountInfo {
            token: *info[2].unsigned_key(),
            owner: *info[1].unsigned_key(),
            amount: 0,
            delegate: None,
        };
        if input_program_accounts.len() >= 4 {
            token_account_info.delegate = Some(TokenAccountDelegateInfo {
                source: *info[3].unsigned_key(),
                original_amount: 0,
            });
        }
        output_program_accounts.push((0, TokenState::Account(token_account_info)));
        Ok(())
    }

    pub fn process_command_transfer(
        info: &mut [KeyedAccount],
        amount: u64,
        input_program_accounts: &[TokenState],
        output_program_accounts: &mut Vec<(usize, TokenState)>,
    ) -> Result<()> {
        if input_program_accounts.len() < 3 {
            error!("Expected 3 accounts");
            Err(Error::InvalidArgument)?;
        }

        if let (TokenState::Account(source_account), TokenState::Account(dest_account)) =
            (&input_program_accounts[1], &input_program_accounts[2])
        {
            if source_account.token != dest_account.token {
                error!("account 1/2 token mismatch");
                Err(Error::InvalidArgument)?;
            }

            if dest_account.delegate.is_some() {
                error!("account 2 is a delegate and cannot accept tokens");
                Err(Error::InvalidArgument)?;
            }

            if info[0].signer_key().unwrap() != &source_account.owner {
                error!("owner of account 1 not present");
                Err(Error::InvalidArgument)?;
            }

            if source_account.amount < amount {
                Err(Error::InsufficentFunds)?;
            }

            let mut output_source_account = source_account.clone();
            output_source_account.amount -= amount;
            output_program_accounts.push((1, TokenState::Account(output_source_account)));

            if let Some(ref delegate_info) = source_account.delegate {
                if input_program_accounts.len() != 4 {
                    error!("Expected 4 accounts");
                    Err(Error::InvalidArgument)?;
                }

                let delegate_account = source_account;
                if let TokenState::Account(source_account) = &input_program_accounts[3] {
                    if source_account.token != delegate_account.token {
                        error!("account 1/3 token mismatch");
                        Err(Error::InvalidArgument)?;
                    }
                    if info[3].unsigned_key() != &delegate_info.source {
                        error!("Account 1 is not a delegate of account 3");
                        Err(Error::InvalidArgument)?;
                    }

                    if source_account.amount < amount {
                        Err(Error::InsufficentFunds)?;
                    }

                    let mut output_source_account = source_account.clone();
                    output_source_account.amount -= amount;
                    output_program_accounts.push((3, TokenState::Account(output_source_account)));
                } else {
                    error!("account 3 is an invalid account");
                    Err(Error::InvalidArgument)?;
                }
            }

            let mut output_dest_account = dest_account.clone();
            output_dest_account.amount += amount;
            output_program_accounts.push((2, TokenState::Account(output_dest_account)));
        } else {
            error!("account 1 and/or 2 are invalid accounts");
            Err(Error::InvalidArgument)?;
        }
        Ok(())
    }

    pub fn process_command_approve(
        info: &mut [KeyedAccount],
        amount: u64,
        input_program_accounts: &[TokenState],
        output_program_accounts: &mut Vec<(usize, TokenState)>,
    ) -> Result<()> {
        if input_program_accounts.len() != 3 {
            error!("Expected 3 accounts");
            Err(Error::InvalidArgument)?;
        }

        if let (TokenState::Account(source_account), TokenState::Account(delegate_account)) =
            (&input_program_accounts[1], &input_program_accounts[2])
        {
            if source_account.token != delegate_account.token {
                error!("account 1/2 token mismatch");
                Err(Error::InvalidArgument)?;
            }

            if info[0].signer_key().unwrap() != &source_account.owner {
                error!("owner of account 1 not present");
                Err(Error::InvalidArgument)?;
            }

            if source_account.delegate.is_some() {
                error!("account 1 is a delegate");
                Err(Error::InvalidArgument)?;
            }

            match &delegate_account.delegate {
                None => {
                    error!("account 2 is not a delegate");
                    Err(Error::InvalidArgument)?;
                }
                Some(delegate_info) => {
                    if info[1].unsigned_key() != &delegate_info.source {
                        error!("account 2 is not a delegate of account 1");
                        Err(Error::InvalidArgument)?;
                    }

                    let mut output_delegate_account = delegate_account.clone();
                    output_delegate_account.amount = amount;
                    output_delegate_account.delegate = Some(TokenAccountDelegateInfo {
                        source: delegate_info.source,
                        original_amount: amount,
                    });
                    output_program_accounts.push((2, TokenState::Account(output_delegate_account)));
                }
            }
        } else {
            error!("account 1 and/or 2 are invalid accounts");
            Err(Error::InvalidArgument)?;
        }
        Ok(())
    }

    pub fn process_command_setowner(
        info: &mut [KeyedAccount],
        input_program_accounts: &[TokenState],
        output_program_accounts: &mut Vec<(usize, TokenState)>,
    ) -> Result<()> {
        if input_program_accounts.len() < 3 {
            error!("Expected 3 accounts");
            Err(Error::InvalidArgument)?;
        }

        if let TokenState::Account(source_account) = &input_program_accounts[1] {
            if info[0].signer_key().unwrap() != &source_account.owner {
                info!("owner of account 1 not present");
                Err(Error::InvalidArgument)?;
            }

            let mut output_source_account = source_account.clone();
            output_source_account.owner = *info[2].unsigned_key();
            output_program_accounts.push((1, TokenState::Account(output_source_account)));
        } else {
            info!("account 1 is invalid");
            Err(Error::InvalidArgument)?;
        }
        Ok(())
    }

    pub fn process(program_id: &Pubkey, info: &mut [KeyedAccount], input: &[u8]) -> Result<()> {
        let command = bincode::deserialize::<Command>(input).map_err(Self::map_to_invalid_args)?;
        info!("process_transaction: command={:?}", command);

        if info[0].signer_key().is_none() {
            Err(Error::InvalidArgument)?;
        }

        let input_program_accounts: Vec<TokenState> = info
            .iter()
            .map(|keyed_account| {
                let account = &keyed_account.account;
                if account.owner == *program_id {
                    match Self::deserialize(&account.data) {
                        Ok(token_program) => token_program,
                        Err(err) => {
                            error!("deserialize failed: {:?}", err);
                            TokenState::Invalid
                        }
                    }
                } else {
                    TokenState::Invalid
                }
            })
            .collect();

        for program_account in &input_program_accounts {
            info!("input_program_account: data={:?}", program_account);
        }

        let mut output_program_accounts: Vec<(_, _)> = vec![];

        match command {
            Command::NewToken(token_info) => Self::process_command_newtoken(
                info,
                token_info,
                &input_program_accounts,
                &mut output_program_accounts,
            )?,
            Command::NewTokenAccount => Self::process_command_newaccount(
                info,
                &input_program_accounts,
                &mut output_program_accounts,
            )?,

            Command::Transfer(amount) => Self::process_command_transfer(
                info,
                amount,
                &input_program_accounts,
                &mut output_program_accounts,
            )?,

            Command::Approve(amount) => Self::process_command_approve(
                info,
                amount,
                &input_program_accounts,
                &mut output_program_accounts,
            )?,

            Command::SetOwner => Self::process_command_setowner(
                info,
                &input_program_accounts,
                &mut output_program_accounts,
            )?,
        }

        for (index, program_account) in &output_program_accounts {
            info!(
                "output_program_account: index={} data={:?}",
                index, program_account
            );
            Self::serialize(program_account, &mut info[*index].account.data)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    pub fn serde() {
        assert_eq!(TokenState::deserialize(&[0]), Ok(TokenState::default()));

        let mut data = vec![0; 256];

        let account = TokenState::Account(TokenAccountInfo {
            token: Pubkey::new(&[1; 32]),
            owner: Pubkey::new(&[2; 32]),
            amount: 123,
            delegate: None,
        });
        account.serialize(&mut data).unwrap();
        assert_eq!(TokenState::deserialize(&data), Ok(account));

        let account = TokenState::Token(TokenInfo {
            supply: 12345,
            decimals: 2,
            name: "A test token".to_string(),
            symbol: "TEST".to_string(),
        });
        account.serialize(&mut data).unwrap();
        assert_eq!(TokenState::deserialize(&data), Ok(account));
    }

    #[test]
    pub fn serde_expect_fail() {
        let mut data = vec![0; 256];

        // Certain TokenState's may not be serialized
        let account = TokenState::default();
        assert_eq!(account, TokenState::Unallocated);
        assert!(account.serialize(&mut data).is_err());
        assert!(account.serialize(&mut data).is_err());
        let account = TokenState::Invalid;
        assert!(account.serialize(&mut data).is_err());

        // Bad deserialize data
        assert!(TokenState::deserialize(&[]).is_err());
        assert!(TokenState::deserialize(&[1]).is_err());
        assert!(TokenState::deserialize(&[1, 2]).is_err());
        assert!(TokenState::deserialize(&[2, 2]).is_err());
        assert!(TokenState::deserialize(&[3]).is_err());
    }

    // Note: business logic tests are located in the @solana/web3.js test suite
}
