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
    amount: f64,
    token_type: TokenType,
}

impl Token {
    fn write_with_symbol(&self, f: &mut Formatter) -> Result {
        match &self.token_type {
            TokenType::Sol => write!(f, "{}{}", SOL_SYMBOL, self.amount,),
            TokenType::SplToken => write!(f, "{} tokens", self.amount,),
        }
    }

    pub fn from(amount: f64, is_sol: bool) -> Self {
        let token_type = if is_sol {
            TokenType::Sol
        } else {
            TokenType::SplToken
        };
        Self { amount, token_type }
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
                token_type: self.token_type,
            }
        } else {
            self
        }
    }
}
