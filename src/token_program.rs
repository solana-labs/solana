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
    #[cfg_attr(feature = "cargo-clippy", allow(needless_pass_by_value))]
    fn map_to_invalid_args(err: std::boxed::Box<bincode::ErrorKind>) -> Error {
        warn!("invalid argument: {:?}", err);
        Error::InvalidArgument
    }

    fn deserialize(input: &[u8]) -> Result<TokenProgram> {
        if input.is_empty() {
            Err(Error::InvalidArgument)?;
        }
        match input[0] {
            0 => Ok(TokenProgram::Unallocated),
            1 => Ok(TokenProgram::Token(
                bincode::deserialize(&input[1..]).map_err(Self::map_to_invalid_args)?,
            )),
            2 => Ok(TokenProgram::Account(
                bincode::deserialize(&input[1..]).map_err(Self::map_to_invalid_args)?,
            )),
            _ => Err(Error::InvalidArgument),
        }
    }

    fn serialize(self: &TokenProgram, output: &mut [u8]) -> Result<()> {
        if output.is_empty() {
            warn!("serialize fail: ouput.len is 0");
            Err(Error::InvalidArgument)?;
        }
        match self {
            TokenProgram::Unallocated | TokenProgram::Invalid => Err(Error::InvalidArgument),
            TokenProgram::Token(token_info) => {
                output[0] = 1;
                let writer = std::io::BufWriter::new(&mut output[1..]);
                bincode::serialize_into(writer, &token_info).map_err(Self::map_to_invalid_args)
            }
            TokenProgram::Account(account_info) => {
                output[0] = 2;
                let writer = std::io::BufWriter::new(&mut output[1..]);
                bincode::serialize_into(writer, &account_info).map_err(Self::map_to_invalid_args)
            }
        }
    }

    pub fn check_id(program_id: &Pubkey) -> bool {
        program_id.as_ref() == TOKEN_PROGRAM_ID
    }

    pub fn id() -> Pubkey {
        Pubkey::new(&TOKEN_PROGRAM_ID)
    }

    pub fn process_command_newtoken(
        tx: &Transaction,
        pix: usize,
        token_info: TokenInfo,
        input_program_accounts: &[TokenProgram],
        output_program_accounts: &mut Vec<(usize, TokenProgram)>,
    ) -> Result<()> {
        if input_program_accounts.len() != 2 {
            error!("Expected 2 accounts");
            Err(Error::InvalidArgument)?;
        }

        if let TokenProgram::Account(dest_account) = &input_program_accounts[1] {
            if tx.key(pix, 0) != Some(&dest_account.token) {
                info!("account 1 token mismatch");
                Err(Error::InvalidArgument)?;
            }

            if dest_account.delegate.is_some() {
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
        Ok(())
    }

    pub fn process_command_newaccount(
        tx: &Transaction,
        pix: usize,
        input_program_accounts: &[TokenProgram],
        output_program_accounts: &mut Vec<(usize, TokenProgram)>,
    ) -> Result<()> {
        // key 0 - Destination new token account
        // key 1 - Owner of the account
        // key 2 - Token this account is associated with
        // key 3 - Source account that this account is a delegate for (optional)
        if input_program_accounts.len() < 3 {
            error!("Expected 3 accounts");
            Err(Error::InvalidArgument)?;
        }
        if input_program_accounts[0] != TokenProgram::Unallocated {
            info!("account 0 is already allocated");
            Err(Error::InvalidArgument)?;
        }

        let mut token_account_info = TokenAccountInfo {
            token: *tx.key(pix, 2).unwrap(),
            owner: *tx.key(pix, 1).unwrap(),
            amount: 0,
            delegate: None,
        };
        if input_program_accounts.len() >= 4 {
            token_account_info.delegate = Some(TokenAccountDelegateInfo {
                source: *tx.key(pix, 3).unwrap(),
                original_amount: 0,
            });
        }
        output_program_accounts.push((0, TokenProgram::Account(token_account_info)));
        Ok(())
    }

    pub fn process_command_transfer(
        tx: &Transaction,
        pix: usize,
        amount: u64,
        input_program_accounts: &[TokenProgram],
        output_program_accounts: &mut Vec<(usize, TokenProgram)>,
    ) -> Result<()> {
        if input_program_accounts.len() < 3 {
            error!("Expected 3 accounts");
            Err(Error::InvalidArgument)?;
        }

        if let (TokenProgram::Account(source_account), TokenProgram::Account(dest_account)) =
            (&input_program_accounts[1], &input_program_accounts[2])
        {
            if source_account.token != dest_account.token {
                info!("account 1/2 token mismatch");
                Err(Error::InvalidArgument)?;
            }

            if dest_account.delegate.is_some() {
                info!("account 2 is a delegate and cannot accept tokens");
                Err(Error::InvalidArgument)?;
            }

            if Some(&source_account.owner) != tx.key(pix, 0) {
                info!("owner of account 1 not present");
                Err(Error::InvalidArgument)?;
            }

            if source_account.amount < amount {
                Err(Error::InsufficentFunds)?;
            }

            let mut output_source_account = source_account.clone();
            output_source_account.amount -= amount;
            output_program_accounts.push((1, TokenProgram::Account(output_source_account)));

            if let Some(ref delegate_info) = source_account.delegate {
                if input_program_accounts.len() != 4 {
                    error!("Expected 4 accounts");
                    Err(Error::InvalidArgument)?;
                }

                let delegate_account = source_account;
                if let TokenProgram::Account(source_account) = &input_program_accounts[3] {
                    if source_account.token != delegate_account.token {
                        info!("account 1/3 token mismatch");
                        Err(Error::InvalidArgument)?;
                    }
                    if Some(&delegate_info.source) != tx.key(pix, 3) {
                        info!("Account 1 is not a delegate of account 3");
                        Err(Error::InvalidArgument)?;
                    }

                    if source_account.amount < amount {
                        Err(Error::InsufficentFunds)?;
                    }

                    let mut output_source_account = source_account.clone();
                    output_source_account.amount -= amount;
                    output_program_accounts.push((3, TokenProgram::Account(output_source_account)));
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
        Ok(())
    }

    pub fn process_command_approve(
        tx: &Transaction,
        pix: usize,
        amount: u64,
        input_program_accounts: &[TokenProgram],
        output_program_accounts: &mut Vec<(usize, TokenProgram)>,
    ) -> Result<()> {
        if input_program_accounts.len() != 3 {
            error!("Expected 3 accounts");
            Err(Error::InvalidArgument)?;
        }

        if let (TokenProgram::Account(source_account), TokenProgram::Account(delegate_account)) =
            (&input_program_accounts[1], &input_program_accounts[2])
        {
            if source_account.token != delegate_account.token {
                info!("account 1/2 token mismatch");
                Err(Error::InvalidArgument)?;
            }

            if Some(&source_account.owner) != tx.key(pix, 0) {
                info!("owner of account 1 not present");
                Err(Error::InvalidArgument)?;
            }

            if source_account.delegate.is_some() {
                info!("account 1 is a delegate");
                Err(Error::InvalidArgument)?;
            }

            match &delegate_account.delegate {
                None => {
                    info!("account 2 is not a delegate");
                    Err(Error::InvalidArgument)?;
                }
                Some(delegate_info) => {
                    if Some(&delegate_info.source) != tx.key(pix, 1) {
                        info!("account 2 is not a delegate of account 1");
                        Err(Error::InvalidArgument)?;
                    }

                    let mut output_delegate_account = delegate_account.clone();
                    output_delegate_account.amount = amount;
                    output_delegate_account.delegate = Some(TokenAccountDelegateInfo {
                        source: delegate_info.source,
                        original_amount: amount,
                    });
                    output_program_accounts
                        .push((2, TokenProgram::Account(output_delegate_account)));
                }
            }
        } else {
            info!("account 1 and/or 2 are invalid accounts");
            Err(Error::InvalidArgument)?;
        }
        Ok(())
    }

    pub fn process_command_setowner(
        tx: &Transaction,
        pix: usize,
        input_program_accounts: &[TokenProgram],
        output_program_accounts: &mut Vec<(usize, TokenProgram)>,
    ) -> Result<()> {
        if input_program_accounts.len() < 3 {
            error!("Expected 3 accounts");
            Err(Error::InvalidArgument)?;
        }

        if let TokenProgram::Account(source_account) = &input_program_accounts[1] {
            if Some(&source_account.owner) != tx.key(pix, 0) {
                info!("owner of account 1 not present");
                Err(Error::InvalidArgument)?;
            }

            let mut output_source_account = source_account.clone();
            output_source_account.owner = *tx.key(pix, 2).unwrap();
            output_program_accounts.push((1, TokenProgram::Account(output_source_account)));
        } else {
            info!("account 1 is invalid");
            Err(Error::InvalidArgument)?;
        }
        Ok(())
    }

    pub fn process_transaction(
        tx: &Transaction,
        pix: usize,
        accounts: &mut [&mut Account],
    ) -> Result<()> {
        let command = bincode::deserialize::<Command>(&tx.userdata(pix))
            .map_err(Self::map_to_invalid_args)?;
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

        let mut output_program_accounts: Vec<(_, _)> = vec![];

        match command {
            Command::NewToken(token_info) => Self::process_command_newtoken(
                tx,
                pix,
                token_info,
                &input_program_accounts,
                &mut output_program_accounts,
            )?,
            Command::NewTokenAccount => Self::process_command_newaccount(
                tx,
                pix,
                &input_program_accounts,
                &mut output_program_accounts,
            )?,

            Command::Transfer(amount) => Self::process_command_transfer(
                tx,
                pix,
                amount,
                &input_program_accounts,
                &mut output_program_accounts,
            )?,

            Command::Approve(amount) => Self::process_command_approve(
                tx,
                pix,
                amount,
                &input_program_accounts,
                &mut output_program_accounts,
            )?,

            Command::SetOwner => Self::process_command_setowner(
                tx,
                pix,
                &input_program_accounts,
                &mut output_program_accounts,
            )?,
        }

        for (index, program_account) in &output_program_accounts {
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
            delegate: None,
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
