//! Solana Rust-based SBF program utility functions and types

#![allow(clippy::arithmetic_side_effects)]

extern crate solana_program;

pub fn uadd(x: u128, y: u128) -> u128 {
    x + y
}
pub fn usubtract(x: u128, y: u128) -> u128 {
    x - y
}
pub fn umultiply(x: u128, y: u128) -> u128 {
    x * y
}
pub fn udivide(n: u128, d: u128) -> u128 {
    n / d
}
pub fn umodulo(n: u128, d: u128) -> u128 {
    n % d
}

pub fn add(x: i128, y: i128) -> i128 {
    x + y
}
pub fn subtract(x: i128, y: i128) -> i128 {
    x - y
}
pub fn multiply(x: i128, y: i128) -> i128 {
    x * y
}
pub fn divide(n: i128, d: i128) -> i128 {
    n / d
}
pub fn modulo(n: i128, d: i128) -> i128 {
    n % d
}
