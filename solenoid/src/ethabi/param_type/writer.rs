// Copyright 2015-2020 Parity Technologies
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::ethabi::ParamType;

/// Output formatter for param type.
pub struct Writer;

impl Writer {
	/// Returns string which is a formatted represenation of param.
	pub fn write(param: &ParamType) -> String {
		match *param {
			ParamType::Address => "address".to_owned(),
			ParamType::Bytes => "bytes".to_owned(),
			ParamType::FixedBytes(len) => format!("bytes{}", len),
			ParamType::Int(len) => format!("int{}", len),
			ParamType::Uint(len) => format!("uint{}", len),
			ParamType::Bool => "bool".to_owned(),
			ParamType::String => "string".to_owned(),
			ParamType::FixedArray(ref param, len) => format!("{}[{}]", Writer::write(param), len),
			ParamType::Array(ref param) => format!("{}[]", Writer::write(param)),
			ParamType::Tuple(ref params) => {
				format!("({})", params.iter().map(|ref t| format!("{}", t)).collect::<Vec<String>>().join(","))
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::Writer;
	use crate::ParamType;

	#[test]
	fn test_write_param() {
		assert_eq!(Writer::write(&ParamType::Address), "address".to_owned());
		assert_eq!(Writer::write(&ParamType::Bytes), "bytes".to_owned());
		assert_eq!(Writer::write(&ParamType::FixedBytes(32)), "bytes32".to_owned());
		assert_eq!(Writer::write(&ParamType::Uint(256)), "uint256".to_owned());
		assert_eq!(Writer::write(&ParamType::Int(64)), "int64".to_owned());
		assert_eq!(Writer::write(&ParamType::Bool), "bool".to_owned());
		assert_eq!(Writer::write(&ParamType::String), "string".to_owned());
		assert_eq!(Writer::write(&ParamType::Array(Box::new(ParamType::Bool))), "bool[]".to_owned());
		assert_eq!(Writer::write(&ParamType::FixedArray(Box::new(ParamType::String), 2)), "string[2]".to_owned());
		assert_eq!(
			Writer::write(&ParamType::FixedArray(Box::new(ParamType::Array(Box::new(ParamType::Bool))), 2)),
			"bool[][2]".to_owned()
		);
	}
}
