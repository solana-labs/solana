// Copyright 2015-2020 Parity Technologies
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::{fmt, num, result::Result as StdResult, string};

/// Ethabi result type
pub type Result<T> = std::result::Result<T, Error>;

/// Ethabi errors
#[derive(Debug)]
pub enum Error {
	/// Invalid entity such as a bad function name.
	InvalidName(String),
	/// Invalid data.
	InvalidData,
	/// Serialization error.
	SerdeJson(serde_json::Error),
	/// Integer parsing error.
	ParseInt(num::ParseIntError),
	/// UTF-8 parsing error.
	Utf8(string::FromUtf8Error),
	/// Hex string parsing error.
	Hex(hex::FromHexError),
	/// Other errors.
	Other(String),
}

impl std::error::Error for Error {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		use self::Error::*;
		match self {
			SerdeJson(e) => Some(e),
			ParseInt(e) => Some(e),
			Utf8(e) => Some(e),
			Hex(e) => Some(e),
			_ => None,
		}
	}
}

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter) -> StdResult<(), fmt::Error> {
		use self::Error::*;
		match *self {
			InvalidName(ref name) => write!(f, "Invalid name `{}`", name),
			InvalidData => write!(f, "Invalid data"),
			SerdeJson(ref err) => write!(f, "Serialization error: {}", err),
			ParseInt(ref err) => write!(f, "Integer parsing error: {}", err),
			Utf8(ref err) => write!(f, "UTF-8 parsing error: {}", err),
			Hex(ref err) => write!(f, "Hex parsing error: {}", err),
			Other(ref error_string) => write!(f, "{}", error_string),
		}
	}
}

impl From<&str> for Error {
	fn from(err: &str) -> Self {
		Error::Other(err.to_owned())
	}
}
impl From<String> for Error {
	fn from(err: String) -> Self {
		Error::Other(err)
	}
}
impl From<serde_json::Error> for Error {
	fn from(err: serde_json::Error) -> Self {
		Error::SerdeJson(err)
	}
}
impl From<num::ParseIntError> for Error {
	fn from(err: num::ParseIntError) -> Self {
		Error::ParseInt(err)
	}
}
impl From<uint::FromDecStrErr> for Error {
	fn from(err: uint::FromDecStrErr) -> Self {
		use uint::FromDecStrErr::*;
		match err {
			InvalidCharacter => Error::Other("Uint parse error: InvalidCharacter".into()),
			InvalidLength => Error::Other("Uint parse error: InvalidLength".into()),
		}
	}
}
impl From<string::FromUtf8Error> for Error {
	fn from(err: string::FromUtf8Error) -> Self {
		Error::Utf8(err)
	}
}
impl From<hex::FromHexError> for Error {
	fn from(err: hex::FromHexError) -> Self {
		Error::Hex(err)
	}
}
