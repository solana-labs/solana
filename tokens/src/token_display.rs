use solana_account_decoder::parse_token::token_amount_to_ui_amount;
use solana_sdk::native_token::lamports_to_sol;
use std::{
    fmt::{Debug, Display, Formatter, Result},
    ops::Add,
};

const SOL_SYMBOL: &str = "◎";

#[derive(PartialEq)]
pub enum TokenType {
    Sol,
    SplToken,
}

pub struct Token {
    amount: u64,
    decimals: u8,
    token_type: TokenType,
}

impl Token {
    fn write_with_symbol(&self, f: &mut Formatter) -> Result {
        match &self.token_type {
            TokenType::Sol => {
                let amount = lamports_to_sol(self.amount);
                write!(f, "{}{}", SOL_SYMBOL, amount)
            }
            TokenType::SplToken => {
                let amount = token_amount_to_ui_amount(self.amount, self.decimals).ui_amount;
                write!(f, "{} tokens", amount)
            }
        }
    }

    pub fn sol(amount: u64) -> Self {
        Self {
            amount,
            decimals: 9,
            token_type: TokenType::Sol,
        }
    }

    pub fn spl_token(amount: u64, decimals: u8) -> Self {
        Self {
            amount,
            decimals,
            token_type: TokenType::SplToken,
        }
    }
}

impl Display for Token {
    fn fmt(&self, f: &mut Formatter) -> Result {
        self.write_with_symbol(f)
    }
}

impl Debug for Token {
    fn fmt(&self, f: &mut Formatter) -> Result {
        self.write_with_symbol(f)
    }
}

impl Add for Token {
    type Output = Token;

    fn add(self, other: Self) -> Self {
        if self.token_type == other.token_type {
            Self {
                amount: self.amount + other.amount,
                decimals: self.decimals,
                token_type: self.token_type,
            }
        } else {
            self
        }
    }
}
