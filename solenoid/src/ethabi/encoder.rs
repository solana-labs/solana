// Copyright 2015-2020 Parity Technologies
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! ABI encoder.

use crate::ethabi::util::pad_u32;
use crate::ethabi::{Bytes, Token, Word};

fn pad_bytes(bytes: &[u8]) -> Vec<Word> {
	let mut result = vec![pad_u32(bytes.len() as u32)];
	result.extend(pad_fixed_bytes(bytes));
	result
}

fn pad_fixed_bytes(bytes: &[u8]) -> Vec<Word> {
	let len = (bytes.len() + 31) / 32;
	let mut result = Vec::with_capacity(len);
	for i in 0..len {
		let mut padded = [0u8; 32];

		let to_copy = match i == len - 1 {
			false => 32,
			true => match bytes.len() % 32 {
				0 => 32,
				x => x,
			},
		};

		let offset = 32 * i;
		padded[..to_copy].copy_from_slice(&bytes[offset..offset + to_copy]);
		result.push(padded);
	}

	result
}

#[derive(Debug)]
enum Mediate {
	Raw(Vec<Word>),
	Prefixed(Vec<Word>),
	PrefixedArray(Vec<Mediate>),
	PrefixedArrayWithLength(Vec<Mediate>),
	RawTuple(Vec<Mediate>),
	PrefixedTuple(Vec<Mediate>),
}

impl Mediate {
	fn head_len(&self) -> u32 {
		match *self {
			Mediate::Raw(ref raw) => 32 * raw.len() as u32,
			Mediate::RawTuple(ref mediates) => 32 * mediates.len() as u32,
			Mediate::Prefixed(_)
			| Mediate::PrefixedArray(_)
			| Mediate::PrefixedArrayWithLength(_)
			| Mediate::PrefixedTuple(_) => 32,
		}
	}

	fn tail_len(&self) -> u32 {
		match *self {
			Mediate::Raw(_) | Mediate::RawTuple(_) => 0,
			Mediate::Prefixed(ref pre) => pre.len() as u32 * 32,
			Mediate::PrefixedArray(ref mediates) => mediates.iter().fold(0, |acc, m| acc + m.head_len() + m.tail_len()),
			Mediate::PrefixedArrayWithLength(ref mediates) => {
				mediates.iter().fold(32, |acc, m| acc + m.head_len() + m.tail_len())
			}
			Mediate::PrefixedTuple(ref mediates) => mediates.iter().fold(0, |acc, m| acc + m.head_len() + m.tail_len()),
		}
	}

	fn head(&self, suffix_offset: u32) -> Vec<Word> {
		match *self {
			Mediate::Raw(ref raw) => raw.clone(),
			Mediate::RawTuple(ref raw) => raw.iter().map(|mediate| mediate.head(0)).flatten().collect(),
			Mediate::Prefixed(_)
			| Mediate::PrefixedArray(_)
			| Mediate::PrefixedArrayWithLength(_)
			| Mediate::PrefixedTuple(_) => vec![pad_u32(suffix_offset)],
		}
	}

	fn tail(&self) -> Vec<Word> {
		match *self {
			Mediate::Raw(_) | Mediate::RawTuple(_) => vec![],
			Mediate::PrefixedTuple(ref mediates) => encode_head_tail(mediates),
			Mediate::Prefixed(ref raw) => raw.clone(),
			Mediate::PrefixedArray(ref mediates) => encode_head_tail(mediates),
			Mediate::PrefixedArrayWithLength(ref mediates) => {
				// + 32 added to offset represents len of the array prepanded to tail
				let mut result = vec![pad_u32(mediates.len() as u32)];

				let head_tail = encode_head_tail(mediates);

				result.extend(head_tail);
				result
			}
		}
	}
}

