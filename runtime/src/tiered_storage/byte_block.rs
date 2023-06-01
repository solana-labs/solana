//! The utility structs and functions for writing byte blocks for the
//! accounts db tiered storage.

use {
    crate::tiered_storage::footer::AccountBlockFormat,
    std::{
        io::{Cursor, Read, Write},
        mem,
    },
};

/// The byte block writer.
///
/// All writes (`write_type` and `write`) will be buffered in the internal
/// buffer of the ByteBlockWriter using the specified encoding.
///
/// To finalize all the writes, invoke `finish` to obtain the encoded byte
/// block.
#[derive(Debug)]
pub enum ByteBlockWriter {
    Raw(Cursor<Vec<u8>>),
    Lz4(lz4::Encoder<Vec<u8>>),
}

impl ByteBlockWriter {
    /// Create a ByteBlockWriter from the specified AccountBlockFormat.
    pub fn new(encoding: AccountBlockFormat) -> Self {
        match encoding {
            AccountBlockFormat::AlignedRaw => Self::Raw(Cursor::new(Vec::new())),
            AccountBlockFormat::Lz4 => Self::Lz4(
                lz4::EncoderBuilder::new()
                    .level(0)
                    .build(Vec::new())
                    .unwrap(),
            ),
        }
    }

    /// Write the specified typed instance to the internal buffer of
    /// the ByteBlockWriter instance.
    pub fn write_type<T>(&mut self, value: &T) -> std::io::Result<usize> {
        let size = mem::size_of::<T>();
        let ptr = value as *const _ as *const u8;
        let slice = unsafe { std::slice::from_raw_parts(ptr, size) };
        self.write(slice)?;
        Ok(size)
    }

    /// Write the specified typed bytes to the internal buffer of the
    /// ByteBlockWriter instance.
    pub fn write(&mut self, buf: &[u8]) -> std::io::Result<()> {
        match self {
            Self::Raw(cursor) => cursor.write_all(buf)?,
            Self::Lz4(lz4_encoder) => lz4_encoder.write_all(buf)?,
        };
        Ok(())
    }

    /// Flush the internal byte buffer that collects all the previous writes
    /// into an encoded byte array.
    pub fn finish(self) -> std::io::Result<Vec<u8>> {
        match self {
            Self::Raw(cursor) => Ok(cursor.into_inner()),
            Self::Lz4(lz4_encoder) => {
                let (compressed_block, result) = lz4_encoder.finish();
                result?;
                Ok(compressed_block)
            }
        }
    }
}

/// The util struct for reading byte blocks.
pub struct ByteBlockReader;

impl ByteBlockReader {
    /// Decode the input byte array using the specified format.
    ///
    /// Typically, the input byte array is the output of ByteBlockWriter::finish().
    ///
    /// Note that calling this function with AccountBlockFormat::AlignedRaw encoding
    /// will result in panic as the input is already decoded.
    pub fn decode(encoding: AccountBlockFormat, input: &[u8]) -> std::io::Result<Vec<u8>> {
        match encoding {
            AccountBlockFormat::Lz4 => {
                let mut decoder = lz4::Decoder::new(input).unwrap();
                let mut output = vec![];
                decoder.read_to_end(&mut output)?;
                Ok(output)
            }
            AccountBlockFormat::AlignedRaw => panic!("the input buffer is already decoded"),
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

    fn write_single(format: AccountBlockFormat) {
        let mut writer = ByteBlockWriter::new(format);
        let value: u32 = 42;

        writer.write_type(&value).unwrap();

        let buffer = writer.finish().unwrap();

        let decoded_buffer = if format == AccountBlockFormat::AlignedRaw {
            buffer
        } else {
            ByteBlockReader::decode(format, &buffer).unwrap()
        };

        assert_eq!(decoded_buffer.len(), mem::size_of::<u32>());

        let (value_from_buffer, next) = read_type::<u32>(&decoded_buffer, 0);
        assert_eq!(value, value_from_buffer);

        if format != AccountBlockFormat::AlignedRaw {
            assert_eq!(next, mem::size_of::<u32>());
        }
    }

    #[test]
    fn test_write_single_raw_format() {
        write_single(AccountBlockFormat::AlignedRaw);
    }

    #[test]
    fn test_write_single_encoded_format() {
        write_single(AccountBlockFormat::Lz4);
    }

    #[derive(Debug, PartialEq)]
    struct TestMetaStruct {
        lamports: u64,
        owner_index: u32,
        data_len: usize,
    }

    fn write_multiple(format: AccountBlockFormat) {
        let mut writer = ByteBlockWriter::new(format);
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

        let decoded_buffer = if format == AccountBlockFormat::AlignedRaw {
            buffer
        } else {
            ByteBlockReader::decode(format, &buffer).unwrap()
        };

        assert_eq!(
            decoded_buffer.len(),
            mem::size_of::<TestMetaStruct>() * 3
                + mem::size_of_val(&test_data1)
                + mem::size_of_val(&test_data2)
                + mem::size_of_val(&test_data3)
        );

        // verify meta1 and its data
        let (meta1_from_buffer, next1) = read_type::<TestMetaStruct>(&decoded_buffer, 0);
        assert_eq!(test_metas[0], meta1_from_buffer);
        assert_eq!(
            test_data1,
            decoded_buffer[next1..][..meta1_from_buffer.data_len]
        );

        // verify meta2 and its data
        let (meta2_from_buffer, next2) =
            read_type::<TestMetaStruct>(&decoded_buffer, next1 + meta1_from_buffer.data_len);
        assert_eq!(test_metas[1], meta2_from_buffer);
        assert_eq!(
            test_data2,
            decoded_buffer[next2..][..meta2_from_buffer.data_len]
        );

        // verify meta3 and its data
        let (meta3_from_buffer, next3) =
            read_type::<TestMetaStruct>(&decoded_buffer, next2 + meta2_from_buffer.data_len);
        assert_eq!(test_metas[2], meta3_from_buffer);
        assert_eq!(
            test_data3,
            decoded_buffer[next3..][..meta3_from_buffer.data_len]
        );
    }

    #[test]
    fn test_write_multiple_raw_format() {
        write_multiple(AccountBlockFormat::AlignedRaw);
    }

    #[test]
    fn test_write_multiple_lz4_format() {
        write_multiple(AccountBlockFormat::Lz4);
    }
}
