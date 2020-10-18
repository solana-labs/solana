// Copyright 2015-2020 Parity Technologies
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Contract event.

use serde::Deserialize;
use std::collections::HashMap;
use tiny_keccak::keccak256;

use crate::ethabi::{
	decode, encode, signature::long_signature, Error, EventParam, Hash, Log, LogParam, ParamType, RawLog,
	RawTopicFilter, Result, Token, Topic, TopicFilter,
};

/// Contract event.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Event {
	/// Event name.
	pub name: String,
	/// Event input.
	pub inputs: Vec<EventParam>,
	/// If anonymous, event cannot be found using `from` filter.
	pub anonymous: bool,
}

impl Event {
	/// Returns names of all params.
	fn params_names(&self) -> Vec<String> {
		self.inputs.iter().map(|p| p.name.clone()).collect()
	}

	/// Returns types of all params.
	fn param_types(&self) -> Vec<ParamType> {
		self.inputs.iter().map(|p| p.kind.clone()).collect()
	}

	/// Returns all params of the event.
	fn indexed_params(&self, indexed: bool) -> Vec<EventParam> {
		self.inputs.iter().filter(|p| p.indexed == indexed).cloned().collect()
	}

	/// Event signature
	pub fn signature(&self) -> Hash {
		long_signature(&self.name, &self.param_types())
	}

	/// Creates topic filter
	pub fn filter(&self, raw: RawTopicFilter) -> Result<TopicFilter> {
		fn convert_token(token: Token, kind: &ParamType) -> Result<Hash> {
			if !token.type_check(kind) {
				return Err(Error::InvalidData);
			}
			let encoded = encode(&[token]);
			if encoded.len() == 32 {
				let mut data = [0u8; 32];
				data.copy_from_slice(&encoded);
				Ok(data.into())
			} else {
				Ok(keccak256(&encoded).into())
			}
		}

		fn convert_topic(topic: Topic<Token>, kind: Option<&ParamType>) -> Result<Topic<Hash>> {
			match topic {
				Topic::Any => Ok(Topic::Any),
				Topic::OneOf(tokens) => match kind {
					None => Err(Error::InvalidData),
					Some(kind) => {
						let topics =
							tokens.into_iter().map(|token| convert_token(token, kind)).collect::<Result<Vec<_>>>()?;
						Ok(Topic::OneOf(topics))
					}
				},
				Topic::This(token) => match kind {
					None => Err(Error::InvalidData),
					Some(kind) => Ok(Topic::This(convert_token(token, kind)?)),
				},
			}
		}

		let kinds: Vec<_> = self.indexed_params(true).into_iter().map(|param| param.kind).collect();
		let result = if self.anonymous {
			TopicFilter {
				topic0: convert_topic(raw.topic0, kinds.get(0))?,
				topic1: convert_topic(raw.topic1, kinds.get(1))?,
				topic2: convert_topic(raw.topic2, kinds.get(2))?,
				topic3: Topic::Any,
			}
		} else {
			TopicFilter {
				topic0: Topic::This(self.signature()),
				topic1: convert_topic(raw.topic0, kinds.get(0))?,
				topic2: convert_topic(raw.topic1, kinds.get(1))?,
				topic3: convert_topic(raw.topic2, kinds.get(2))?,
			}
		};

		Ok(result)
	}

	// Converts param types for indexed parameters to bytes32 where appropriate
	// This applies to strings, arrays, structs and bytes to follow the encoding of
	// these indexed param types according to
	// https://solidity.readthedocs.io/en/develop/abi-spec.html#encoding-of-indexed-event-parameters
	fn convert_topic_param_type(&self, kind: &ParamType) -> ParamType {
		match kind {
			ParamType::String
			| ParamType::Bytes
			| ParamType::Array(_)
			| ParamType::FixedArray(_, _)
			| ParamType::Tuple(_) => ParamType::FixedBytes(32),
			_ => kind.clone(),
		}
	}