fn encode_head_tail(mediates: &Vec<Mediate>) -> Vec<Word> {
	let heads_len = mediates.iter().fold(0, |acc, m| acc + m.head_len());

	let (mut result, len) =
		mediates.iter().fold((Vec::with_capacity(heads_len as usize), heads_len), |(mut acc, offset), m| {
			acc.extend(m.head(offset));
			(acc, offset + m.tail_len())
		});

	let tails = mediates.iter().fold(Vec::with_capacity((len - heads_len) as usize), |mut acc, m| {
		acc.extend(m.tail());
		acc
	});

	result.extend(tails);
	result
}

/// Encodes vector of tokens into ABI compliant vector of bytes.
pub fn encode(tokens: &[Token]) -> Bytes {
	let mediates = &tokens.iter().map(encode_token).collect();

	encode_head_tail(mediates).iter().flat_map(|word| word.to_vec()).collect()
}

fn encode_token(token: &Token) -> Mediate {
	match *token {
		Token::Address(ref address) => {
			let mut padded = [0u8; 32];
			padded[12..].copy_from_slice(address.as_ref());
			Mediate::Raw(vec![padded])
		}
		Token::Bytes(ref bytes) => Mediate::Prefixed(pad_bytes(bytes)),
		Token::String(ref s) => Mediate::Prefixed(pad_bytes(s.as_bytes())),
		Token::FixedBytes(ref bytes) => Mediate::Raw(pad_fixed_bytes(bytes)),
		Token::Int(int) => Mediate::Raw(vec![int.into()]),
		Token::Uint(uint) => Mediate::Raw(vec![uint.into()]),
		Token::Bool(b) => {
			let mut value = [0u8; 32];
			if b {
				value[31] = 1;
			}
			Mediate::Raw(vec![value])
		}
		Token::Array(ref tokens) => {
			let mediates = tokens.iter().map(encode_token).collect();

			Mediate::PrefixedArrayWithLength(mediates)
		}
		Token::FixedArray(ref tokens) => {
			let mediates = tokens.iter().map(encode_token).collect();

			if token.is_dynamic() {
				Mediate::PrefixedArray(mediates)
			} else {
				Mediate::Raw(encode_head_tail(&mediates))
			}
		}
		Token::Tuple(ref tokens) if token.is_dynamic() => {
			let mediates = tokens.iter().map(encode_token).collect();

			Mediate::PrefixedTuple(mediates)
		}
		Token::Tuple(ref tokens) => {
			let mediates = tokens.iter().map(encode_token).collect();

			Mediate::RawTuple(mediates)
		}
	}
}

#[cfg(test)]
mod tests {
	use crate::util::pad_u32;
	use crate::{encode, Token};
	use hex_literal::hex;

