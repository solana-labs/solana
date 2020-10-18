// Copyright 2015-2020 Parity Technologies
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::ethabi::{Error, ParamType};

/// Used to convert param type represented as a string to rust structure.
pub struct Reader;

impl Reader {
	/// Converts string to param type.
	pub fn read(name: &str) -> Result<ParamType, Error> {
		match name.chars().last() {
			// check if it is a struct
			Some(')') => {
				if !name.starts_with('(') {
					return Err(Error::InvalidName(name.to_owned()));
				};

				let mut subtypes = Vec::new();
				let mut subtuples = Vec::new();
				let mut nested = 0isize;
				let mut last_item = 1;

				// Iterate over name and build the nested tuple structure
				for (pos, c) in name.chars().enumerate() {
					match c {
						'(' => {
							nested += 1;
							// If an '(' is encountered within the tuple
							// insert an empty subtuples vector to be filled
							if nested > 1 {
								subtuples.push(vec![]);
								last_item = pos + 1;
							}
						}
						')' => {
							nested -= 1;
							// End parsing and return an error if parentheses aren't symmetrical
							if nested < 0 {
								return Err(Error::InvalidName(name.to_owned()));
							}
							// If there have not been any characters since the last item
							// increment position without inserting any subtypes
							else if name[last_item..pos].len() < 1 {
								last_item = pos + 1;
							}
							// If the item is in the top level of the tuple insert it into subtypes
							else if nested == 0 {
								let sub = &name[last_item..pos];
								let subtype = Reader::read(sub)?;
								subtypes.push(Box::new(subtype));
								last_item = pos + 1;
							}
							// If the item is in a sublevel of the tuple:
							// insert it into the subtuple vector for the current depth level
							// process all the subtuple vectors created into sub tuples and insert
							// them into subtypes
							else if nested > 0 {
								let sub = &name[last_item..pos];
								let subtype = Reader::read(sub)?;
								subtuples[(nested - 1) as usize].push(Box::new(subtype));
								let initial_tuple_params = subtuples.remove(0);
								let tuple_params = subtuples.into_iter().fold(
									initial_tuple_params,
									|mut tuple_params, nested_param_set| {
										tuple_params.push(Box::new(ParamType::Tuple(nested_param_set)));
										tuple_params
									},
								);
								subtypes.push(Box::new(ParamType::Tuple(tuple_params)));
								subtuples = Vec::new();
								last_item = pos + 1;
							}
						}
						',' => {
							// If there have not been any characters since the last item
							// increment position without inserting any subtypes
							if name[last_item..pos].len() < 1 {
								last_item = pos + 1
							}
							// If the item is in the top level of the tuple insert it into subtypes
							else if nested == 1 {
								let sub = &name[last_item..pos];
								let subtype = Reader::read(sub)?;
								subtypes.push(Box::new(subtype));
								last_item = pos + 1;
							}
							// If the item is in a sublevel of the tuple
							// insert it into the subtuple vector for the current depth level
							else if nested > 1 {
								let sub = &name[last_item..pos];
								let subtype = Reader::read(sub)?;
								subtuples[(nested - 2) as usize].push(Box::new(subtype));
								last_item = pos + 1;
							}
						}
						_ => (),
					}
				}
				return Ok(ParamType::Tuple(subtypes));
			}
			// check if it is a fixed or dynamic array.
			Some(']') => {
				// take number part
				let num: String =
					name.chars().rev().skip(1).take_while(|c| *c != '[').collect::<String>().chars().rev().collect();

				let count = name.chars().count();
				if num.is_empty() {
					// we already know it's a dynamic array!
					let subtype = Reader::read(&name[..count - 2])?;
					return Ok(ParamType::Array(Box::new(subtype)));
				} else {
					// it's a fixed array.
					let len = usize::from_str_radix(&num, 10)?;
					let subtype = Reader::read(&name[..count - num.len() - 2])?;
					return Ok(ParamType::FixedArray(Box::new(subtype), len));
				}
			}
			_ => (),
		}

		let result = match name {
			"address" => ParamType::Address,
			"bytes" => ParamType::Bytes,
			"bool" => ParamType::Bool,
			"string" => ParamType::String,
			"int" => ParamType::Int(256),
			"tuple" => ParamType::Tuple(vec![]),
			"uint" => ParamType::Uint(256),
			s if s.starts_with("int") => {
				let len = usize::from_str_radix(&s[3..], 10)?;
				ParamType::Int(len)
			}
			s if s.starts_with("uint") => {
				let len = usize::from_str_radix(&s[4..], 10)?;
				ParamType::Uint(len)
			}
			s if s.starts_with("bytes") => {
				let len = usize::from_str_radix(&s[5..], 10)?;
				ParamType::FixedBytes(len)
			}
			_ => {
				return Err(Error::InvalidName(name.to_owned()));
			}
		};

		Ok(result)
	}
}