	/// Parses `RawLog` and retrieves all log params from it.
	pub fn parse_log(&self, log: RawLog) -> Result<Log> {
		let topics = log.topics;
		let data = log.data;
		let topics_len = topics.len();
		// obtains all params info
		let topic_params = self.indexed_params(true);
		let data_params = self.indexed_params(false);
		// then take first topic if event is not anonymous
		let to_skip = if self.anonymous {
			0
		} else {
			// verify
			let event_signature = topics.get(0).ok_or(Error::InvalidData)?;
			if event_signature != &self.signature() {
				return Err(Error::InvalidData.into());
			}
			1
		};

		let topic_types =
			topic_params.iter().map(|p| self.convert_topic_param_type(&p.kind)).collect::<Vec<ParamType>>();

		let flat_topics = topics.into_iter().skip(to_skip).flat_map(|t| t.as_ref().to_vec()).collect::<Vec<u8>>();

		let topic_tokens = decode(&topic_types, &flat_topics)?;

		// topic may be only a 32 bytes encoded token
		if topic_tokens.len() != topics_len - to_skip {
			return Err(Error::InvalidData);
		}

		let topics_named_tokens = topic_params.into_iter().map(|p| p.name).zip(topic_tokens.into_iter());

		let data_types = data_params.iter().map(|p| p.kind.clone()).collect::<Vec<ParamType>>();

		let data_tokens = decode(&data_types, &data)?;

		let data_named_tokens = data_params.into_iter().map(|p| p.name).zip(data_tokens.into_iter());

		let named_tokens = topics_named_tokens.chain(data_named_tokens).collect::<HashMap<String, Token>>();

		let decoded_params = self
			.params_names()
			.into_iter()
			.map(|name| LogParam { name: name.clone(), value: named_tokens[&name].clone() })
			.collect();

		let result = Log { params: decoded_params };

		Ok(result)
	}
}

#[cfg(test)]
mod tests {
	use crate::{
		log::{Log, RawLog},
		signature::long_signature,
		token::Token,
		Event, EventParam, LogParam, ParamType,
	};
	use hex::FromHex;

	#[test]
	fn test_decoding_event() {
		let event = Event {
			name: "foo".to_owned(),
			inputs: vec![
				EventParam { name: "a".to_owned(), kind: ParamType::Int(256), indexed: false },
				EventParam { name: "b".to_owned(), kind: ParamType::Int(256), indexed: true },
				EventParam { name: "c".to_owned(), kind: ParamType::Address, indexed: false },
				EventParam { name: "d".to_owned(), kind: ParamType::Address, indexed: true },
				EventParam { name: "e".to_owned(), kind: ParamType::String, indexed: true },
				EventParam {
					name: "f".to_owned(),
					kind: ParamType::Array(Box::new(ParamType::Int(256))),
					indexed: true,
				},
				EventParam {
					name: "g".to_owned(),
					kind: ParamType::FixedArray(Box::new(ParamType::Address), 5),
					indexed: true,
				},
			],
			anonymous: false,
		};

		let log = RawLog {
			topics: vec![
				long_signature(
					"foo",
					&[
						ParamType::Int(256),
						ParamType::Int(256),
						ParamType::Address,
						ParamType::Address,
						ParamType::String,
						ParamType::Array(Box::new(ParamType::Int(256))),
						ParamType::FixedArray(Box::new(ParamType::Address), 5),
					],
				),
				"0000000000000000000000000000000000000000000000000000000000000002".parse().unwrap(),
				"0000000000000000000000001111111111111111111111111111111111111111".parse().unwrap(),
				"00000000000000000aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".parse().unwrap(),
				"00000000000000000bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".parse().unwrap(),
				"00000000000000000ccccccccccccccccccccccccccccccccccccccccccccccc".parse().unwrap(),
			],
			data: ("".to_owned()
				+ "0000000000000000000000000000000000000000000000000000000000000003"
				+ "0000000000000000000000002222222222222222222222222222222222222222")
				.from_hex()
				.unwrap(),
		};
		let result = event.parse_log(log).unwrap();

		assert_eq!(
			result,
			Log {
				params: vec![
					(
						"a".to_owned(),
						Token::Int("0000000000000000000000000000000000000000000000000000000000000003".into())
					),
					(
						"b".to_owned(),
						Token::Int("0000000000000000000000000000000000000000000000000000000000000002".into())
					),
					("c".to_owned(), Token::Address("2222222222222222222222222222222222222222".parse().unwrap())),
					("d".to_owned(), Token::Address("1111111111111111111111111111111111111111".parse().unwrap())),
					(
						"e".to_owned(),
						Token::FixedBytes(
							"00000000000000000aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".from_hex().unwrap()
						)
					),
					(
						"f".to_owned(),
						Token::FixedBytes(
							"00000000000000000bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".from_hex().unwrap()
						)
					),
					(
						"g".to_owned(),
						Token::FixedBytes(
							"00000000000000000ccccccccccccccccccccccccccccccccccccccccccccccc".from_hex().unwrap()
						)
					),
				]
				.into_iter()
				.map(|(name, value)| LogParam { name, value })
				.collect::<Vec<_>>()
			}
		);
	}
}
