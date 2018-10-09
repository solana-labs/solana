//! ERC20-like Token program

use bincode;

use solana_program_interface::account::Account;
use solana_program_interface::pubkey::Pubkey;
use std;
use transaction::Transaction;

#[derive(Debug, PartialEq)]
pub enum Error {
    InvalidArgument,
    InsufficentFunds,
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
     * The source account for the tokens.
     *
     * If `source` is None, `amount` belongs to this account.
     * If `source` is Option<>, `amount` represents an allowance of tokens that
     * may be transferred from the `source` account.
     */
    source: Option<Pubkey>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
enum Command {
    NewToken(TokenInfo),
    NewTokenAccount(),
    Transfer(u64),
    Approve(u64),
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum TokenProgram {
    Unallocated,
    Token(TokenInfo),
    Account(TokenAccountInfo),
    Invalid,
}
impl Default for TokenProgram {
    fn default() -> TokenProgram {
        TokenProgram::Unallocated
    }
}

pub const TOKEN_PROGRAM_ID: [u8; 32] = [
    5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

impl TokenProgram {
    fn to_invalid_args(err: std::boxed::Box<bincode::ErrorKind>) -> Error {
        warn!("invalid argument: {:?}", err);
        Error::InvalidArgument
    }

    fn deserialize(input: &[u8]) -> Result<TokenProgram> {
        if input.len() < 1 {
            Err(Error::InvalidArgument)?;
        }
        match input[0] {
            0 => Ok(TokenProgram::Unallocated),
            1 => Ok(TokenProgram::Token(
                bincode::deserialize(&input[1..]).map_err(Self::to_invalid_args)?,
            )),
            2 => Ok(TokenProgram::Account(
                bincode::deserialize(&input[1..]).map_err(Self::to_invalid_args)?,
            )),
            _ => Err(Error::InvalidArgument),
        }
    }

    fn serialize(self: &TokenProgram, output: &mut [u8]) -> Result<()> {
        if output.len() == 0 {
            warn!("serialize fail: ouput.len is 0");
            Err(Error::InvalidArgument)?;
        }
        match self {
            TokenProgram::Unallocated | TokenProgram::Invalid => Err(Error::InvalidArgument),
            TokenProgram::Token(token_info) => {
                output[0] = 1;
                let writer = std::io::BufWriter::new(&mut output[1..]);
                bincode::serialize_into(writer, &token_info).map_err(Self::to_invalid_args)
            }
            TokenProgram::Account(account_info) => {
                output[0] = 2;
                let writer = std::io::BufWriter::new(&mut output[1..]);
                bincode::serialize_into(writer, &account_info).map_err(Self::to_invalid_args)
            }
        }
    }

    pub fn check_id(program_id: &Pubkey) -> bool {
        program_id.as_ref() == TOKEN_PROGRAM_ID
    }

    pub fn id() -> Pubkey {
        Pubkey::new(&TOKEN_PROGRAM_ID)
    }

    pub fn process_transaction(tx: &Transaction, accounts: &mut [Account]) -> Result<()> {
        let command =
            bincode::deserialize::<Command>(&tx.userdata).map_err(Self::to_invalid_args)?;
        info!("process_transaction: command={:?}", command);

        let input_program_accounts: Vec<TokenProgram> = accounts
            .iter()
            .map(|account| {
                if Self::check_id(&account.program_id) {
                    Self::deserialize(&account.userdata)
                        .map_err(|err| {
                            info!("deserialize failed: {:?}", err);
                            TokenProgram::Invalid
                        }).unwrap()
                } else {
                    TokenProgram::Invalid
                }
            }).collect();

        //let mut output_program_accounts: Vec<(usize, TokenProgram)> = vec![];
        let mut output_program_accounts: Vec<(_, _)> = vec![];

        match command {
            Command::NewToken(token_info) => {
                if accounts.len() != 2 {
                    error!("Expected 2 accounts");
                    Err(Error::InvalidArgument)?;
                }

                if let TokenProgram::Account(dest_account) = &input_program_accounts[1] {
                    if tx.keys[0] != dest_account.token {
                        info!("account 1 token mismatch");
                        Err(Error::InvalidArgument)?;
                    }

                    if dest_account.source.is_some() {
                        info!("account 1 is a delegate and cannot accept tokens");
                        Err(Error::InvalidArgument)?;
                    }

                    let mut output_dest_account = dest_account.clone();
                    output_dest_account.amount = token_info.supply;
                    output_program_accounts.push((1, TokenProgram::Account(output_dest_account)));
                } else {
                    info!("account 1 invalid");
                    Err(Error::InvalidArgument)?;
                }

                if input_program_accounts[0] != TokenProgram::Unallocated {
                    info!("account 0 not available");
                    Err(Error::InvalidArgument)?;
                }
                output_program_accounts.push((0, TokenProgram::Token(token_info)));
            }
            Command::NewTokenAccount() => {
                // key 0 - Destination new token account
                // key 1 - Owner of the account
                // key 2 - Token this account is associated with
                // key 3 - Source account that this account is a delegate for (optional)
                if accounts.len() < 3 {
                    error!("Expected 3 accounts");
                    Err(Error::InvalidArgument)?;
                }
                if input_program_accounts[0] != TokenProgram::Unallocated {
                    info!("account 0 is already allocated");
                    Err(Error::InvalidArgument)?;
                }

                let mut token_account_info = TokenAccountInfo {
                    token: tx.keys[2],
                    owner: tx.keys[1],
                    amount: 0,
                    source: None,
                };
                if accounts.len() >= 4 {
                    token_account_info.source = Some(tx.keys[3]);
                }
                output_program_accounts.push((0, TokenProgram::Account(token_account_info)));
            }
            Command::Transfer(amount) => {
                if accounts.len() < 3 {
                    error!("Expected 3 accounts");
                    Err(Error::InvalidArgument)?;
                }

                if let (
                    TokenProgram::Account(source_account),
                    TokenProgram::Account(dest_account),
                ) = (&input_program_accounts[1], &input_program_accounts[2])
                {
                    if source_account.token != dest_account.token {
                        info!("account 1/2 token mismatch");
                        Err(Error::InvalidArgument)?;
                    }

                    if dest_account.source.is_some() {
                        info!("account 2 is a delegate and cannot accept tokens");
                        Err(Error::InvalidArgument)?;
                    }

                    if source_account.owner != tx.keys[0] {
                        info!("owner of account 1 not present");
                        Err(Error::InvalidArgument)?;
                    }

                    if source_account.amount < amount {
                        Err(Error::InsufficentFunds)?;
                    }

                    let mut output_source_account = source_account.clone();
                    output_source_account.amount -= amount;
                    output_program_accounts.push((1, TokenProgram::Account(output_source_account)));

                    if source_account.source.is_some() {
                        if accounts.len() != 4 {
                            error!("Expected 4 accounts");
                            Err(Error::InvalidArgument)?;
                        }

                        let delegate_account = source_account;
                        if let TokenProgram::Account(source_account) = &input_program_accounts[3] {
                            if source_account.token != delegate_account.token {
                                info!("account 1/3 token mismatch");
                                Err(Error::InvalidArgument)?;
                            }
                            if delegate_account.source != Some(tx.keys[3]) {
                                info!("Account 1 is not a delegate of account 3");
                                Err(Error::InvalidArgument)?;
                            }

                            if source_account.amount < amount {
                                Err(Error::InsufficentFunds)?;
                            }

                            let mut output_source_account = source_account.clone();
                            output_source_account.amount -= amount;
                            output_program_accounts
                                .push((3, TokenProgram::Account(output_source_account)));
                        } else {
                            info!("account 3 is an invalid account");
                            Err(Error::InvalidArgument)?;
                        }
                    }

                    let mut output_dest_account = dest_account.clone();
                    output_dest_account.amount += amount;
                    output_program_accounts.push((2, TokenProgram::Account(output_dest_account)));
                } else {
                    info!("account 1 and/or 2 are invalid accounts");
                    Err(Error::InvalidArgument)?;
                }
            }
            Command::Approve(amount) => {
                if accounts.len() != 3 {
                    error!("Expected 3 accounts");
                    Err(Error::InvalidArgument)?;
                }

                if let (
                    TokenProgram::Account(source_account),
                    TokenProgram::Account(delegate_account),
                ) = (&input_program_accounts[1], &input_program_accounts[2])
                {
                    if source_account.token != delegate_account.token {
                        info!("account 1/2 token mismatch");
                        Err(Error::InvalidArgument)?;
                    }

                    if source_account.owner != tx.keys[0] {
                        info!("owner of account 1 not present");
                        Err(Error::InvalidArgument)?;
                    }

                    if source_account.source.is_some() {
                        info!("account 1 is a delegate");
                        Err(Error::InvalidArgument)?;
                    }

                    if delegate_account.source != Some(tx.keys[1]) {
                        info!("account 2 is not a delegate of account 1");
                        Err(Error::InvalidArgument)?;
                    }

                    let mut output_delegate_account = delegate_account.clone();
                    output_delegate_account.amount = amount;
                    output_program_accounts
                        .push((2, TokenProgram::Account(output_delegate_account)));
                } else {
                    info!("account 1 and/or 2 are invalid accounts");
                    Err(Error::InvalidArgument)?;
                }
            }
        }

        for (index, program_account) in output_program_accounts.iter() {
            info!(
                "output_program_account: index={} userdata={:?}",
                index, program_account
            );
            Self::serialize(program_account, &mut accounts[*index].userdata)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    pub fn serde() {
        assert_eq!(TokenProgram::deserialize(&[0]), Ok(TokenProgram::default()));

        let mut userdata = vec![0; 256];

        let account = TokenProgram::Account(TokenAccountInfo {
            token: Pubkey::new(&[1; 32]),
            owner: Pubkey::new(&[2; 32]),
            amount: 123,
            source: None,
        });
        assert!(account.serialize(&mut userdata).is_ok());
        assert_eq!(TokenProgram::deserialize(&userdata), Ok(account));

        let account = TokenProgram::Token(TokenInfo {
            supply: 12345,
            decimals: 2,
            name: "A test token".to_string(),
            symbol: "TEST".to_string(),
        });
        assert!(account.serialize(&mut userdata).is_ok());
        assert_eq!(TokenProgram::deserialize(&userdata), Ok(account));
    }

    #[test]
    pub fn serde_expect_fail() {
        let mut userdata = vec![0; 256];

        // Certain TokenProgram's may not be serialized
        let account = TokenProgram::default();
        assert_eq!(account, TokenProgram::Unallocated);
        assert!(account.serialize(&mut userdata).is_err());
        assert!(account.serialize(&mut userdata).is_err());
        let account = TokenProgram::Invalid;
        assert!(account.serialize(&mut userdata).is_err());

        // Bad deserialize userdata
        assert!(TokenProgram::deserialize(&[]).is_err());
        assert!(TokenProgram::deserialize(&[1]).is_err());
        assert!(TokenProgram::deserialize(&[1, 2]).is_err());
        assert!(TokenProgram::deserialize(&[2, 2]).is_err());
        assert!(TokenProgram::deserialize(&[3]).is_err());
    }

    // Note: business logic tests are located in the @solana/web3.js test suite
}
