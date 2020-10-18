// Copyright 2015-2020 Parity Technologies
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Ethereum ABI params.
use crate::ethabi::{Address, Bytes, FixedBytes, ParamType, Uint};
use hex::ToHex;
use std::fmt;

/// Ethereum ABI params.
#[derive(Debug, PartialEq, Clone)]
pub enum Token {
	/// Address.
	///
	/// solidity name: address
	/// Encoded to left padded [0u8; 32].
	Address(Address),
	/// Vector of bytes with known size.
	///
	/// solidity name eg.: bytes8, bytes32, bytes64, bytes1024
	/// Encoded to right padded [0u8; ((N + 31) / 32) * 32].
	FixedBytes(FixedBytes),
	/// Vector of bytes of unknown size.
	///
	/// solidity name: bytes
	/// Encoded in two parts.
	/// Init part: offset of 'closing part`.
	/// Closing part: encoded length followed by encoded right padded bytes.
	Bytes(Bytes),
	/// Signed integer.
	///
	/// solidity name: int
	Int(Uint),
	/// Unisnged integer.
	///
	/// solidity name: uint
	Uint(Uint),
	/// Boolean value.
	///
	/// solidity name: bool
	/// Encoded as left padded [0u8; 32], where last bit represents boolean value.
	Bool(bool),
	/// String.
	///
	/// solidity name: string
	/// Encoded in the same way as bytes. Must be utf8 compliant.
	String(String),
	/// Array with known size.
	///
	/// solidity name eg.: int[3], bool[3], address[][8]
	/// Encoding of array is equal to encoding of consecutive elements of array.
	FixedArray(Vec<Token>),
	/// Array of params with unknown size.
	///
	/// solidity name eg. int[], bool[], address[5][]
	Array(Vec<Token>),
	/// Tuple of params of variable types.
	///
	/// solidity name: tuple
	Tuple(Vec<Token>),
}

impl fmt::Display for Token {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match *self {
			Token::Bool(b) => write!(f, "{}", b),
			Token::String(ref s) => write!(f, "{}", s),
			Token::Address(ref a) => write!(f, "{:x}", a),
			Token::Bytes(ref bytes) | Token::FixedBytes(ref bytes) => write!(f, "{}", bytes.to_hex::<String>()),
			Token::Uint(ref i) | Token::Int(ref i) => write!(f, "{:x}", i),
			Token::Array(ref arr) | Token::FixedArray(ref arr) => {
				let s = arr.iter().map(|ref t| format!("{}", t)).collect::<Vec<String>>().join(",");

				write!(f, "[{}]", s)
			}
			Token::Tuple(ref s) => {
				let s = s.iter().map(|ref t| format!("{}", t)).collect::<Vec<String>>().join(",");

				write!(f, "({})", s)
			}
		}
	}
}

impl Token {
	/// Check whether the type of the token matches the given parameter type.
	///
	/// Numeric types (`Int` and `Uint`) type check if the size of the token
	/// type is of greater or equal size than the provided parameter type.
	pub fn type_check(&self, param_type: &ParamType) -> bool {
		match *self {
			Token::Address(_) => *param_type == ParamType::Address,
			Token::Bytes(_) => *param_type == ParamType::Bytes,
			Token::Int(_) => {
				if let ParamType::Int(_) = *param_type {
					true
				} else {
					false
				}
			}
			Token::Uint(_) => {
				if let ParamType::Uint(_) = *param_type {
					true
				} else {
					false
				}
			}
			Token::Bool(_) => *param_type == ParamType::Bool,
			Token::String(_) => *param_type == ParamType::String,
			Token::FixedBytes(ref bytes) => {
				if let ParamType::FixedBytes(size) = *param_type {
					size >= bytes.len()
				} else {
					false
				}
			}
			Token::Array(ref tokens) => {
				if let ParamType::Array(ref param_type) = *param_type {
					tokens.iter().all(|t| t.type_check(param_type))
				} else {
					false
				}
			}
			Token::FixedArray(ref tokens) => {
				if let ParamType::FixedArray(ref param_type, size) = *param_type {
					size == tokens.len() && tokens.iter().all(|t| t.type_check(param_type))
				} else {
					false
				}
			}
			Token::Tuple(ref tokens) => {
				if let ParamType::Tuple(ref param_type) = *param_type {
					tokens.iter().enumerate().all(|(i, t)| t.type_check(&param_type[i]))
				} else {
					false
				}
			}
		}
	}

	/// Converts token to...
	pub fn to_address(self) -> Option<Address> {
		match self {
			Token::Address(address) => Some(address),
			_ => None,
		}
	}

	/// Converts token to...
	pub fn to_fixed_bytes(self) -> Option<Vec<u8>> {
		match self {
			Token::FixedBytes(bytes) => Some(bytes),
			_ => None,
		}
	}

	/// Converts token to...
	pub fn to_bytes(self) -> Option<Vec<u8>> {
		match self {
			Token::Bytes(bytes) => Some(bytes),
			_ => None,
		}
	}

	/// Converts token to...
	pub fn to_int(self) -> Option<Uint> {
		match self {
			Token::Int(int) => Some(int),
			_ => None,
		}
	}

	/// Converts token to...
	pub fn to_uint(self) -> Option<Uint> {
		match self {
			Token::Uint(uint) => Some(uint),
			_ => None,
		}
	}

	/// Converts token to...
	pub fn to_bool(self) -> Option<bool> {
		match self {
			Token::Bool(b) => Some(b),
			_ => None,
		}
	}