	#[test]
	fn encode_address() {
		let address = Token::Address([0x11u8; 20].into());
		let encoded = encode(&vec![address]);
		let expected = hex!("0000000000000000000000001111111111111111111111111111111111111111");
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_dynamic_array_of_addresses() {
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let addresses = Token::Array(vec![address1, address2]);
		let encoded = encode(&vec![addresses]);
		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000002222222222222222222222222222222222222222
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_fixed_array_of_addresses() {
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let addresses = Token::FixedArray(vec![address1, address2]);
		let encoded = encode(&vec![addresses]);
		let expected = hex!(
			"
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000002222222222222222222222222222222222222222
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_two_addresses() {
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let encoded = encode(&vec![address1, address2]);
		let expected = hex!(
			"
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000002222222222222222222222222222222222222222
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_fixed_array_of_dynamic_array_of_addresses() {
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let address3 = Token::Address([0x33u8; 20].into());
		let address4 = Token::Address([0x44u8; 20].into());
		let array0 = Token::Array(vec![address1, address2]);
		let array1 = Token::Array(vec![address3, address4]);
		let fixed = Token::FixedArray(vec![array0, array1]);
		let encoded = encode(&vec![fixed]);
		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000040
			00000000000000000000000000000000000000000000000000000000000000a0
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000002222222222222222222222222222222222222222
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000003333333333333333333333333333333333333333
			0000000000000000000000004444444444444444444444444444444444444444
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_dynamic_array_of_fixed_array_of_addresses() {
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let address3 = Token::Address([0x33u8; 20].into());
		let address4 = Token::Address([0x44u8; 20].into());
		let array0 = Token::FixedArray(vec![address1, address2]);
		let array1 = Token::FixedArray(vec![address3, address4]);
		let dynamic = Token::Array(vec![array0, array1]);
		let encoded = encode(&vec![dynamic]);
		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000002222222222222222222222222222222222222222
			0000000000000000000000003333333333333333333333333333333333333333
			0000000000000000000000004444444444444444444444444444444444444444
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_dynamic_array_of_dynamic_arrays() {
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let array0 = Token::Array(vec![address1]);
		let array1 = Token::Array(vec![address2]);
		let dynamic = Token::Array(vec![array0, array1]);
		let encoded = encode(&vec![dynamic]);
		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000080
			0000000000000000000000000000000000000000000000000000000000000001
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000000000000000000000000000000000000000000001
			0000000000000000000000002222222222222222222222222222222222222222
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_dynamic_array_of_dynamic_arrays2() {
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let address3 = Token::Address([0x33u8; 20].into());
		let address4 = Token::Address([0x44u8; 20].into());
		let array0 = Token::Array(vec![address1, address2]);
		let array1 = Token::Array(vec![address3, address4]);
		let dynamic = Token::Array(vec![array0, array1]);
		let encoded = encode(&vec![dynamic]);
		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000000000000000000000000000000000000000000040
			00000000000000000000000000000000000000000000000000000000000000a0
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000002222222222222222222222222222222222222222
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000003333333333333333333333333333333333333333
			0000000000000000000000004444444444444444444444444444444444444444
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_fixed_array_of_fixed_arrays() {
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let address3 = Token::Address([0x33u8; 20].into());
		let address4 = Token::Address([0x44u8; 20].into());
		let array0 = Token::FixedArray(vec![address1, address2]);
		let array1 = Token::FixedArray(vec![address3, address4]);
		let fixed = Token::FixedArray(vec![array0, array1]);
		let encoded = encode(&vec![fixed]);
		let expected = hex!(
			"
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000002222222222222222222222222222222222222222
			0000000000000000000000003333333333333333333333333333333333333333
			0000000000000000000000004444444444444444444444444444444444444444
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_empty_array() {
		// Empty arrays
		let encoded = encode(&vec![Token::Array(vec![]), Token::Array(vec![])]);
		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000060
			0000000000000000000000000000000000000000000000000000000000000000
			0000000000000000000000000000000000000000000000000000000000000000
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);

		// Nested empty arrays
		let encoded = encode(&vec![Token::Array(vec![Token::Array(vec![])]), Token::Array(vec![Token::Array(vec![])])]);
		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000040
			00000000000000000000000000000000000000000000000000000000000000a0
			0000000000000000000000000000000000000000000000000000000000000001
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000000
			0000000000000000000000000000000000000000000000000000000000000001
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000000
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_bytes() {
		let bytes = Token::Bytes(vec![0x12, 0x34]);
		let encoded = encode(&vec![bytes]);
		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000002
			1234000000000000000000000000000000000000000000000000000000000000
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_fixed_bytes() {
		let bytes = Token::FixedBytes(vec![0x12, 0x34]);
		let encoded = encode(&vec![bytes]);
		let expected = hex!("1234000000000000000000000000000000000000000000000000000000000000");
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_string() {
		let s = Token::String("gavofyork".to_owned());
		let encoded = encode(&vec![s]);
		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000009
			6761766f66796f726b0000000000000000000000000000000000000000000000
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_bytes2() {
		let bytes = Token::Bytes(hex!("10000000000000000000000000000000000000000000000000000000000002").to_vec());
		let encoded = encode(&vec![bytes]);
		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000020
			000000000000000000000000000000000000000000000000000000000000001f
			1000000000000000000000000000000000000000000000000000000000000200
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_bytes3() {
		let bytes = Token::Bytes(
			hex!(
				"
			1000000000000000000000000000000000000000000000000000000000000000
			1000000000000000000000000000000000000000000000000000000000000000
		"
			)
			.to_vec(),
		);
		let encoded = encode(&vec![bytes]);
		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000040
			1000000000000000000000000000000000000000000000000000000000000000
			1000000000000000000000000000000000000000000000000000000000000000
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_two_bytes() {
		let bytes1 = Token::Bytes(hex!("10000000000000000000000000000000000000000000000000000000000002").to_vec());
		let bytes2 = Token::Bytes(hex!("0010000000000000000000000000000000000000000000000000000000000002").to_vec());
		let encoded = encode(&vec![bytes1, bytes2]);
		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000080
			000000000000000000000000000000000000000000000000000000000000001f
			1000000000000000000000000000000000000000000000000000000000000200
			0000000000000000000000000000000000000000000000000000000000000020
			0010000000000000000000000000000000000000000000000000000000000002
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_uint() {
		let mut uint = [0u8; 32];
		uint[31] = 4;
		let encoded = encode(&vec![Token::Uint(uint.into())]);
		let expected = hex!("0000000000000000000000000000000000000000000000000000000000000004");
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_int() {
		let mut int = [0u8; 32];
		int[31] = 4;
		let encoded = encode(&vec![Token::Int(int.into())]);
		let expected = hex!("0000000000000000000000000000000000000000000000000000000000000004");
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_bool() {
		let encoded = encode(&vec![Token::Bool(true)]);
		let expected = hex!("0000000000000000000000000000000000000000000000000000000000000001");
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_bool2() {
		let encoded = encode(&vec![Token::Bool(false)]);
		let expected = hex!("0000000000000000000000000000000000000000000000000000000000000000");
		assert_eq!(encoded, expected);
	}

	#[test]
	fn comprehensive_test() {
		let bytes = hex!(
			"
			131a3afc00d1b1e3461b955e53fc866dcf303b3eb9f4c16f89e388930f48134b
			131a3afc00d1b1e3461b955e53fc866dcf303b3eb9f4c16f89e388930f48134b
		"
		)
		.to_vec();
		let encoded =
			encode(&vec![Token::Int(5.into()), Token::Bytes(bytes.clone()), Token::Int(3.into()), Token::Bytes(bytes)]);

		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000005
			0000000000000000000000000000000000000000000000000000000000000080
			0000000000000000000000000000000000000000000000000000000000000003
			00000000000000000000000000000000000000000000000000000000000000e0
			0000000000000000000000000000000000000000000000000000000000000040
			131a3afc00d1b1e3461b955e53fc866dcf303b3eb9f4c16f89e388930f48134b
			131a3afc00d1b1e3461b955e53fc866dcf303b3eb9f4c16f89e388930f48134b
			0000000000000000000000000000000000000000000000000000000000000040
			131a3afc00d1b1e3461b955e53fc866dcf303b3eb9f4c16f89e388930f48134b
			131a3afc00d1b1e3461b955e53fc866dcf303b3eb9f4c16f89e388930f48134b
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn test_pad_u32() {
		// this will fail if endianess is not supported
		assert_eq!(pad_u32(0x1)[31], 1);
		assert_eq!(pad_u32(0x100)[30], 1);
	}

	#[test]
	fn comprehensive_test2() {
		let encoded = encode(&vec![
			Token::Int(1.into()),
			Token::String("gavofyork".to_owned()),
			Token::Int(2.into()),
			Token::Int(3.into()),
			Token::Int(4.into()),
			Token::Array(vec![Token::Int(5.into()), Token::Int(6.into()), Token::Int(7.into())]),
		]);

		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000001
			00000000000000000000000000000000000000000000000000000000000000c0
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000000000000000000000000000000000000000000003
			0000000000000000000000000000000000000000000000000000000000000004
			0000000000000000000000000000000000000000000000000000000000000100
			0000000000000000000000000000000000000000000000000000000000000009
			6761766f66796f726b0000000000000000000000000000000000000000000000
			0000000000000000000000000000000000000000000000000000000000000003
			0000000000000000000000000000000000000000000000000000000000000005
			0000000000000000000000000000000000000000000000000000000000000006
			0000000000000000000000000000000000000000000000000000000000000007
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_dynamic_array_of_bytes() {
		let bytes = hex!("019c80031b20d5e69c8093a571162299032018d913930d93ab320ae5ea44a4218a274f00d607");
		let encoded = encode(&vec![Token::Array(vec![Token::Bytes(bytes.to_vec())])]);

		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000001
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000026
			019c80031b20d5e69c8093a571162299032018d913930d93ab320ae5ea44a421
			8a274f00d6070000000000000000000000000000000000000000000000000000
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_dynamic_array_of_bytes2() {
		let bytes = hex!("4444444444444444444444444444444444444444444444444444444444444444444444444444");
		let bytes2 = hex!("6666666666666666666666666666666666666666666666666666666666666666666666666666");
		let encoded = encode(&vec![Token::Array(vec![Token::Bytes(bytes.to_vec()), Token::Bytes(bytes2.to_vec())])]);

		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000002
			0000000000000000000000000000000000000000000000000000000000000040
			00000000000000000000000000000000000000000000000000000000000000a0
			0000000000000000000000000000000000000000000000000000000000000026
			4444444444444444444444444444444444444444444444444444444444444444
			4444444444440000000000000000000000000000000000000000000000000000
			0000000000000000000000000000000000000000000000000000000000000026
			6666666666666666666666666666666666666666666666666666666666666666
			6666666666660000000000000000000000000000000000000000000000000000
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_static_tuple_of_addresses() {
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let encoded = encode(&vec![Token::Tuple(vec![address1, address2])]);

		let expected = hex!(
			"
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000002222222222222222222222222222222222222222
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_dynamic_tuple() {
		let string1 = Token::String("gavofyork".to_owned());
		let string2 = Token::String("gavofyork".to_owned());
		let tuple = Token::Tuple(vec![string1, string2]);
		let encoded = encode(&vec![tuple]);
		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000080
			0000000000000000000000000000000000000000000000000000000000000009
			6761766f66796f726b0000000000000000000000000000000000000000000000
			0000000000000000000000000000000000000000000000000000000000000009
			6761766f66796f726b0000000000000000000000000000000000000000000000
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_dynamic_tuple_of_bytes2() {
		let bytes = hex!("4444444444444444444444444444444444444444444444444444444444444444444444444444");
		let bytes2 = hex!("6666666666666666666666666666666666666666666666666666666666666666666666666666");
		let encoded = encode(&vec![Token::Tuple(vec![Token::Bytes(bytes.to_vec()), Token::Bytes(bytes2.to_vec())])]);

		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000040
			00000000000000000000000000000000000000000000000000000000000000a0
			0000000000000000000000000000000000000000000000000000000000000026
			4444444444444444444444444444444444444444444444444444444444444444
			4444444444440000000000000000000000000000000000000000000000000000
			0000000000000000000000000000000000000000000000000000000000000026
			6666666666666666666666666666666666666666666666666666666666666666
			6666666666660000000000000000000000000000000000000000000000000000
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_complex_tuple() {
		let uint = Token::Uint([0x11u8; 32].into());
		let string = Token::String("gavofyork".to_owned());
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let tuple = Token::Tuple(vec![uint, string, address1, address2]);
		let encoded = encode(&vec![tuple]);
		let expected = hex!(
			"
            0000000000000000000000000000000000000000000000000000000000000020
            1111111111111111111111111111111111111111111111111111111111111111
            0000000000000000000000000000000000000000000000000000000000000080
            0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000002222222222222222222222222222222222222222
			0000000000000000000000000000000000000000000000000000000000000009
			6761766f66796f726b0000000000000000000000000000000000000000000000
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_nested_tuple() {
		let string1 = Token::String("test".to_owned());
		let string2 = Token::String("cyborg".to_owned());
		let string3 = Token::String("night".to_owned());
		let string4 = Token::String("day".to_owned());
		let string5 = Token::String("weee".to_owned());
		let string6 = Token::String("funtests".to_owned());
		let bool = Token::Bool(true);
		let deep_tuple = Token::Tuple(vec![string5, string6]);
		let inner_tuple = Token::Tuple(vec![string3, string4, deep_tuple]);
		let outer_tuple = Token::Tuple(vec![string1, bool, string2, inner_tuple]);
		let encoded = encode(&vec![outer_tuple]);
		let expected = hex!(
			"
			0000000000000000000000000000000000000000000000000000000000000020
			0000000000000000000000000000000000000000000000000000000000000080
			0000000000000000000000000000000000000000000000000000000000000001
			00000000000000000000000000000000000000000000000000000000000000c0
			0000000000000000000000000000000000000000000000000000000000000100
			0000000000000000000000000000000000000000000000000000000000000004
			7465737400000000000000000000000000000000000000000000000000000000
			0000000000000000000000000000000000000000000000000000000000000006
			6379626f72670000000000000000000000000000000000000000000000000000
			0000000000000000000000000000000000000000000000000000000000000060
			00000000000000000000000000000000000000000000000000000000000000a0
			00000000000000000000000000000000000000000000000000000000000000e0
			0000000000000000000000000000000000000000000000000000000000000005
			6e69676874000000000000000000000000000000000000000000000000000000
			0000000000000000000000000000000000000000000000000000000000000003
			6461790000000000000000000000000000000000000000000000000000000000
			0000000000000000000000000000000000000000000000000000000000000040
			0000000000000000000000000000000000000000000000000000000000000080
			0000000000000000000000000000000000000000000000000000000000000004
			7765656500000000000000000000000000000000000000000000000000000000
			0000000000000000000000000000000000000000000000000000000000000008
			66756e7465737473000000000000000000000000000000000000000000000000
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_params_containing_dynamic_tuple() {
		let address1 = Token::Address([0x22u8; 20].into());
		let bool1 = Token::Bool(true);
		let string1 = Token::String("spaceship".to_owned());
		let string2 = Token::String("cyborg".to_owned());
		let tuple = Token::Tuple(vec![bool1, string1, string2]);
		let address2 = Token::Address([0x33u8; 20].into());
		let address3 = Token::Address([0x44u8; 20].into());
		let bool2 = Token::Bool(false);
		let encoded = encode(&vec![address1, tuple, address2, address3, bool2]);
		let expected = hex!(
			"
			0000000000000000000000002222222222222222222222222222222222222222
			00000000000000000000000000000000000000000000000000000000000000a0
			0000000000000000000000003333333333333333333333333333333333333333
			0000000000000000000000004444444444444444444444444444444444444444
			0000000000000000000000000000000000000000000000000000000000000000
			0000000000000000000000000000000000000000000000000000000000000001
			0000000000000000000000000000000000000000000000000000000000000060
			00000000000000000000000000000000000000000000000000000000000000a0
			0000000000000000000000000000000000000000000000000000000000000009
			7370616365736869700000000000000000000000000000000000000000000000
			0000000000000000000000000000000000000000000000000000000000000006
			6379626f72670000000000000000000000000000000000000000000000000000
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn encode_params_containing_static_tuple() {
		let address1 = Token::Address([0x11u8; 20].into());
		let address2 = Token::Address([0x22u8; 20].into());
		let bool1 = Token::Bool(true);
		let bool2 = Token::Bool(false);
		let tuple = Token::Tuple(vec![address2, bool1, bool2]);
		let address3 = Token::Address([0x33u8; 20].into());
		let address4 = Token::Address([0x44u8; 20].into());
		let encoded = encode(&vec![address1, tuple, address3, address4]);
		let expected = hex!(
			"
			0000000000000000000000001111111111111111111111111111111111111111
			0000000000000000000000002222222222222222222222222222222222222222
			0000000000000000000000000000000000000000000000000000000000000001
			0000000000000000000000000000000000000000000000000000000000000000
			0000000000000000000000003333333333333333333333333333333333333333
			0000000000000000000000004444444444444444444444444444444444444444
		"
		)
		.to_vec();
		assert_eq!(encoded, expected);
	}
}
