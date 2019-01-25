use bincode::{deserialize_from, serialize_into, Error};
use serde::Serialize;
use std::io::{Cursor, Read, Write};
use std::mem::size_of;

pub fn encode_len<W: Write>(writer: &mut W, len: usize) -> Result<(), Error> {
    let mut rem_len = len;
    loop {
        let mut elem = (rem_len & 0x7f) as u8;
        rem_len >>= 7;
        if rem_len == 0 {
            writer.write_all(&[elem])?;
            break;
        } else {
            elem |= 0x80;
            writer.write_all(&[elem])?;
        }
    }
    Ok(())
}

pub fn decode_len<R: Read>(reader: &mut R) -> Result<usize, Error> {
    let mut len: usize = 0;
    let mut size: usize = 0;
    loop {
        let mut elem = [0u8; 1];
        reader.read_exact(&mut elem)?;
        len |= (elem[0] as usize & 0x7f) << (size * 7);
        size += 1;
        if elem[0] as usize & 0x80 == 0 {
            break;
        }
        assert!(size <= size_of::<usize>() + 1);
    }
    Ok(len)
}

pub fn serialize_vec_with<T>(
    mut writer: &mut Cursor<&mut [u8]>,
    input: &[T],
    ser_fn: fn(&mut Cursor<&mut [u8]>, &T) -> Result<(), Error>,
) -> Result<(), Error> {
    encode_len(&mut writer, input.len())?;
    input.iter().for_each(|e| ser_fn(&mut writer, &e).unwrap());
    Ok(())
}

pub fn serialize_vec_bytes<W: Write>(mut writer: W, input: &[u8]) -> Result<(), Error> {
    let len = input.len();
    encode_len(&mut writer, len)?;
    writer.write_all(input)?;
    Ok(())
}

pub fn serialize_vec<W: Write, T>(mut writer: W, input: &[T]) -> Result<(), Error>
where
    T: Serialize,
{
    let len = input.len();
    encode_len(&mut writer, len)?;
    input
        .iter()
        .for_each(|e| serialize_into(&mut writer, &e).unwrap());
    Ok(())
}

pub fn deserialize_vec_bytes<R: Read>(mut reader: &mut R) -> Result<Vec<u8>, Error> {
    let vec_len = decode_len(&mut reader)?;
    let mut buf = vec![0; vec_len];
    reader.read_exact(&mut buf[..])?;
    Ok(buf)
}

pub fn deserialize_vec<R: Read, T>(mut reader: &mut R) -> Result<Vec<T>, Error>
where
    T: serde::de::DeserializeOwned,
{
    let vec_len = decode_len(&mut reader)?;
    let mut vec: Vec<T> = Vec::with_capacity(vec_len);
    for _ in 0..vec_len {
        let t: T = deserialize_from(&mut reader)?;
        vec.push(t);
    }
    Ok(vec)
}

