// Copyright 2015-2020 Parity Technologies
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::ethabi::errors::Error;
use crate::ethabi::token::{StrictTokenizer, Tokenizer};
use crate::ethabi::Uint;

/// Tries to parse string as a token. Does not require string to clearly represent the value.
pub struct LenientTokenizer;

impl Tokenizer for LenientTokenizer {
	fn tokenize_address(value: &str) -> Result<[u8; 20], Error> {
		StrictTokenizer::tokenize_address(value)
	}

	fn tokenize_string(value: &str) -> Result<String, Error> {
		StrictTokenizer::tokenize_string(value)
	}

	fn tokenize_bool(value: &str) -> Result<bool, Error> {
		StrictTokenizer::tokenize_bool(value)
	}

	fn tokenize_bytes(value: &str) -> Result<Vec<u8>, Error> {
		StrictTokenizer::tokenize_bytes(value)
	}

	fn tokenize_fixed_bytes(value: &str, len: usize) -> Result<Vec<u8>, Error> {
		StrictTokenizer::tokenize_fixed_bytes(value, len)
	}

	fn tokenize_uint(value: &str) -> Result<[u8; 32], Error> {
		let result = StrictTokenizer::tokenize_uint(value);
		if result.is_ok() {
			return result;
		}

		let uint = Uint::from_dec_str(value)?;
		Ok(uint.into())
	}

	// We don't have a proper signed int 256-bit long type, so here we're cheating. We build a U256
	// out of it and check that it's within the lower/upper bound of a hypothetical I256 type: half
	// the `U256::max_value().
	fn tokenize_int(value: &str) -> Result<[u8; 32], Error> {
		let result = StrictTokenizer::tokenize_int(value);
		if result.is_ok() {
			return result;
		}

		let abs = Uint::from_dec_str(value.trim_start_matches('-'))?;
		let max = Uint::max_value() / 2;
		let int = if value.starts_with('-') {
			if abs.is_zero() {
				return Ok(abs.into());
			} else if abs > max + 1 {
				return Err(Error::Other("int256 parse error: Underflow".into()));
			}
			!abs + 1 // two's complement
		} else {
			if abs > max {
				return Err(Error::Other("int256 parse error: Overflow".into()));
			}
			abs
		};
		Ok(int.into())
	}
}
