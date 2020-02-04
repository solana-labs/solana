use crate::{
    account::KeyedAccount, account_info::AccountInfo, instruction::InstructionError,
    program_error::ProgramError,
};
use num_traits::FromPrimitive;

/// Return the next KeyedAccount or a NotEnoughAccountKeys error
pub fn next_keyed_account<'a, 'b, I: Iterator<Item = &'a KeyedAccount<'b>>>(
    iter: &mut I,
) -> Result<I::Item, InstructionError> {
    iter.next().ok_or(InstructionError::NotEnoughAccountKeys)
}

/// Return the next AccountInfo or a NotEnoughAccountKeys error
pub fn next_account_info<'a, 'b, I: Iterator<Item = &'a AccountInfo<'b>>>(
    iter: &mut I,
) -> Result<I::Item, ProgramError> {
    iter.next().ok_or(ProgramError::NotEnoughAccountKeys)
}

/// Return true if the first keyed_account is executable, used to determine if
/// the loader should call a program's 'main'
pub fn is_executable(keyed_accounts: &[KeyedAccount]) -> Result<bool, InstructionError> {
    Ok(!keyed_accounts.is_empty() && keyed_accounts[0].executable()?)
}

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

/// Allows customer errors to be decoded back to their original enum
pub trait DecodeError<E> {
    fn decode_custom_error_to_enum(custom: u32) -> Option<E>
    where
        E: FromPrimitive,
    {
        E::from_u32(custom)
    }
    fn type_of() -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_derive::FromPrimitive;

    #[test]
    fn test_decode_custom_error_to_enum() {
        #[derive(Debug, FromPrimitive, PartialEq)]
        enum TestEnum {
            A,
            B,
            C,
        }
        impl<T> DecodeError<T> for TestEnum {
            fn type_of() -> &'static str {
                "TestEnum"
            }
        }
        assert_eq!(TestEnum::decode_custom_error_to_enum(0), Some(TestEnum::A));
        assert_eq!(TestEnum::decode_custom_error_to_enum(1), Some(TestEnum::B));
        assert_eq!(TestEnum::decode_custom_error_to_enum(2), Some(TestEnum::C));
        let option: Option<TestEnum> = TestEnum::decode_custom_error_to_enum(3);
        assert_eq!(option, None);
    }
}
