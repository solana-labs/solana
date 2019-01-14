use bincode::{deserialize, serialize, Error};
use serde::{Deserialize, Serialize};

pub fn encode_len(len: usize) -> Vec<u8> {
    let mut vec: Vec<u8> = vec![];
    let mut rem_len = len;
    loop {
        let mut elem = (rem_len & 0x7f) as u8;
        rem_len >>= 7;
        if rem_len == 0 {
            vec.push(elem);
            break;
        } else {
            elem |= 0x80;
            vec.push(elem);
        }
    }
    vec
}

pub fn decode_len(vec: &[u8]) -> (usize, usize) {
    let mut len: usize = 0;
    let mut size: usize = 0;
    if vec.is_empty() {
        return (0, 0);
    }
    loop {
        let elem = vec[size] as usize;
        len |= (elem & 0x7f) << (size * 7);
        size += 1;
        if elem & 0x80 == 0 {
            break;
        }
        assert!(size <= std::mem::size_of::<usize>() + 1);
    }
    (len, size)
}

fn serialize_vec_internal<T>(input: &[T], mut elems: Vec<u8>) -> Result<Vec<u8>, Error> {
    let mut vec = encode_len(input.len());
    vec.append(&mut elems);
    Ok(vec)
}

pub fn serialize_vec<T>(input: &[T]) -> Result<Vec<u8>, Error>
where
    T: Serialize,
{
    let elems: Vec<u8> = input.iter().flat_map(|e| serialize(&e).unwrap()).collect();
    serialize_vec_internal(input, elems)
}

pub fn serialize_vec_with<T>(
    input: &[T],
    ser_fn: fn(&T) -> Result<Vec<u8>, Error>,
) -> Result<Vec<u8>, Error> {
    let elems: Vec<u8> = input.iter().flat_map(|e| ser_fn(&e).unwrap()).collect();
    serialize_vec_internal(input, elems)
}

pub fn deserialize_vec<'a, T>(s: &'a [u8]) -> Result<(usize, Vec<T>), Error>
where
    T: Deserialize<'a>,
{
    let (vec_len, size) = decode_len(s);
    let mut elem_offset = size as usize;
    let mut vec: Vec<T> = vec![];
    for _ in 0..vec_len {
        let t: T = deserialize(&s[elem_offset..])?;
        elem_offset += std::mem::size_of_val(&t);
        vec.push(t);
    }
    Ok((elem_offset, vec))
}

