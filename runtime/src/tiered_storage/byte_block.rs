use {
    crate::tiered_storage::footer::ByteBlockFormat,
    std::{
        io::{Cursor, Write},
        mem,
    },
};

enum ByteBlockEncoder {
    Raw(Cursor<Vec<u8>>),
}

pub struct ByteBlockWriter {
    len: usize,
    encoder: ByteBlockEncoder,
}

impl ByteBlockWriter {
    pub fn new(encoding: ByteBlockFormat) -> Self {
        Self {
            len: 0,
            encoder: match encoding {
                ByteBlockFormat::AlignedRaw => ByteBlockEncoder::Raw(Cursor::new(Vec::new())),
                ByteBlockFormat::Lz4 => todo!(),
            },
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn write_type<T>(&mut self, value: &T) -> std::io::Result<usize> {
        let len = mem::size_of::<T>();
        let value_ptr = (value as *const T) as *const u8;
        let ptr = unsafe { std::slice::from_raw_parts(value_ptr, len) };
        self.write(ptr, len)?;
        Ok(len)
    }

    pub fn write(&mut self, buf: &[u8], len: usize) -> std::io::Result<()> {
        let result = match &mut self.encoder {
            ByteBlockEncoder::Raw(cursor) => cursor.write_all(&buf[0..len]),
        };
        if result.is_ok() {
            self.len += len;
        }
        result
    }

    pub fn finish(self) -> std::io::Result<(Vec<u8>, usize)> {
        match self.encoder {
            ByteBlockEncoder::Raw(cursor) => Ok((cursor.into_inner(), self.len)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_type<T>(buffer: &[u8], offset: usize) -> (&T, usize) {
        let size = std::mem::size_of::<T>();
        let (next, overflow) = offset.overflowing_add(size);
        if overflow || next > buffer.len() {
            panic!();
        }
        let data = &buffer[offset..next];
        let ptr = data.as_ptr() as *const u8;

        let slice = unsafe { std::slice::from_raw_parts(ptr, size) };
        let slice_ptr = slice.as_ptr() as *const T;
        (unsafe { &*slice_ptr }, next)
    }

    #[test]
    fn test_write_single_raw_format() {
        let mut writer = ByteBlockWriter::new(ByteBlockFormat::AlignedRaw);
        let value: u32 = 42;

        writer.write_type(&value).unwrap();
        assert_eq!(writer.len(), mem::size_of::<u32>());

        let (buffer, len) = writer.finish().unwrap();
        assert_eq!(len, mem::size_of::<u32>());
        assert_eq!(buffer.len(), mem::size_of::<u32>());

        let (value_from_buffer, next): (&u32, usize) = read_type(&buffer, 0);
        assert_eq!(value, *value_from_buffer);
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
        let mut writer = ByteBlockWriter::new(ByteBlockFormat::AlignedRaw);
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

        let (buffer, len) = writer.finish().unwrap();
        assert_eq!(len, mem::size_of::<TestMetaStruct>() * 3 + 600);

        // verify meta1 and its data
        let (meta1_from_buffer, next1): (&TestMetaStruct, usize) = read_type(&buffer, 0);
        assert_eq!(test_metas[0], *meta1_from_buffer);
        assert_eq!(
            test_data1,
            buffer[next1..next1 + meta1_from_buffer.data_len]
        );

        // verify meta2 and its data
        let (meta2_from_buffer, next2): (&TestMetaStruct, usize) =
            read_type(&buffer, next1 + meta1_from_buffer.data_len);
        assert_eq!(test_metas[1], *meta2_from_buffer);
        assert_eq!(
            test_data2,
            buffer[next2..next2 + meta2_from_buffer.data_len]
        );

        // verify meta3 and its data
        let (meta3_from_buffer, next3): (&TestMetaStruct, usize) =
            read_type(&buffer, next2 + meta2_from_buffer.data_len);
        assert_eq!(test_metas[2], *meta3_from_buffer);
        assert_eq!(
            test_data3,
            buffer[next3..next3 + meta3_from_buffer.data_len]
        );
    }
}
