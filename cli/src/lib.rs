#[macro_use]
extern crate lazy_static;

pub mod config;
pub mod display;
pub mod input_validators;
pub mod validator_info;
pub mod wallet;

pub(crate) fn lamports_to_sol(lamports: u64) -> f64 {
    lamports as f64 / 2u64.pow(34) as f64
}
