// Copyright 2015-2020 Parity Technologies
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Function and event param types.

use super::Writer;
use std::fmt;

/// Function and event param types.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamType {
	/// Address.
	Address,
	/// Bytes.
	Bytes,
	/// Signed integer.
	Int(usize),
	/// Unsigned integer.
	Uint(usize),
	/// Boolean.
	Bool,
	/// String.
	String,
	/// Array of unknown size.
	Array(Box<ParamType>),
	/// Vector of bytes with fixed size.
	FixedBytes(usize),
	/// Array with fixed size.
	FixedArray(Box<ParamType>, usize),
	/// Tuple containing different types
	Tuple(Vec<Box<ParamType>>),
}

impl fmt::Display for ParamType {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{}", Writer::write(self))
	}
}

impl ParamType {
	/// returns whether a zero length byte slice (`0x`) is
	/// a valid encoded form of this param type
	pub fn is_empty_bytes_valid_encoding(&self) -> bool {
		match self {
			ParamType::FixedBytes(len) => *len == 0,
			ParamType::FixedArray(_, len) => *len == 0,
			_ => false,
		}
	}

	/// returns whether a ParamType is dynamic
	/// used to decide how the ParamType should be encoded
	pub fn is_dynamic(&self) -> bool {
		match self {
			ParamType::Bytes | ParamType::String | ParamType::Array(_) => true,
			ParamType::FixedArray(elem_type, _) => elem_type.is_dynamic(),
			ParamType::Tuple(params) => params.iter().any(|param| param.is_dynamic()),
			_ => false,
		}
	}
}

#[cfg(test)]
mod tests {
	use crate::ParamType;

	#[test]
	fn test_param_type_display() {
		assert_eq!(format!("{}", ParamType::Address), "address".to_owned());
		assert_eq!(format!("{}", ParamType::Bytes), "bytes".to_owned());
		assert_eq!(format!("{}", ParamType::FixedBytes(32)), "bytes32".to_owned());
		assert_eq!(format!("{}", ParamType::Uint(256)), "uint256".to_owned());
		assert_eq!(format!("{}", ParamType::Int(64)), "int64".to_owned());
		assert_eq!(format!("{}", ParamType::Bool), "bool".to_owned());
		assert_eq!(format!("{}", ParamType::String), "string".to_owned());
		assert_eq!(format!("{}", ParamType::Array(Box::new(ParamType::Bool))), "bool[]".to_owned());
		assert_eq!(format!("{}", ParamType::FixedArray(Box::new(ParamType::Uint(256)), 2)), "uint256[2]".to_owned());
		assert_eq!(format!("{}", ParamType::FixedArray(Box::new(ParamType::String), 2)), "string[2]".to_owned());
		assert_eq!(
			format!("{}", ParamType::FixedArray(Box::new(ParamType::Array(Box::new(ParamType::Bool))), 2)),
			"bool[][2]".to_owned()
		);
	}

	#[test]
	fn test_is_dynamic() {
		assert_eq!(ParamType::Address.is_dynamic(), false);
		assert_eq!(ParamType::Bytes.is_dynamic(), true);
		assert_eq!(ParamType::FixedBytes(32).is_dynamic(), false);
		assert_eq!(ParamType::Uint(256).is_dynamic(), false);
		assert_eq!(ParamType::Int(64).is_dynamic(), false);
		assert_eq!(ParamType::Bool.is_dynamic(), false);
		assert_eq!(ParamType::String.is_dynamic(), true);
		assert_eq!(ParamType::Array(Box::new(ParamType::Bool)).is_dynamic(), true);
		assert_eq!(ParamType::FixedArray(Box::new(ParamType::Uint(256)), 2).is_dynamic(), false);
		assert_eq!(ParamType::FixedArray(Box::new(ParamType::String), 2).is_dynamic(), true);
		assert_eq!(ParamType::FixedArray(Box::new(ParamType::Array(Box::new(ParamType::Bool))), 2).is_dynamic(), true);
	}
}