pub fn deserialize_vec_with<T>(
    mut reader: &mut Cursor<&[u8]>,
    deser_fn: fn(&mut Cursor<&[u8]>) -> Result<T, Error>,
) -> Result<Vec<T>, Error> {
    let vec_len = decode_len(&mut reader)?;
    let mut vec: Vec<T> = Vec::with_capacity(vec_len);
    for _ in 0..vec_len {
        let t: T = deser_fn(&mut reader)?;
        vec.push(t);
    }
    Ok(vec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{deserialize, serialize, serialized_size};
    use serde::ser::Serializer;
    use serde::Deserialize;
    use std::fmt;
    use std::io::Cursor;

    #[test]
    fn test_shortvec_encode_len() {
        let mut buf = vec![0u8; size_of::<u64>() + 1];
        let mut wr = Cursor::new(&mut buf[..]);
        encode_len(&mut wr, 0x0).unwrap();
        let vec = wr.get_ref()[..wr.position() as usize].to_vec();
        assert_eq!(vec, vec![0u8]);
        wr.set_position(0);
        encode_len(&mut wr, 0x5).unwrap();
        let vec = wr.get_ref()[..wr.position() as usize].to_vec();
        assert_eq!(vec, vec![0x5u8]);
        wr.set_position(0);
        encode_len(&mut wr, 0x7f).unwrap();
        let vec = wr.get_ref()[..wr.position() as usize].to_vec();
        assert_eq!(vec, vec![0x7fu8]);
        wr.set_position(0);
        encode_len(&mut wr, 0x80).unwrap();
        let vec = wr.get_ref()[..wr.position() as usize].to_vec();
        assert_eq!(vec, vec![0x80u8, 0x01u8]);
        wr.set_position(0);
        encode_len(&mut wr, 0xff).unwrap();
        let vec = wr.get_ref()[..wr.position() as usize].to_vec();
        assert_eq!(vec, vec![0xffu8, 0x01u8]);
        wr.set_position(0);
        encode_len(&mut wr, 0x100).unwrap();
        let vec = wr.get_ref()[..wr.position() as usize].to_vec();
        assert_eq!(vec, vec![0x80u8, 0x02u8]);
        wr.set_position(0);
        encode_len(&mut wr, 0x7fff).unwrap();
        let vec = wr.get_ref()[..wr.position() as usize].to_vec();
        assert_eq!(vec, vec![0xffu8, 0xffu8, 0x01u8]);
        wr.set_position(0);
        encode_len(&mut wr, 0x200000).unwrap();
        let vec = wr.get_ref()[..wr.position() as usize].to_vec();
        assert_eq!(vec, vec![0x80u8, 0x80u8, 0x80u8, 0x01u8]);
        wr.set_position(0);
        encode_len(&mut wr, 0x7ffffffff).unwrap();
        let vec = wr.get_ref()[..wr.position() as usize].to_vec();
        assert_eq!(vec, vec![0xffu8, 0xffu8, 0xffu8, 0xffu8, 0x7fu8]);
    }

    #[test]
    #[should_panic]
    fn test_shortvec_decode_zero_len() {
        let mut buf = vec![];
        let mut rd = Cursor::new(&mut buf[..]);
        assert_eq!(decode_len(&mut rd).unwrap(), 0);
        assert_eq!(rd.position(), 0);
    }

    #[test]
    fn test_shortvec_decode_len() {
        let mut buf = vec![0u8];
        let mut rd = Cursor::new(&mut buf[..]);
        assert_eq!(decode_len(&mut rd).unwrap(), 0);
        assert_eq!(rd.position(), 1);
        let mut buf = vec![5u8];
        let mut rd = Cursor::new(&mut buf[..]);
        assert_eq!(decode_len(&mut rd).unwrap(), 5);
        assert_eq!(rd.position(), 1);
        let mut buf = vec![0x7fu8];
        let mut rd = Cursor::new(&mut buf[..]);
        assert_eq!(decode_len(&mut rd).unwrap(), 0x7f);
        assert_eq!(rd.position(), 1);
        let mut buf = vec![0x80u8, 0x01u8];
        let mut rd = Cursor::new(&mut buf[..]);
        assert_eq!(decode_len(&mut rd).unwrap(), 0x80);
        assert_eq!(rd.position(), 2);
        let mut buf = vec![0xffu8, 0x01u8];
        let mut rd = Cursor::new(&mut buf[..]);
        assert_eq!(decode_len(&mut rd).unwrap(), 0xff);
        assert_eq!(rd.position(), 2);
        let mut buf = vec![0x80u8, 0x02u8];
        let mut rd = Cursor::new(&mut buf[..]);
        assert_eq!(decode_len(&mut rd).unwrap(), 0x100);
        assert_eq!(rd.position(), 2);
        let mut buf = vec![0xffu8, 0xffu8, 0x01u8];
        let mut rd = Cursor::new(&mut buf[..]);
        assert_eq!(decode_len(&mut rd).unwrap(), 0x7fff);
        assert_eq!(rd.position(), 3);
        let mut buf = vec![0x80u8, 0x80u8, 0x80u8, 0x01u8];
        let mut rd = Cursor::new(&mut buf[..]);
        assert_eq!(decode_len(&mut rd).unwrap(), 0x200000);
        assert_eq!(rd.position(), 4);
        let mut buf = vec![0xffu8, 0xffu8, 0xffu8, 0xffu8, 0x7fu8];
        let mut rd = Cursor::new(&mut buf[..]);
        assert_eq!(decode_len(&mut rd).unwrap(), 0x7ffffffff);
        assert_eq!(rd.position(), 5);
    }

    #[test]
    fn test_shortvec_u8() {
        let vec: Vec<u8> = vec![4; 32];
        let mut buf = vec![0u8; serialized_size(&vec).unwrap() as usize + 1];
        let mut wr = Cursor::new(&mut buf[..]);
        serialize_vec_bytes(&mut wr, &vec).unwrap();
        let size = wr.position() as usize;
        let ser = &wr.into_inner()[..size];
        assert_eq!(ser.len(), vec.len() + 1);
        let mut rd = Cursor::new(&ser[..]);
        let deser: Vec<u8> = deserialize_vec_bytes(&mut rd).unwrap();
        assert_eq!(vec, deser);
    }

    #[derive(Debug, Eq, PartialEq)]
    struct TestVec {
        vec_u8: Vec<u8>,
        id: u8,
        vec_u32: Vec<u32>,
    }

    impl TestVec {
        pub fn serialize_with(
            mut writer: &mut Cursor<&mut [u8]>,
            tv: &TestVec,
        ) -> Result<(), Error> {
            serialize_vec(&mut writer, &tv.vec_u8).unwrap();
            serialize_into(&mut writer, &tv.id).unwrap();
            serialize_vec(&mut writer, &tv.vec_u32).unwrap();
            Ok(())
        }
        pub fn from_bytes(mut reader: &mut Cursor<&[u8]>) -> Result<Self, Error> {
            let vec_u8: Vec<u8> = deserialize_vec(&mut reader).unwrap();
            let id: u8 = deserialize_from(&mut reader).unwrap();
            let vec_u32: Vec<u32> = deserialize_vec(&mut reader).unwrap();
            Ok(TestVec {
                vec_u8,
                id,
                vec_u32,
            })
        }
        pub fn serialized_size(&self) -> u64 {
            let mut buf = vec![0u8; size_of::<u64>() + 1];
            let mut size = size_of::<u64>();
            let mut wr = Cursor::new(&mut buf[..]);
            encode_len(&mut wr, self.vec_u8.len()).unwrap();
            size += wr.position() as usize + self.vec_u8.len() * size_of::<u8>();
            size += size_of::<u8>();
            wr.set_position(0);
            encode_len(&mut wr, self.vec_u32.len()).unwrap();
            size += wr.position() as usize + self.vec_u32.len() * size_of::<u32>();
            size as u64
        }
    }

    impl Serialize for TestVec {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            use serde::ser::Error;
            let mut buf = vec![0u8; self.serialized_size() as usize];
            let mut wr = Cursor::new(&mut buf[..]);
            serialize_vec(&mut wr, &self.vec_u8).map_err(Error::custom)?;
            serialize_into(&mut wr, &self.id).map_err(Error::custom)?;
            serialize_vec(&mut wr, &self.vec_u32).map_err(Error::custom)?;
            let len = wr.position() as usize;
            serializer.serialize_bytes(&wr.into_inner()[..len])
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
            let mut rd = Cursor::new(&data[..]);
            let v1: Vec<u8> = deserialize_vec(&mut rd).map_err(Error::custom)?;
            let id: u8 = deserialize_from(&mut rd).map_err(Error::custom)?;
            let v2: Vec<u32> = deserialize_vec(&mut rd).map_err(Error::custom)?;
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
        let size = tvec.serialized_size() as usize;
        assert_eq!(
            size,
            tvec.vec_u8.len() + 1 + 1 + (tvec.vec_u32.len() * 4) + 1 + 8
        );
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
        fn serialized_size(tvec: &TestVecArr) -> u64 {
            let mut buf = vec![0u8; size_of::<u64>() + 1];
            let mut wr = Cursor::new(&mut buf[..]);
            encode_len(&mut wr, tvec.data.len()).unwrap();
            let size: u64 = tvec
                .data
                .iter()
                .map(|tv| TestVec::serialized_size(&tv))
                .sum();
            size_of::<u64>() as u64 + size + wr.position()
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
        let mut buf = vec![0u8; TestVecArr::serialized_size(&tvecarr) as usize];
        let mut wr = Cursor::new(&mut buf[..]);
        serialize_vec_with(&mut wr, &tvecarr.data, TestVec::serialize_with).unwrap();
        let size = wr.position() as usize;
        let ser = &wr.into_inner()[..size];
        let mut rd = Cursor::new(&ser[..]);
        let deser = deserialize_vec_with(&mut rd, TestVec::from_bytes).unwrap();
        assert_eq!(tvecarr.data, deser);
    }
}
