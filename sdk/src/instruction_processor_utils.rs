use crate::account::KeyedAccount;
use crate::instruction::InstructionError;
use crate::pubkey::Pubkey;
use num_traits::FromPrimitive;

// All native programs export a symbol named process()
pub const ENTRYPOINT: &str = "process";

// Native program ENTRYPOINT prototype
pub type Entrypoint = unsafe extern "C" fn(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError>;

// Convenience macro to define the native program entrypoint.  Supply a fn to this macro that
// conforms to the `Entrypoint` type signature.
#[macro_export]
macro_rules! solana_entrypoint(
    ($entrypoint:ident) => (
        #[no_mangle]
        pub extern "C" fn process(
            program_id: &solana_sdk::pubkey::Pubkey,
            keyed_accounts: &mut [solana_sdk::account::KeyedAccount],
            data: &[u8],
        ) -> Result<(), solana_sdk::instruction::InstructionError> {
            $entrypoint(program_id, keyed_accounts, data)
        }
    )
);

#[macro_export]
macro_rules! solana_program_id(
    ($program_id:ident) => (

        pub fn check_id(program_id: &solana_sdk::pubkey::Pubkey) -> bool {
            program_id.as_ref() == $program_id
        }

        pub fn id() -> solana_sdk::pubkey::Pubkey {
            solana_sdk::pubkey::Pubkey::new(&$program_id)
        }

        #[cfg(test)]
        #[test]
        fn test_program_id() {
            assert!(check_id(&id()));
        }
    )
);

pub trait DecodeError<E> {
    fn decode_custom_error_to_enum(int: u32) -> Option<E>
    where
        E: FromPrimitive,
    {
        E::from_u32(int)
    }
    fn type_of(&self) -> &'static str;
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
            fn type_of(&self) -> &'static str {
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
