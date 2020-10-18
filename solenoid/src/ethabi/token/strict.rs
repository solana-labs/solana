// Copyright 2015-2020 Parity Technologies
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::ethabi::errors::Error;
use crate::ethabi::token::Tokenizer;
use hex::FromHex;

/// Tries to parse string as a token. Require string to clearly represent the value.
pub struct StrictTokenizer;

impl Tokenizer for StrictTokenizer {
	fn tokenize_address(value: &str) -> Result<[u8; 20], Error> {
		let hex: Vec<u8> = value.from_hex()?;
		match hex.len() == 20 {
			false => Err(Error::InvalidData),
			true => {
				let mut address = [0u8; 20];
				address.copy_from_slice(&hex);
				Ok(address)
			}
		}
	}

	fn tokenize_string(value: &str) -> Result<String, Error> {
		Ok(value.to_owned())
	}

	fn tokenize_bool(value: &str) -> Result<bool, Error> {
		match value {
			"true" | "1" => Ok(true),
			"false" | "0" => Ok(false),
			_ => Err(Error::InvalidData),
		}
	}

	fn tokenize_bytes(value: &str) -> Result<Vec<u8>, Error> {
		value.from_hex().map_err(Into::into)
	}

	fn tokenize_fixed_bytes(value: &str, len: usize) -> Result<Vec<u8>, Error> {
		let hex: Vec<u8> = value.from_hex()?;
		match hex.len() == len {
			true => Ok(hex),
			false => Err(Error::InvalidData),
		}
	}

	fn tokenize_uint(value: &str) -> Result<[u8; 32], Error> {
		let hex: Vec<u8> = value.from_hex()?;
		match hex.len() == 32 {
			true => {
				let mut uint = [0u8; 32];
				uint.copy_from_slice(&hex);
				Ok(uint)
			}
			false => Err(Error::InvalidData),
		}
	}

	fn tokenize_int(value: &str) -> Result<[u8; 32], Error> {
		Self::tokenize_uint(value)
	}
}

#[cfg(test)]
mod tests {
	use crate::token::{StrictTokenizer, Token, Tokenizer};
	use crate::ParamType;

	#[test]
	fn tokenize_address() {
		assert_eq!(
			StrictTokenizer::tokenize(&ParamType::Address, "1111111111111111111111111111111111111111").unwrap(),
			Token::Address([0x11u8; 20].into())
		);
		assert_eq!(
			StrictTokenizer::tokenize(&ParamType::Address, "2222222222222222222222222222222222222222").unwrap(),
			Token::Address([0x22u8; 20].into())
		);
	}

	#[test]
	fn tokenize_string() {
		assert_eq!(
			StrictTokenizer::tokenize(&ParamType::String, "gavofyork").unwrap(),
			Token::String("gavofyork".to_owned())
		);
		assert_eq!(StrictTokenizer::tokenize(&ParamType::String, "hello").unwrap(), Token::String("hello".to_owned()));
	}

	#[test]
	fn tokenize_bool() {
		assert_eq!(StrictTokenizer::tokenize(&ParamType::Bool, "true").unwrap(), Token::Bool(true));
		assert_eq!(StrictTokenizer::tokenize(&ParamType::Bool, "1").unwrap(), Token::Bool(true));
		assert_eq!(StrictTokenizer::tokenize(&ParamType::Bool, "false").unwrap(), Token::Bool(false));
		assert_eq!(StrictTokenizer::tokenize(&ParamType::Bool, "0").unwrap(), Token::Bool(false));
	}

	#[test]
	fn tokenize_bytes() {
		assert_eq!(
			StrictTokenizer::tokenize(&ParamType::Bytes, "123456").unwrap(),
			Token::Bytes(vec![0x12, 0x34, 0x56])
		);
		assert_eq!(StrictTokenizer::tokenize(&ParamType::Bytes, "0017").unwrap(), Token::Bytes(vec![0x00, 0x17]));
	}

	#[test]
	fn tokenize_fixed_bytes() {
		assert_eq!(
			StrictTokenizer::tokenize(&ParamType::FixedBytes(3), "123456").unwrap(),
			Token::FixedBytes(vec![0x12, 0x34, 0x56])
		);
		assert_eq!(
			StrictTokenizer::tokenize(&ParamType::FixedBytes(2), "0017").unwrap(),
			Token::FixedBytes(vec![0x00, 0x17])
		);
	}

	#[test]
	fn tokenize_uint() {
		assert_eq!(
			StrictTokenizer::tokenize(
				&ParamType::Uint(256),
				"1111111111111111111111111111111111111111111111111111111111111111"
			)
			.unwrap(),
			Token::Uint([0x11u8; 32].into())
		);

		assert_eq!(
			StrictTokenizer::tokenize(
				&ParamType::Uint(256),
				"2222222222222222222222222222222222222222222222222222222222222222"
			)
			.unwrap(),
			Token::Uint([0x22u8; 32].into())
		);
	}

	#[test]
	fn tokenize_int() {
		assert_eq!(
			StrictTokenizer::tokenize(
				&ParamType::Int(256),
				"1111111111111111111111111111111111111111111111111111111111111111"
			)
			.unwrap(),
			Token::Int([0x11u8; 32].into())
		);

		assert_eq!(
			StrictTokenizer::tokenize(
				&ParamType::Int(256),
				"2222222222222222222222222222222222222222222222222222222222222222"
			)
			.unwrap(),
			Token::Int([0x22u8; 32].into())
		);
	}

	#[test]
	fn tokenize_empty_array() {
		assert_eq!(
			StrictTokenizer::tokenize(&ParamType::Array(Box::new(ParamType::Bool)), "[]").unwrap(),
			Token::Array(vec![])
		);
	}

	#[test]
	fn tokenize_bool_array() {
		assert_eq!(
			StrictTokenizer::tokenize(&ParamType::Array(Box::new(ParamType::Bool)), "[true,1,0,false]").unwrap(),
			Token::Array(vec![Token::Bool(true), Token::Bool(true), Token::Bool(false), Token::Bool(false)])
		);
	}

	#[test]
	fn tokenize_bool_array_of_arrays() {
		assert_eq!(
			StrictTokenizer::tokenize(
				&ParamType::Array(Box::new(ParamType::Array(Box::new(ParamType::Bool)))),
				"[[true,1,0],[false]]"
			)
			.unwrap(),
			Token::Array(vec![
				Token::Array(vec![Token::Bool(true), Token::Bool(true), Token::Bool(false)]),
				Token::Array(vec![Token::Bool(false)])
			])
		);
	}
}
