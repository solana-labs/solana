use crate::instruction::InstructionError;

/// Deserialize with a limit based the maximum amount of data a program can expect to get.
/// This function should be used in place of direct deserialization to help prevent OOM errors
pub fn limited_deserialize<T>(instruction_data: &[u8]) -> Result<T, InstructionError>
where
    T: serde::de::DeserializeOwned,
{
    let limit = crate::packet::PACKET_DATA_SIZE as u64;
    bincode::config()
        .limit(limit)
        .deserialize(instruction_data)
        .map_err(|_| InstructionError::InvalidInstructionData)
}