	/// Converts token to...
	pub fn to_string(self) -> Option<String> {
		match self {
			Token::String(s) => Some(s),
			_ => None,
		}
	}

	/// Converts token to...
	pub fn to_fixed_array(self) -> Option<Vec<Token>> {
		match self {
			Token::FixedArray(arr) => Some(arr),
			_ => None,
		}
	}

	/// Converts token to...
	pub fn to_array(self) -> Option<Vec<Token>> {
		match self {
			Token::Array(arr) => Some(arr),
			_ => None,
		}
	}

	/// Check if all the types of the tokens match the given parameter types.
	pub fn types_check(tokens: &[Token], param_types: &[ParamType]) -> bool {
		param_types.len() == tokens.len() && {
			param_types.iter().zip(tokens).all(|(param_type, token)| token.type_check(param_type))
		}
	}

	/// Check if the token is a dynamic type resulting in prefixed encoding
	pub fn is_dynamic(&self) -> bool {
		match self {
			Token::Bytes(_) | Token::String(_) | Token::Array(_) => true,
			Token::FixedArray(tokens) => tokens.iter().any(|token| token.is_dynamic()),
			Token::Tuple(tokens) => tokens.iter().any(|token| token.is_dynamic()),
			_ => false,
		}
	}
}

#[cfg(test)]
mod tests {
	use crate::{ParamType, Token};

	#[test]
	fn test_type_check() {
		fn assert_type_check(tokens: Vec<Token>, param_types: Vec<ParamType>) {
			assert!(Token::types_check(&tokens, &param_types))
		}

		fn assert_not_type_check(tokens: Vec<Token>, param_types: Vec<ParamType>) {
			assert!(!Token::types_check(&tokens, &param_types))
		}

		assert_type_check(vec![Token::Uint(0.into()), Token::Bool(false)], vec![ParamType::Uint(256), ParamType::Bool]);
		assert_type_check(vec![Token::Uint(0.into()), Token::Bool(false)], vec![ParamType::Uint(32), ParamType::Bool]);

		assert_not_type_check(vec![Token::Uint(0.into())], vec![ParamType::Uint(32), ParamType::Bool]);
		assert_not_type_check(vec![Token::Uint(0.into()), Token::Bool(false)], vec![ParamType::Uint(32)]);
		assert_not_type_check(
			vec![Token::Bool(false), Token::Uint(0.into())],
			vec![ParamType::Uint(32), ParamType::Bool],
		);

		assert_type_check(vec![Token::FixedBytes(vec![0, 0, 0, 0])], vec![ParamType::FixedBytes(4)]);
		assert_type_check(vec![Token::FixedBytes(vec![0, 0, 0])], vec![ParamType::FixedBytes(4)]);
		assert_not_type_check(vec![Token::FixedBytes(vec![0, 0, 0, 0])], vec![ParamType::FixedBytes(3)]);

		assert_type_check(
			vec![Token::Array(vec![Token::Bool(false), Token::Bool(true)])],
			vec![ParamType::Array(Box::new(ParamType::Bool))],
		);
		assert_not_type_check(
			vec![Token::Array(vec![Token::Bool(false), Token::Uint(0.into())])],
			vec![ParamType::Array(Box::new(ParamType::Bool))],
		);
		assert_not_type_check(
			vec![Token::Array(vec![Token::Bool(false), Token::Bool(true)])],
			vec![ParamType::Array(Box::new(ParamType::Address))],
		);

		assert_type_check(
			vec![Token::FixedArray(vec![Token::Bool(false), Token::Bool(true)])],
			vec![ParamType::FixedArray(Box::new(ParamType::Bool), 2)],
		);
		assert_not_type_check(
			vec![Token::FixedArray(vec![Token::Bool(false), Token::Bool(true)])],
			vec![ParamType::FixedArray(Box::new(ParamType::Bool), 3)],
		);
		assert_not_type_check(
			vec![Token::FixedArray(vec![Token::Bool(false), Token::Uint(0.into())])],
			vec![ParamType::FixedArray(Box::new(ParamType::Bool), 2)],
		);
		assert_not_type_check(
			vec![Token::FixedArray(vec![Token::Bool(false), Token::Bool(true)])],
			vec![ParamType::FixedArray(Box::new(ParamType::Address), 2)],
		);
	}

	#[test]
	fn test_is_dynamic() {
		assert_eq!(Token::Address("0000000000000000000000000000000000000000".parse().unwrap()).is_dynamic(), false);
		assert_eq!(Token::Bytes(vec![0, 0, 0, 0]).is_dynamic(), true);
		assert_eq!(Token::FixedBytes(vec![0, 0, 0, 0]).is_dynamic(), false);
		assert_eq!(Token::Uint(0.into()).is_dynamic(), false);
		assert_eq!(Token::Int(0.into()).is_dynamic(), false);
		assert_eq!(Token::Bool(false).is_dynamic(), false);
		assert_eq!(Token::String("".into()).is_dynamic(), true);
		assert_eq!(Token::Array(vec![Token::Bool(false)]).is_dynamic(), true);
		assert_eq!(Token::FixedArray(vec![Token::Uint(0.into())]).is_dynamic(), false);
		assert_eq!(Token::FixedArray(vec![Token::String("".into())]).is_dynamic(), true);
		assert_eq!(Token::FixedArray(vec![Token::Array(vec![Token::Bool(false)])]).is_dynamic(), true);
	}
}
