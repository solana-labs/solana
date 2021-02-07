use itertools::EitherOrBoth::{Both, Left, Right};
use itertools::Itertools;
use log::*;
use solana_exchange_program::exchange_state::*;
use solana_sdk::pubkey::Pubkey;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::{error, fmt};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToOrder {
    pub pubkey: Pubkey,
    pub info: OrderInfo,
}

impl Ord for ToOrder {
    fn cmp(&self, other: &Self) -> Ordering {
        other.info.price.cmp(&self.info.price)
    }
}
impl PartialOrd for ToOrder {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FromOrder {
    pub pubkey: Pubkey,
    pub info: OrderInfo,
}

impl Ord for FromOrder {
    fn cmp(&self, other: &Self) -> Ordering {
        self.info.price.cmp(&other.info.price)
    }
}
impl PartialOrd for FromOrder {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Default)]
pub struct OrderBook {
    // TODO scale to x token types
    to_ab: BinaryHeap<ToOrder>,
    from_ab: BinaryHeap<FromOrder>,
}
impl fmt::Display for OrderBook {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(
            f,
            "+-Order Book--------------------------+-------------------------------------+"
        )?;
        for (i, it) in self
            .to_ab
            .iter()
            .zip_longest(self.from_ab.iter())
            .enumerate()
        {
            match it {
                Both(to, from) => writeln!(
                    f,
                    "| T AB {:8} for {:8}/{:8} | F AB {:8} for {:8}/{:8} |{}",
                    to.info.tokens,
                    SCALER,
                    to.info.price,
                    from.info.tokens,
                    SCALER,
                    from.info.price,
                    i
                )?,
                Left(to) => writeln!(
                    f,
                    "| T AB {:8} for {:8}/{:8} |                                     |{}",
                    to.info.tokens, SCALER, to.info.price, i
                )?,
                Right(from) => writeln!(
                    f,
                    "|                                     | F AB {:8} for {:8}/{:8} |{}",
                    from.info.tokens, SCALER, from.info.price, i
                )?,
            }
        }
        write!(
            f,
            "+-------------------------------------+-------------------------------------+"
        )?;
        Ok(())
    }
}

impl OrderBook {
    // TODO
    // pub fn cancel(&mut self, pubkey: Pubkey) -> Result<(), Box<dyn error::Error>> {
    //     Ok(())
    // }
    pub fn push(&mut self, pubkey: Pubkey, info: OrderInfo) -> Result<(), Box<dyn error::Error>> {
        check_trade(info.side, info.tokens, info.price)?;
        match info.side {
            OrderSide::Ask => {
                self.to_ab.push(ToOrder { pubkey, info });
            }
            OrderSide::Bid => {
                self.from_ab.push(FromOrder { pubkey, info });
            }
        }
        Ok(())
    }
    pub fn pop(&mut self) -> Option<(ToOrder, FromOrder)> {
        if let Some(pair) = Self::pop_pair(&mut self.to_ab, &mut self.from_ab) {
            return Some(pair);
        }
        None
    }
    pub fn get_num_outstanding(&self) -> (usize, usize) {
        (self.to_ab.len(), self.from_ab.len())
    }

    fn pop_pair(
        to_ab: &mut BinaryHeap<ToOrder>,
        from_ab: &mut BinaryHeap<FromOrder>,
    ) -> Option<(ToOrder, FromOrder)> {
        let to = to_ab.peek()?;
        let from = from_ab.peek()?;
        if from.info.price < to.info.price {
            debug!("Trade not viable");
            return None;
        }
        let to = to_ab.pop()?;
        let from = from_ab.pop()?;
        Some((to, from))
    }
}
