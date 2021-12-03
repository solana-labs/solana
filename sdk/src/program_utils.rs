use {crate::instruction::InstructionError, bincode::config::Options};

/// Deserialize with a limit based the maximum amount of data a program can expect to get.
/// This function should be used in place of direct deserialization to help prevent OOM errors
pub fn limited_deserialize<T>(instruction_data: &[u8]) -> Result<T, InstructionError>
where
    T: serde::de::DeserializeOwned,
{
    let limit = crate::packet::PACKET_DATA_SIZE as u64;
    bincode::options()
        .with_limit(limit)
        .with_fixint_encoding() // As per https://github.com/servo/bincode/issues/333, these two options are needed
        .allow_trailing_bytes() // to retain the behavior of bincode::deserialize with the new `options()` method
        .deserialize_from(instruction_data)
        .map_err(|_| InstructionError::InvalidInstructionData)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_limited_deserialize() {
        #[derive(Deserialize, Serialize)]
        enum Foo {
            Bar(Vec<u8>),
        }

        let item = Foo::Bar([1; crate::packet::PACKET_DATA_SIZE - 12].to_vec()); // crate::packet::PACKET_DATA_SIZE - 12: size limit, minus enum variant and vec len() serialized sizes
        let serialized = bincode::serialize(&item).unwrap();
        assert!(limited_deserialize::<Foo>(&serialized).is_ok());

        let item = Foo::Bar([1; crate::packet::PACKET_DATA_SIZE - 11].to_vec()); // Extra byte should bump serialized size over the size limit
        let serialized = bincode::serialize(&item).unwrap();
        assert!(limited_deserialize::<Foo>(&serialized).is_err());
    }
}
