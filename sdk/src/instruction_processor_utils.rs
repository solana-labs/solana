use crate::{account::KeyedAccount, instruction::InstructionError, pubkey::Pubkey};
use num_traits::FromPrimitive;

// Prototype of a native program entry point
pub type Entrypoint = unsafe extern "C" fn(
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    instruction_data: &[u8],
) -> Result<(), InstructionError>;

/// Convenience macro to declare a native program
///
/// bs58_string: bs58 string representation the program's id
/// name: Name of the program, must match the library name in Cargo.toml
/// entrypoint: Program's entrypoint, must be of `type Entrypoint`
///
/// # Examples
///
/// ```
/// use std::str::FromStr;
/// # // wrapper is used so that the macro invocation occurs in the item position
/// # // rather than in the statement position which isn't allowed.
/// # mod item_wrapper {
/// use solana_sdk::account::KeyedAccount;
/// use solana_sdk::instruction::InstructionError;
/// use solana_sdk::pubkey::Pubkey;
/// use solana_sdk::declare_program;
///
/// fn my_process_instruction(
///     program_id: &Pubkey,
///     keyed_accounts: &[KeyedAccount],
///     instruction_data: &[u8],
/// ) -> Result<(), InstructionError> {
///   // Process an instruction
///   Ok(())
/// }
///
/// declare_program!(
///     "My11111111111111111111111111111111111111111",
///     solana_my_program,
///     my_process_instruction
/// );
///
/// # }
/// # use solana_sdk::pubkey::Pubkey;
/// # use item_wrapper::id;
/// let my_id = Pubkey::from_str("My11111111111111111111111111111111111111111").unwrap();
/// assert_eq!(id(), my_id);
/// ```
/// ```
/// use std::str::FromStr;
/// # // wrapper is used so that the macro invocation occurs in the item position
/// # // rather than in the statement position which isn't allowed.
/// # mod item_wrapper {
/// use solana_sdk::account::KeyedAccount;
/// use solana_sdk::instruction::InstructionError;
/// use solana_sdk::pubkey::Pubkey;
/// use solana_sdk::declare_program;
///
/// fn my_process_instruction(
///     program_id: &Pubkey,
///     keyed_accounts: &[KeyedAccount],
///     instruction_data: &[u8],
/// ) -> Result<(), InstructionError> {
///   // Process an instruction
///   Ok(())
/// }
///
/// declare_program!(
///     solana_sdk::system_program::ID,
///     solana_my_program,
///     my_process_instruction
/// );
/// # }
///
/// # use item_wrapper::id;
/// assert_eq!(id(), solana_sdk::system_program::ID);
/// ```
#[macro_export]
macro_rules! declare_program(
    ($bs58_string:expr, $name:ident, $entrypoint:expr) => (
        $crate::declare_id!($bs58_string);

        #[macro_export]
        macro_rules! $name {
            () => {
                (stringify!($name).to_string(), $crate::id())
            };
        }

        #[no_mangle]
        pub extern "C" fn $name(
            program_id: &$crate::pubkey::Pubkey,
            keyed_accounts: &[$crate::account::KeyedAccount],
            instruction_data: &[u8],
        ) -> Result<(), $crate::instruction::InstructionError> {
            $entrypoint(program_id, keyed_accounts, instruction_data)
        }
    )
);

/// Return the next KeyedAccount or a NotEnoughAccountKeys instruction error
pub fn next_keyed_account<I: Iterator>(iter: &mut I) -> Result<I::Item, InstructionError> {
    iter.next().ok_or(InstructionError::NotEnoughAccountKeys)
}

/// Return true if the first keyed_account is executable, used to determine if
/// the loader should call a program's 'main'
pub fn is_executable(keyed_accounts: &[KeyedAccount]) -> Result<bool, InstructionError> {
    Ok(!keyed_accounts.is_empty() && keyed_accounts[0].executable()?)
}

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
