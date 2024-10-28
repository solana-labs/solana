//! Utilities for the [borsh] serialization format.
//!
//! To avoid backwards-incompatibilities when the Solana SDK changes its dependency
//! on borsh, it's recommended to instead use the version-specific file directly,
//! ie. `v0_10`.
//!
//! This file remains for developers who use these borsh helpers, but it will
//! be removed in a future release
//!
//! [borsh]: https://borsh.io/
use borsh0_10::{maybestd::io::Error, BorshDeserialize, BorshSchema, BorshSerialize};

/// Get the worst-case packed length for the given BorshSchema
///
/// Note: due to the serializer currently used by Borsh, this function cannot
/// be used on-chain in the Solana SBF execution environment.
#[deprecated(since = "1.17.0", note = "Please use `v0_10::get_packed_len` instead")]
pub fn get_packed_len<S: BorshSchema>() -> usize {
    #[allow(deprecated)]
    crate::v0_10::get_packed_len::<S>()
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
    note = "Please use `v0_10::try_from_slice_unchecked` instead"
)]
pub fn try_from_slice_unchecked<T: BorshDeserialize>(data: &[u8]) -> Result<T, Error> {
    #[allow(deprecated)]
    crate::v0_10::try_from_slice_unchecked::<T>(data)
}

/// Get the packed length for the serialized form of this object instance.
///
/// Useful when working with instances of types that contain a variable-length
/// sequence, such as a Vec or HashMap.  Since it is impossible to know the packed
/// length only from the type's schema, this can be used when an instance already
/// exists, to figure out how much space to allocate in an account.
#[deprecated(
    since = "1.17.0",
    note = "Please use `v0_10::get_instance_packed_len` instead"
)]
pub fn get_instance_packed_len<T: BorshSerialize>(instance: &T) -> Result<usize, Error> {
    #[allow(deprecated)]
    crate::v0_10::get_instance_packed_len(instance)
}