#[cfg(test)]
mod tests {
	use super::Reader;
	use crate::ParamType;

	#[test]
	fn test_read_param() {
		assert_eq!(Reader::read("address").unwrap(), ParamType::Address);
		assert_eq!(Reader::read("bytes").unwrap(), ParamType::Bytes);
		assert_eq!(Reader::read("bytes32").unwrap(), ParamType::FixedBytes(32));
		assert_eq!(Reader::read("bool").unwrap(), ParamType::Bool);
		assert_eq!(Reader::read("string").unwrap(), ParamType::String);
		assert_eq!(Reader::read("int").unwrap(), ParamType::Int(256));
		assert_eq!(Reader::read("uint").unwrap(), ParamType::Uint(256));
		assert_eq!(Reader::read("int32").unwrap(), ParamType::Int(32));
		assert_eq!(Reader::read("uint32").unwrap(), ParamType::Uint(32));
	}

	#[test]
	fn test_read_array_param() {
		assert_eq!(Reader::read("address[]").unwrap(), ParamType::Array(Box::new(ParamType::Address)));
		assert_eq!(Reader::read("uint[]").unwrap(), ParamType::Array(Box::new(ParamType::Uint(256))));
		assert_eq!(Reader::read("bytes[]").unwrap(), ParamType::Array(Box::new(ParamType::Bytes)));
		assert_eq!(
			Reader::read("bool[][]").unwrap(),
			ParamType::Array(Box::new(ParamType::Array(Box::new(ParamType::Bool))))
		);
	}

	#[test]
	fn test_read_fixed_array_param() {
		assert_eq!(Reader::read("address[2]").unwrap(), ParamType::FixedArray(Box::new(ParamType::Address), 2));
		assert_eq!(Reader::read("bool[17]").unwrap(), ParamType::FixedArray(Box::new(ParamType::Bool), 17));
		assert_eq!(
			Reader::read("bytes[45][3]").unwrap(),
			ParamType::FixedArray(Box::new(ParamType::FixedArray(Box::new(ParamType::Bytes), 45)), 3)
		);
	}

	#[test]
	fn test_read_mixed_arrays() {
		assert_eq!(
			Reader::read("bool[][3]").unwrap(),
			ParamType::FixedArray(Box::new(ParamType::Array(Box::new(ParamType::Bool))), 3)
		);
		assert_eq!(
			Reader::read("bool[3][]").unwrap(),
			ParamType::Array(Box::new(ParamType::FixedArray(Box::new(ParamType::Bool), 3)))
		);
	}

	#[test]
	fn test_read_struct_param() {
		assert_eq!(
			Reader::read("(address,bool)").unwrap(),
			ParamType::Tuple(vec![Box::new(ParamType::Address), Box::new(ParamType::Bool)])
		);
		assert_eq!(
			Reader::read("(bool[3],uint256)").unwrap(),
			ParamType::Tuple(vec![
				Box::new(ParamType::FixedArray(Box::new(ParamType::Bool), 3)),
				Box::new(ParamType::Uint(256))
			])
		);
	}

	#[test]
	fn test_read_nested_struct_param() {
		assert_eq!(
			Reader::read("(address,bool,(bool,uint256))").unwrap(),
			ParamType::Tuple(vec![
				Box::new(ParamType::Address),
				Box::new(ParamType::Bool),
				Box::new(ParamType::Tuple(vec![Box::new(ParamType::Bool), Box::new(ParamType::Uint(256))]))
			])
		);
	}

	#[test]
	fn test_read_complex_nested_struct_param() {
		assert_eq!(
			Reader::read("(address,bool,(bool,uint256,(bool,uint256)),(bool,uint256))").unwrap(),
			ParamType::Tuple(vec![
				Box::new(ParamType::Address),
				Box::new(ParamType::Bool),
				Box::new(ParamType::Tuple(vec![
					Box::new(ParamType::Bool),
					Box::new(ParamType::Uint(256)),
					Box::new(ParamType::Tuple(vec![Box::new(ParamType::Bool), Box::new(ParamType::Uint(256))]))
				])),
				Box::new(ParamType::Tuple(vec![Box::new(ParamType::Bool), Box::new(ParamType::Uint(256))]))
			])
		);
	}
}
