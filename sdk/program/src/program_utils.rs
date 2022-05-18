//! Contains a single utility function for deserializing from [bincode].
//!
//! [bincode]: https://docs.rs/bincode

use {crate::instruction::InstructionError, bincode::config::Options};

/// Deserialize with a limit based the maximum amount of data a program can expect to get.
/// This function should be used in place of direct deserialization to help prevent OOM errors
pub fn limited_deserialize<T>(instruction_data: &[u8], limit: u64) -> Result<T, InstructionError>
where
    T: serde::de::DeserializeOwned,
{
    bincode::options()
        .with_limit(limit)
        .with_fixint_encoding() // As per https://github.com/servo/bincode/issues/333, these two options are needed
        .allow_trailing_bytes() // to retain the behavior of bincode::deserialize with the new `options()` method
        .deserialize_from(instruction_data)
        .map_err(|_| InstructionError::InvalidInstructionData)
}

#[cfg(test)]
pub mod tests {
    use {super::*, solana_program::system_instruction::SystemInstruction};

    #[test]
    fn test_limited_deserialize_advance_nonce_account() {
        let item = SystemInstruction::AdvanceNonceAccount;
        let serialized = bincode::serialize(&item).unwrap();

        assert_eq!(
            serialized.len(),
            4,
            "`SanitizedMessage::get_durable_nonce()` may need a change"
        );

        assert_eq!(
            limited_deserialize::<SystemInstruction>(&serialized, 4).as_ref(),
            Ok(&item)
        );
        assert!(limited_deserialize::<SystemInstruction>(&serialized, 3).is_err());
    }
}
