use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::ser::{SerializeTuple, Serializer};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::marker::PhantomData;
use std::mem::size_of;

/// Same as usize, but serialized with 1 to 9 bytes. If the value is above
/// 0x7f, the top bit is set and the remaining value is stored in the next
/// bytes. Each byte follows the same pattern until the 9th byte. The 9th
/// byte, if needed, uses all 8 bits to store the last byte of the original
/// value.
pub struct ShortUsize(pub usize);

impl Serialize for ShortUsize {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_tuple(0)?;

        let mut rem_len = self.0;
        loop {
            let mut elem = (rem_len & 0x7f) as u8;
            rem_len >>= 7;
            if rem_len == 0 {
                seq.serialize_element(&elem)?;
                break;
            } else {
                elem |= 0x80;
                seq.serialize_element(&elem)?;
            }
        }
        seq.end()
    }
}

struct ShortLenVisitor;

impl<'de> Visitor<'de> for ShortLenVisitor {
    type Value = ShortUsize;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a multi-byte length")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<ShortUsize, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut len: usize = 0;
        let mut size: usize = 0;
        loop {
            let elem: u8 = seq
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(size, &self))?;

            len |= (elem as usize & 0x7f) << (size * 7);
            size += 1;

            if elem as usize & 0x80 == 0 {
                break;
            }

            if size > size_of::<usize>() + 1 {
                return Err(de::Error::invalid_length(size, &self));
            }
        }

        Ok(ShortUsize(len))
    }
}

impl<'de> Deserialize<'de> for ShortUsize {
    fn deserialize<D>(deserializer: D) -> Result<ShortUsize, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_tuple(9, ShortLenVisitor)
    }
}

/// If you don't want to use the ShortVec newtype, you can do ShortVec
/// serialization on an ordinary vector with the following field annotation:
///
/// #[serde(with = "short_vec")]
///
pub fn serialize<S: Serializer, T: Serialize>(
    elements: &[T],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut seq = serializer.serialize_tuple(0)?;

    let short_len = ShortUsize(elements.len());
    seq.serialize_element(&short_len)?;

    for element in elements {
        seq.serialize_element(element)?;
    }
    seq.end()
}

struct ShortVecVisitor<T> {
    _t: PhantomData<T>,
}

impl<'de, T> Visitor<'de> for ShortVecVisitor<T>
where
    T: Deserialize<'de>,
{
    type Value = Vec<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a Vec with a multi-byte length")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Vec<T>, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let short_len: ShortUsize = seq
            .next_element()?
            .ok_or_else(|| de::Error::invalid_length(0, &self))?;
        let len = short_len.0;

        let mut result = Vec::with_capacity(len);
        for i in 0..len {
            let elem = seq
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(i, &self))?;
            result.push(elem);
        }
        Ok(result)
    }
}

/// If you don't want to use the ShortVec newtype, you can do ShortVec
/// deserialization on an ordinary vector with the following field annotation:
///
/// #[serde(with = "short_vec")]
///
pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let visitor = ShortVecVisitor { _t: PhantomData };
    deserializer.deserialize_tuple(std::usize::MAX, visitor)
}

pub struct ShortVec<T>(pub Vec<T>);

impl<T: Serialize> Serialize for ShortVec<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize(&self.0, serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for ShortVec<T> {
    fn deserialize<D>(deserializer: D) -> Result<ShortVec<T>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize(deserializer).map(ShortVec)
    }
}

/// Return the serialized length.
pub fn encode_len(len: usize) -> Vec<u8> {
    bincode::serialize(&ShortUsize(len)).unwrap()
}

/// Return the decoded value and how many bytes it consumed.
pub fn decode_len(bytes: &[u8]) -> (usize, usize) {
    let short_len: ShortUsize = bincode::deserialize(bytes).unwrap();
    let num_bytes = bincode::serialized_size(&short_len).unwrap() as usize;
    (short_len.0, num_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{deserialize, serialize};

    fn assert_len_encoding(len: usize, bytes: &[u8]) {
        assert_eq!(encode_len(len), bytes, "unexpected usize encoding");
        assert_eq!(
            decode_len(bytes),
            (len, bytes.len()),
            "unexpected usize decoding"
        );
    }

    #[test]
    fn test_short_vec_encode_len() {
        assert_len_encoding(0x0, &[0x0]);
        assert_len_encoding(0x7f, &[0x7f]);
        assert_len_encoding(0x80, &[0x80, 0x01]);
        assert_len_encoding(0xff, &[0xff, 0x01]);
        assert_len_encoding(0x100, &[0x80, 0x02]);
        assert_len_encoding(0x7fff, &[0xff, 0xff, 0x01]);
        assert_len_encoding(0x200000, &[0x80, 0x80, 0x80, 0x01]);
        assert_len_encoding(0x7ffffffff, &[0xff, 0xff, 0xff, 0xff, 0x7f]);
    }

    #[test]
    #[should_panic]
    fn test_short_vec_decode_zero_len() {
        decode_len(&[]);
    }

    #[test]
    fn test_short_vec_u8() {
        let vec = ShortVec(vec![4u8; 32]);
        let bytes = serialize(&vec).unwrap();
        assert_eq!(bytes.len(), vec.0.len() + 1);

        let vec1: ShortVec<u8> = deserialize(&bytes).unwrap();
        assert_eq!(vec.0, vec1.0);
    }
}
