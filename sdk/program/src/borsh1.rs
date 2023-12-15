#![allow(clippy::arithmetic_side_effects)]
//! Utilities for the [borsh] serialization format, version 1.
//!
//! [borsh]: https://borsh.io/
use {
    crate::borsh::{
        impl_get_instance_packed_len, impl_get_packed_len_v1, impl_try_from_slice_unchecked,
    },
    borsh::io,
};

impl_get_packed_len_v1!(borsh);
impl_try_from_slice_unchecked!(borsh, io);
impl_get_instance_packed_len!(borsh, io);

#[cfg(test)]
mod tests {
    use {crate::borsh::impl_tests, borsh::io};
    impl_tests!(borsh, io);
}
