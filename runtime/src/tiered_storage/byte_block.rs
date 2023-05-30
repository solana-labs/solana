use {
    crate::tiered_storage::footer::AccountBlockFormat,
    std::{
        io::{Cursor, Write},
        mem,
    },
};

#[derive(Debug)]
pub enum ByteBlockWriter {
    Raw(Cursor<Vec<u8>>),
}

impl ByteBlockWriter {
    pub fn new(encoding: AccountBlockFormat) -> Self {
        match encoding {
            AccountBlockFormat::AlignedRaw => Self::Raw(Cursor::new(Vec::new())),
            AccountBlockFormat::Lz4 => todo!(),
        }
    }

    pub fn write_type<T>(&mut self, value: &T) -> std::io::Result<usize> {
        let size = mem::size_of::<T>();
        let ptr = value as *const _ as *const u8;
        let slice = unsafe { std::slice::from_raw_parts(ptr, size) };
        self.write(slice)?;
        Ok(size)
    }

    pub fn write(&mut self, buf: &[u8]) -> std::io::Result<()> {
        match self {
            Self::Raw(cursor) => cursor.write_all(buf)?,
        };
        Ok(())
    }

    pub fn finish(self) -> std::io::Result<Vec<u8>> {
        match self {
            Self::Raw(cursor) => Ok(cursor.into_inner()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_type<T>(buffer: &[u8], offset: usize) -> (T, usize) {
        let size = std::mem::size_of::<T>();
        let (next, overflow) = offset.overflowing_add(size);
        assert!(!overflow && next <= buffer.len());
        let data = &buffer[offset..next];
        let ptr = data.as_ptr() as *const T;

        (unsafe { std::ptr::read_unaligned(ptr) }, next)
    }

    #[test]
    fn test_write_single_raw_format() {
        let mut writer = ByteBlockWriter::new(AccountBlockFormat::AlignedRaw);
        let value: u32 = 42;

        writer.write_type(&value).unwrap();

        let buffer = writer.finish().unwrap();
        assert_eq!(buffer.len(), mem::size_of::<u32>());

        let (value_from_buffer, next) = read_type::<u32>(&buffer, 0);
        assert_eq!(value, value_from_buffer);
        assert_eq!(next, mem::size_of::<u32>());
    }

    #[derive(Debug, PartialEq)]
    struct TestMetaStruct {
        lamports: u64,
        owner_index: u32,
        data_len: usize,
    }

    #[test]
    fn test_write_multiple_raw_format() {
        let mut writer = ByteBlockWriter::new(AccountBlockFormat::AlignedRaw);
        let test_metas: Vec<TestMetaStruct> = vec![
            TestMetaStruct {
                lamports: 10,
                owner_index: 0,
                data_len: 100,
            },
            TestMetaStruct {
                lamports: 20,
                owner_index: 1,
                data_len: 200,
            },
            TestMetaStruct {
                lamports: 30,
                owner_index: 2,
                data_len: 300,
            },
        ];
        let test_data1 = [11u8; 100];
        let test_data2 = [22u8; 200];
        let test_data3 = [33u8; 300];

        // Write the above meta and data in an interleaving way.
        writer.write_type(&test_metas[0]).unwrap();
        writer.write_type(&test_data1).unwrap();
        writer.write_type(&test_metas[1]).unwrap();
        writer.write_type(&test_data2).unwrap();
        writer.write_type(&test_metas[2]).unwrap();
        writer.write_type(&test_data3).unwrap();

        let buffer = writer.finish().unwrap();
        assert_eq!(
            buffer.len(),
            mem::size_of::<TestMetaStruct>() * 3
                + mem::size_of_val(&test_data1)
                + mem::size_of_val(&test_data2)
                + mem::size_of_val(&test_data3)
        );

        // verify meta1 and its data
        let (meta1_from_buffer, next1) = read_type::<TestMetaStruct>(&buffer, 0);
        assert_eq!(test_metas[0], meta1_from_buffer);
        assert_eq!(test_data1, buffer[next1..][..meta1_from_buffer.data_len]);

        // verify meta2 and its data
        let (meta2_from_buffer, next2) =
            read_type::<TestMetaStruct>(&buffer, next1 + meta1_from_buffer.data_len);
        assert_eq!(test_metas[1], meta2_from_buffer);
        assert_eq!(test_data2, buffer[next2..][..meta2_from_buffer.data_len]);

        // verify meta3 and its data
        let (meta3_from_buffer, next3) =
            read_type::<TestMetaStruct>(&buffer, next2 + meta2_from_buffer.data_len);
        assert_eq!(test_metas[2], meta3_from_buffer);
        assert_eq!(test_data3, buffer[next3..][..meta3_from_buffer.data_len]);
    }
}
