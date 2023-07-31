#![allow(clippy::integer_arithmetic)]
//! Utilities for the [borsh] serialization format, version 0.9.
//!
//! This file is provided for backwards compatibility with types that still use
//! borsh 0.9, even though this crate canonically uses borsh 0.10.
//!
//! [borsh]: https://borsh.io/
use crate::borsh::{
    impl_get_instance_packed_len, impl_get_packed_len, impl_try_from_slice_unchecked,
};

impl_get_packed_len!(
    borsh0_9,
    #[deprecated(
        since = "1.17.0",
        note = "Please upgrade to Borsh 0.10 and use `borsh0_10::get_packed_len` instead"
    )]
);
impl_try_from_slice_unchecked!(
    borsh0_9,
    #[deprecated(
        since = "1.17.0",
        note = "Please upgrade to Borsh 0.10 and use `borsh0_10::try_from_slice_unchecked` instead"
    )]
);
impl_get_instance_packed_len!(
    borsh0_9,
    #[deprecated(
        since = "1.17.0",
        note = "Please upgrade to Borsh 0.10 and use `borsh0_10::get_instance_packed_len` instead"
    )]
);

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use {
        super::*,
        borsh0_9::{maybestd::io::ErrorKind, BorshDeserialize, BorshSerialize},
        std::{collections::HashMap, mem::size_of},
    };

    type Child = [u8; 64];
    type Parent = Vec<Child>;

    #[test]
    fn unchecked_deserialization() {
        let parent = vec![[0u8; 64], [1u8; 64], [2u8; 64]];

        // exact size, both work
        let mut byte_vec = vec![0u8; 4 + get_packed_len::<Child>() * 3];
        let mut bytes = byte_vec.as_mut_slice();
        parent.serialize(&mut bytes).unwrap();
        let deserialized = Parent::try_from_slice(&byte_vec).unwrap();
        assert_eq!(deserialized, parent);
        let deserialized = try_from_slice_unchecked::<Parent>(&byte_vec).unwrap();
        assert_eq!(deserialized, parent);

        // too big, only unchecked works
        let mut byte_vec = vec![0u8; 4 + get_packed_len::<Child>() * 10];
        let mut bytes = byte_vec.as_mut_slice();
        parent.serialize(&mut bytes).unwrap();
        let err = Parent::try_from_slice(&byte_vec).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidData);
        let deserialized = try_from_slice_unchecked::<Parent>(&byte_vec).unwrap();
        assert_eq!(deserialized, parent);
    }

    #[test]
    fn packed_len() {
        assert_eq!(get_packed_len::<u64>(), size_of::<u64>());
        assert_eq!(get_packed_len::<Child>(), size_of::<u8>() * 64);
    }

    #[test]
    fn instance_packed_len_matches_packed_len() {
        let child = [0u8; 64];
        assert_eq!(
            get_packed_len::<Child>(),
            get_instance_packed_len(&child).unwrap(),
        );
        assert_eq!(
            get_packed_len::<u8>(),
            get_instance_packed_len(&0u8).unwrap(),
        );
        assert_eq!(
            get_packed_len::<u16>(),
            get_instance_packed_len(&0u16).unwrap(),
        );
        assert_eq!(
            get_packed_len::<u32>(),
            get_instance_packed_len(&0u32).unwrap(),
        );
        assert_eq!(
            get_packed_len::<u64>(),
            get_instance_packed_len(&0u64).unwrap(),
        );
        assert_eq!(
            get_packed_len::<u128>(),
            get_instance_packed_len(&0u128).unwrap(),
        );
        assert_eq!(
            get_packed_len::<[u8; 10]>(),
            get_instance_packed_len(&[0u8; 10]).unwrap(),
        );
        assert_eq!(
            get_packed_len::<(i8, i16, i32, i64, i128)>(),
            get_instance_packed_len(&(i8::MAX, i16::MAX, i32::MAX, i64::MAX, i128::MAX)).unwrap(),
        );
    }

    #[test]
    fn instance_packed_len_with_vec() {
        let parent = vec![
            [0u8; 64], [1u8; 64], [2u8; 64], [3u8; 64], [4u8; 64], [5u8; 64],
        ];
        assert_eq!(
            get_instance_packed_len(&parent).unwrap(),
            4 + parent.len() * get_packed_len::<Child>()
        );
    }

    #[test]
    fn instance_packed_len_with_varying_sizes_in_hashmap() {
        let mut data = HashMap::new();
        let key1 = "the first string, it's actually really really long".to_string();
        let value1 = "".to_string();
        let key2 = "second string, shorter".to_string();
        let value2 = "a real value".to_string();
        let key3 = "third".to_string();
        let value3 = "an even longer value".to_string();
        data.insert(key1.clone(), value1.clone());
        data.insert(key2.clone(), value2.clone());
        data.insert(key3.clone(), value3.clone());
        assert_eq!(
            get_instance_packed_len(&data).unwrap(),
            4 + get_instance_packed_len(&key1).unwrap()
                + get_instance_packed_len(&value1).unwrap()
                + get_instance_packed_len(&key2).unwrap()
                + get_instance_packed_len(&value2).unwrap()
                + get_instance_packed_len(&key3).unwrap()
                + get_instance_packed_len(&value3).unwrap()
        );
    }
}