#[allow(clippy::type_complexity)]
pub fn deserialize_vec_with<'a, T>(
    s: &'a [u8],
    deser_fn: fn(&[u8]) -> Result<(usize, T), Error>,
) -> Result<(usize, Vec<T>), Error>
where
    T: Deserialize<'a>,
{
    let (vec_len, size) = decode_len(s);
    let mut elem_offset = size as usize;
    let mut vec: Vec<T> = vec![];
    for _ in 0..vec_len {
        let (offset, t): (usize, T) = deser_fn(&s[elem_offset..])?;
        elem_offset += offset;
        vec.push(t);
    }
    Ok((elem_offset, vec))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::ser::Serializer;
    use std::fmt;

    #[test]
    fn test_shortvec_encode_len() {
        assert_eq!(encode_len(0x0), vec![0u8]);
        assert_eq!(encode_len(0x5), vec![0x5u8]);
        assert_eq!(encode_len(0x7f), vec![0x7fu8]);
        assert_eq!(encode_len(0x80), vec![0x80u8, 0x01u8]);
        assert_eq!(encode_len(0xff), vec![0xffu8, 0x01u8]);
        assert_eq!(encode_len(0x100), vec![0x80u8, 0x02u8]);
        assert_eq!(encode_len(0x7fff), vec![0xffu8, 0xffu8, 0x01u8]);
        assert_eq!(encode_len(0x200000), vec![0x80u8, 0x80u8, 0x80u8, 0x01u8]);
        assert_eq!(
            encode_len(0x7ffffffff),
            vec![0xffu8, 0xffu8, 0xffu8, 0xffu8, 0x7fu8]
        );
    }

    #[test]
    fn test_shortvec_decode_len() {
        assert_eq!(decode_len(&vec![]), (0, 0));
        assert_eq!(decode_len(&vec![0u8]), (0, 1));
        assert_eq!(decode_len(&vec![5u8]), (5, 1));
        assert_eq!(decode_len(&vec![0x7fu8]), (0x7f, 1));
        assert_eq!(decode_len(&vec![0x80u8, 0x01u8]), (0x80, 2));
        assert_eq!(decode_len(&vec![0xffu8, 0x01u8]), (0xff, 2));
        assert_eq!(decode_len(&vec![0x80u8, 0x02u8]), (0x100, 2));
        assert_eq!(decode_len(&vec![0xffu8, 0xffu8, 0x01u8]), (0x7fff, 3));
        assert_eq!(
            decode_len(&vec![0x80u8, 0x80u8, 0x80u8, 0x01u8]),
            (0x200000, 4)
        );
        assert_eq!(
            decode_len(&vec![0xffu8, 0xffu8, 0xffu8, 0xffu8, 0x7fu8]),
            (0x7ffffffff, 5)
        );
    }

    #[test]
    fn test_shortvec_u8() {
        let vec: Vec<u8> = vec![4; 32];
        let ser = serialize_vec(&vec).unwrap();
        assert_eq!(ser.len(), vec.len() + 1);
        let (len, deser): (usize, Vec<u8>) = deserialize_vec(&ser).unwrap();
        assert_eq!(vec, deser);
        assert_eq!(len, vec.len() + 1);
    }

    #[derive(Debug, Eq, PartialEq)]
    struct TestVec {
        vec_u8: Vec<u8>,
        id: u8,
        vec_u32: Vec<u32>,
    }

    impl TestVec {
        pub fn into_bytes(&self) -> Result<Vec<u8>, Error> {
            let mut v1 = serialize_vec(&self.vec_u8).unwrap();
            let mut v2 = serialize_vec(&self.vec_u32).unwrap();
            v1.append(&mut vec![self.id]);
            v1.append(&mut v2);
            Ok(v1)
        }
        pub fn from_bytes(data: &[u8]) -> Result<(usize, Self), Error> {
            let (len, vec_u8): (usize, Vec<u8>) = deserialize_vec(&data).unwrap();
            let id = data[len];
            let (len1, vec_u32): (usize, Vec<u32>) = deserialize_vec(&data[len + 1..]).unwrap();
            Ok((
                len + len1 + 1,
                TestVec {
                    vec_u8,
                    id,
                    vec_u32,
                },
            ))
        }
    }

    impl Serialize for TestVec {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            use serde::ser::Error;
            let mut v1 = serialize_vec(&self.vec_u8).map_err(Error::custom)?;
            let mut v2 = serialize_vec(&self.vec_u32).map_err(Error::custom)?;
            v1.append(&mut vec![self.id]);
            v1.append(&mut v2);
            serializer.serialize_bytes(&v1)
        }
    }

    struct TestVecVisitor;
    impl<'a> serde::de::Visitor<'a> for TestVecVisitor {
        type Value = TestVec;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("Expecting TestVec")
        }
        fn visit_bytes<E>(self, data: &[u8]) -> Result<TestVec, E>
        where
            E: serde::de::Error,
        {
            use serde::de::Error;
            let (len, v1): (usize, Vec<u8>) = deserialize_vec(data).map_err(Error::custom)?;
            let id = data[len];
            let (_, v2): (usize, Vec<u32>) =
                deserialize_vec(&data[len + 1..]).map_err(Error::custom)?;
            Ok(TestVec {
                vec_u8: v1,
                id,
                vec_u32: v2,
            })
        }
    }
    impl<'de> Deserialize<'de> for TestVec {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: ::serde::Deserializer<'de>,
        {
            deserializer.deserialize_bytes(TestVecVisitor)
        }
    }

    #[test]
    fn test_shortvec_testvec() {
        let tvec: TestVec = TestVec {
            vec_u8: vec![4; 32],
            id: 5,
            vec_u32: vec![6; 32],
        };
        let ser = serialize(&tvec).unwrap();
        assert_eq!(
            ser.len(),
            tvec.vec_u8.len() + 1 + 1 + (tvec.vec_u32.len() * 4) + 1 + 8
        );
        let deser = deserialize(&ser).unwrap();
        assert_eq!(tvec, deser);
    }

    #[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
    struct TestVecArr {
        data: Vec<TestVec>,
    }
    impl TestVecArr {
        fn new() -> Self {
            TestVecArr { data: vec![] }
        }
    }
    #[test]
    fn test_shortvec_testvec_with() {
        let tvec1 = TestVec {
            vec_u8: vec![4; 32],
            id: 5,
            vec_u32: vec![6; 32],
        };
        let tvec2 = TestVec {
            vec_u8: vec![7; 32],
            id: 8,
            vec_u32: vec![9; 32],
        };
        let tvec3 = TestVec {
            vec_u8: vec![],
            id: 10,
            vec_u32: vec![11; 32],
        };
        let mut tvecarr: TestVecArr = TestVecArr::new();
        tvecarr.data.push(tvec1);
        tvecarr.data.push(tvec2);
        tvecarr.data.push(tvec3);
        let ser = serialize_vec_with(&tvecarr.data, TestVec::into_bytes).unwrap();
        let (_, deser) = deserialize_vec_with(&ser, TestVec::from_bytes).unwrap();
        assert_eq!(tvecarr.data, deser);
    }
}
