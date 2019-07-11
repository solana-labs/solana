use serde_derive::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::{error, fmt};

/// Fixed-point scaler, 10 = one base 10 digit to the right of the decimal, 100 = 2, ...
/// Used by both price and amount in their fixed point representation
pub const SCALER: u64 = 1000;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ExchangeError {
    InvalidTrade(String),
}
impl error::Error for ExchangeError {}
impl fmt::Display for ExchangeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ExchangeError::InvalidTrade(s) => write!(f, "{}", s),
        }
    }
}

/// Supported token types
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Token {
    A,
    B,
    C,
    D,
}
impl Default for Token {
    fn default() -> Self {
        Token::A
    }
}

// Values of tokens, could be quantities, prices, etc...
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[allow(non_snake_case)]
pub struct Tokens {
    pub A: u64,
    pub B: u64,
    pub C: u64,
    pub D: u64,
}
impl Tokens {
    pub fn new(a: u64, b: u64, c: u64, d: u64) -> Self {
        Self {
            A: a,
            B: b,
            C: c,
            D: d,
        }
    }
}
impl std::ops::Index<Token> for Tokens {
    type Output = u64;
    fn index(&self, t: Token) -> &u64 {
        match t {
            Token::A => &self.A,
            Token::B => &self.B,
            Token::C => &self.C,
            Token::D => &self.D,
        }
    }
}
impl std::ops::IndexMut<Token> for Tokens {
    fn index_mut(&mut self, t: Token) -> &mut u64 {
        match t {
            Token::A => &mut self.A,
            Token::B => &mut self.B,
            Token::C => &mut self.C,
            Token::D => &mut self.D,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[allow(non_snake_case)]
pub enum TokenPair {
    AB,
    AC,
    AD,
    BC,
    BD,
    CD,
}
impl Default for TokenPair {
    fn default() -> Self {
        TokenPair::AB
    }
}
impl TokenPair {
    pub fn primary(self) -> Token {
        match self {
            TokenPair::AB | TokenPair::AC | TokenPair::AD => Token::A,
            TokenPair::BC | TokenPair::BD => Token::B,
            TokenPair::CD => Token::C,
        }
    }
    pub fn secondary(self) -> Token {
        match self {
            TokenPair::AB => Token::B,
            TokenPair::AC | TokenPair::BC => Token::C,
            TokenPair::AD | TokenPair::BD | TokenPair::CD => Token::D,
        }
    }
}

/// Token accounts are populated with this structure
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct TokenAccountInfo {
    /// Investor who owns this account
    pub owner: Pubkey,
    /// Current number of tokens this account holds
    pub tokens: Tokens,
}
impl TokenAccountInfo {
    pub fn owner(mut self, owner: &Pubkey) -> Self {
        self.owner = *owner;
        self
    }
    pub fn tokens(mut self, a: u64, b: u64, c: u64, d: u64) -> Self {
        self.tokens = Tokens {
            A: a,
            B: b,
            C: c,
            D: d,
        };
        self
    }
}

/// Direction of the exchange between two tokens in a pair
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Direction {
    /// Trade first token type (primary) in the pair 'To' the second
    To,
    /// Trade first token type in the pair 'From' the second (secondary)
    From,
}
impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Direction::To => write!(f, "T")?,
            Direction::From => write!(f, "F")?,
        }
        Ok(())
    }
}

/// Trade accounts are populated with this structure
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OrderInfo {
    /// Owner of the trade order
    pub owner: Pubkey,
    /// Direction of the exchange
    pub direction: Direction,
    /// Token pair indicating two tokens to exchange, first is primary
    pub pair: TokenPair,
    /// Number of tokens to exchange; primary or secondary depending on direction.  Once
    /// this number goes to zero this trade order will be converted into a regular token account
    pub tokens: u64,
    /// Scaled price of the secondary token given the primary is equal to the scale value
    /// If scale is 1 and price is 2 then ratio is 1:2 or 1 primary token for 2 secondary tokens
    pub price: u64,
    /// Number of tokens that have been settled so far.  These nay be transferred to another
    /// token account by the owner.
    pub tokens_settled: u64,
}
impl Default for OrderInfo {
    fn default() -> Self {
        Self {
            owner: Pubkey::default(),
            pair: TokenPair::AB,
            direction: Direction::To,
            tokens: 0,
            price: 0,
            tokens_settled: 0,
        }
    }
}
impl OrderInfo {
    pub fn pair(mut self, pair: TokenPair) -> Self {
        self.pair = pair;
        self
    }
    pub fn direction(mut self, direction: Direction) -> Self {
        self.direction = direction;
        self
    }
    pub fn tokens(mut self, tokens: u64) -> Self {
        self.tokens = tokens;
        self
    }
    pub fn price(mut self, price: u64) -> Self {
        self.price = price;
        self
    }
}

pub fn check_trade(direction: Direction, tokens: u64, price: u64) -> Result<(), ExchangeError> {
    match direction {
        Direction::To => {
            if tokens * price / SCALER == 0 {
                Err(ExchangeError::InvalidTrade(format!(
                    "To trade of {} for {}/{} results in 0 tradeable tokens",
                    tokens, SCALER, price
                )))?
            }
        }
        Direction::From => {
            if tokens * SCALER / price == 0 {
                Err(ExchangeError::InvalidTrade(format!(
                    "From trade of {} for {}?{} results in 0 tradeable tokens",
                    tokens, SCALER, price
                )))?
            }
        }
    }
    Ok(())
}

/// Type of exchange account, account's user data is populated with this enum
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ExchangeState {
    /// Account's Userdata is unallocated
    Unallocated,
    // Token account
    Account(TokenAccountInfo),
    // Trade order account
    Trade(OrderInfo),
    Invalid,
}
impl Default for ExchangeState {
    fn default() -> Self {
        ExchangeState::Unallocated
    }
}
