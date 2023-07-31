//! Utilities for the [borsh] serialization format.
//!
//! To avoid backwards-incompatibilities when the Solana SDK changes its dependency
//! on borsh, it's recommended to instead use the version-specific file directly,
//! ie. `borsh0_10`.
//!
//! This file remains for developers who use these borsh helpers, but it will
//! be removed in a future release
//!
//! [borsh]: https://borsh.io/
use borsh::{maybestd::io::Error, BorshDeserialize, BorshSchema, BorshSerialize};

/// Get the worst-case packed length for the given BorshSchema
///
/// Note: due to the serializer currently used by Borsh, this function cannot
/// be used on-chain in the Solana SBF execution environment.
#[deprecated(
    since = "1.17.0",
    note = "Please use `borsh0_10::get_packed_len` instead"
)]
pub fn get_packed_len<S: BorshSchema>() -> usize {
    crate::borsh0_10::get_packed_len::<S>()
}

/// Deserializes without checking that the entire slice has been consumed
///
/// Normally, `try_from_slice` checks the length of the final slice to ensure
/// that the deserialization uses up all of the bytes in the slice.
///
/// Note that there is a potential issue with this function. Any buffer greater than
/// or equal to the expected size will properly deserialize. For example, if the
/// user passes a buffer destined for a different type, the error won't get caught
/// as easily.
#[deprecated(
    since = "1.17.0",
    note = "Please use `borsh0_10::try_from_slice_unchecked` instead"
)]
pub fn try_from_slice_unchecked<T: BorshDeserialize>(data: &[u8]) -> Result<T, Error> {
    crate::borsh0_10::try_from_slice_unchecked::<T>(data)
}

/// Get the packed length for the serialized form of this object instance.
///
/// Useful when working with instances of types that contain a variable-length
/// sequence, such as a Vec or HashMap.  Since it is impossible to know the packed
/// length only from the type's schema, this can be used when an instance already
/// exists, to figure out how much space to allocate in an account.
#[deprecated(
    since = "1.17.0",
    note = "Please use `borsh0_10::get_instance_packed_len` instead"
)]
pub fn get_instance_packed_len<T: BorshSerialize>(instance: &T) -> Result<usize, Error> {
    crate::borsh0_10::get_instance_packed_len(instance)
}

macro_rules! impl_get_packed_len {
    ($borsh:ident $(,#[$meta:meta])?) => {
        /// Get the worst-case packed length for the given BorshSchema
        ///
        /// Note: due to the serializer currently used by Borsh, this function cannot
        /// be used on-chain in the Solana SBF execution environment.
        $(#[$meta])?
        pub fn get_packed_len<S: $borsh::BorshSchema>() -> usize {
            let $borsh::schema::BorshSchemaContainer { declaration, definitions } =
                &S::schema_container();
            get_declaration_packed_len(declaration, definitions)
        }

        /// Get packed length for the given BorshSchema Declaration
        fn get_declaration_packed_len(
            declaration: &str,
            definitions: &std::collections::HashMap<$borsh::schema::Declaration, $borsh::schema::Definition>,
        ) -> usize {
            match definitions.get(declaration) {
                Some($borsh::schema::Definition::Array { length, elements }) => {
                    *length as usize * get_declaration_packed_len(elements, definitions)
                }
                Some($borsh::schema::Definition::Enum { variants }) => {
                    1 + variants
                        .iter()
                        .map(|(_, declaration)| get_declaration_packed_len(declaration, definitions))
                        .max()
                        .unwrap_or(0)
                }
                Some($borsh::schema::Definition::Struct { fields }) => match fields {
                    $borsh::schema::Fields::NamedFields(named_fields) => named_fields
                        .iter()
                        .map(|(_, declaration)| get_declaration_packed_len(declaration, definitions))
                        .sum(),
                    $borsh::schema::Fields::UnnamedFields(declarations) => declarations
                        .iter()
                        .map(|declaration| get_declaration_packed_len(declaration, definitions))
                        .sum(),
                    $borsh::schema::Fields::Empty => 0,
                },
                Some($borsh::schema::Definition::Sequence {
                    elements: _elements,
                }) => panic!("Missing support for Definition::Sequence"),
                Some($borsh::schema::Definition::Tuple { elements }) => elements
                    .iter()
                    .map(|element| get_declaration_packed_len(element, definitions))
                    .sum(),
                None => match declaration {
                    "bool" | "u8" | "i8" => 1,
                    "u16" | "i16" => 2,
                    "u32" | "i32" => 4,
                    "u64" | "i64" => 8,
                    "u128" | "i128" => 16,
                    "nil" => 0,
                    _ => panic!("Missing primitive type: {declaration}"),
                },
            }
        }
    }
}
pub(crate) use impl_get_packed_len;

macro_rules! impl_try_from_slice_unchecked {
    ($borsh:ident $(,#[$meta:meta])?) => {
        /// Deserializes without checking that the entire slice has been consumed
        ///
        /// Normally, `try_from_slice` checks the length of the final slice to ensure
        /// that the deserialization uses up all of the bytes in the slice.
        ///
        /// Note that there is a potential issue with this function. Any buffer greater than
        /// or equal to the expected size will properly deserialize. For example, if the
        /// user passes a buffer destined for a different type, the error won't get caught
        /// as easily.
        $(#[$meta])?
        pub fn try_from_slice_unchecked<T: $borsh::BorshDeserialize>(data: &[u8]) -> Result<T, $borsh::maybestd::io::Error> {
            let mut data_mut = data;
            let result = T::deserialize(&mut data_mut)?;
            Ok(result)
        }
    }
}
pub(crate) use impl_try_from_slice_unchecked;

macro_rules! impl_get_instance_packed_len {
    ($borsh:ident $(,#[$meta:meta])?) => {
        /// Helper struct which to count how much data would be written during serialization
        #[derive(Default)]
        struct WriteCounter {
            count: usize,
        }

        impl $borsh::maybestd::io::Write for WriteCounter {
            fn write(&mut self, data: &[u8]) -> Result<usize, $borsh::maybestd::io::Error> {
                let amount = data.len();
                self.count += amount;
                Ok(amount)
            }

            fn flush(&mut self) -> Result<(), $borsh::maybestd::io::Error> {
                Ok(())
            }
        }

        /// Get the packed length for the serialized form of this object instance.
        ///
        /// Useful when working with instances of types that contain a variable-length
        /// sequence, such as a Vec or HashMap.  Since it is impossible to know the packed
        /// length only from the type's schema, this can be used when an instance already
        /// exists, to figure out how much space to allocate in an account.
        $(#[$meta])?
        pub fn get_instance_packed_len<T: $borsh::BorshSerialize>(instance: &T) -> Result<usize, $borsh::maybestd::io::Error> {
            let mut counter = WriteCounter::default();
            instance.serialize(&mut counter)?;
            Ok(counter.count)
        }
    }
}
pub(crate) use impl_get_instance_packed_len;
