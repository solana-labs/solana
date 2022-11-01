//! Converting custom error codes to enums.

use num_traits::FromPrimitive;

/// Allows custom errors to be decoded back to their original enum.
///
/// Some Solana error enums, like [`ProgramError`], include a `Custom` variant,
/// like [`ProgramError::Custom`], that contains a `u32` error code. This code
/// may represent any error that is not covered by the error enum's named
/// variants. It is common for programs to convert their own error enums to an
/// error code and store it in the `Custom` variant, possibly with the help of
/// the [`ToPrimitive`] trait.
///
/// This trait builds on the [`FromPrimitive`] trait to help convert those error
/// codes to the original error enum they represent.
///
/// As this allows freely converting `u32` to any type that implements
/// `FromPrimitive`, it is only used correctly when the caller is certain of the
/// original error type.
///
/// [`ProgramError`]: crate::program_error::ProgramError
/// [`ProgramError::Custom`]: crate::program_error::ProgramError::Custom
/// [`ToPrimitive`]: num_traits::ToPrimitive
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
    use {super::*, num_derive::FromPrimitive};

    #[test]
    fn test_decode_custom_error_to_enum() {
        #[derive(Debug, FromPrimitive, PartialEq, Eq)]
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
