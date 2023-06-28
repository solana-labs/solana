//! The utility structs and functions for writing byte blocks for the
//! accounts db tiered storage.

use {
    crate::tiered_storage::{footer::AccountBlockFormat, meta::AccountMetaOptionalFields},
    std::{
        io::{Cursor, Read, Write},
        mem,
    },
};

/// The encoder for the byte-block.
#[derive(Debug)]
pub enum ByteBlockEncoder {
    Raw(Cursor<Vec<u8>>),
    Lz4(lz4::Encoder<Vec<u8>>),
}

/// The byte block writer.
///
/// All writes (`write_type` and `write`) will be buffered in the internal
/// buffer of the ByteBlockWriter using the specified encoding.
///
/// To finalize all the writes, invoke `finish` to obtain the encoded byte
/// block.
#[derive(Debug)]
pub struct ByteBlockWriter {
    /// the encoder for the byte-block
    encoder: ByteBlockEncoder,
    /// the length of the raw data
    len: usize,
}

impl ByteBlockWriter {
    /// Create a ByteBlockWriter from the specified AccountBlockFormat.
    pub fn new(encoding: AccountBlockFormat) -> Self {
        Self {
            encoder: match encoding {
                AccountBlockFormat::AlignedRaw => ByteBlockEncoder::Raw(Cursor::new(Vec::new())),
                AccountBlockFormat::Lz4 => ByteBlockEncoder::Lz4(
                    lz4::EncoderBuilder::new()
                        .level(0)
                        .build(Vec::new())
                        .unwrap(),
                ),
            },
            len: 0,
        }
    }

    /// Return the length of the raw data (i.e. after decoding).
    pub fn raw_len(&self) -> usize {
        self.len
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

    /// Write all the Some fields of the specified AccountMetaOptionalFields.
    ///
    /// Note that the existance of each optional field is stored separately in
    /// AccountMetaFlags.
    pub fn write_optional_fields(
        &mut self,
        opt_fields: &AccountMetaOptionalFields,
    ) -> std::io::Result<usize> {
        let mut size = 0;
        if let Some(rent_epoch) = opt_fields.rent_epoch {
            size += self.write_type(&rent_epoch)?;
        }
        if let Some(hash) = opt_fields.account_hash {
            size += self.write_type(&hash)?;
        }
        if let Some(write_version) = opt_fields.write_version {
            size += self.write_type(&write_version)?;
        }

        debug_assert_eq!(size, opt_fields.size());

        Ok(size)
    }

    /// Write the specified typed bytes to the internal buffer of the
    /// ByteBlockWriter instance.
    pub fn write(&mut self, buf: &[u8]) -> std::io::Result<()> {
        match &mut self.encoder {
            ByteBlockEncoder::Raw(cursor) => cursor.write_all(buf)?,
            ByteBlockEncoder::Lz4(lz4_encoder) => lz4_encoder.write_all(buf)?,
        };
        self.len += buf.len();
        Ok(())
    }

    /// Flush the internal byte buffer that collects all the previous writes
    /// into an encoded byte array.
    pub fn finish(self) -> std::io::Result<Vec<u8>> {
        match self.encoder {
            ByteBlockEncoder::Raw(cursor) => Ok(cursor.into_inner()),
            ByteBlockEncoder::Lz4(lz4_encoder) => {
                let (compressed_block, result) = lz4_encoder.finish();
                result?;
                Ok(compressed_block)
            }
        }
    }
}

/// The util struct for reading byte blocks.
pub struct ByteBlockReader;

/// Reads the raw part of the input byte_block at the specified offset
/// as type T.
///
/// If `offset` + size_of::<T>() exceeds the size of the input byte_block,
/// then None will be returned.
pub fn read_type<T>(byte_block: &[u8], offset: usize) -> Option<&T> {
    let (next, overflow) = offset.overflowing_add(std::mem::size_of::<T>());
    if overflow || next > byte_block.len() {
        return None;
    }
    let ptr = byte_block[offset..].as_ptr() as *const T;
    debug_assert!(ptr as usize % std::mem::align_of::<T>() == 0);
    Some(unsafe { &*ptr })
}

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
    use {
        super::*,
        crate::account_storage::meta::StoredMetaWriteVersion,
        solana_sdk::{hash::Hash, stake_history::Epoch},
    };

    fn read_type_unaligned<T>(buffer: &[u8], offset: usize) -> (T, usize) {
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
        assert_eq!(writer.raw_len(), mem::size_of::<u32>());

        let buffer = writer.finish().unwrap();

        let decoded_buffer = if format == AccountBlockFormat::AlignedRaw {
            buffer
        } else {
            ByteBlockReader::decode(format, &buffer).unwrap()
        };

        assert_eq!(decoded_buffer.len(), mem::size_of::<u32>());

        let (value_from_buffer, next) = read_type_unaligned::<u32>(&decoded_buffer, 0);
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
        assert_eq!(
            writer.raw_len(),
            mem::size_of::<TestMetaStruct>() * 3
                + mem::size_of_val(&test_data1)
                + mem::size_of_val(&test_data2)
                + mem::size_of_val(&test_data3)
        );

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
        let (meta1_from_buffer, next1) = read_type_unaligned::<TestMetaStruct>(&decoded_buffer, 0);
        assert_eq!(test_metas[0], meta1_from_buffer);
        assert_eq!(
            test_data1,
            decoded_buffer[next1..][..meta1_from_buffer.data_len]
        );

        // verify meta2 and its data
        let (meta2_from_buffer, next2) = read_type_unaligned::<TestMetaStruct>(
            &decoded_buffer,
            next1 + meta1_from_buffer.data_len,
        );
        assert_eq!(test_metas[1], meta2_from_buffer);
        assert_eq!(
            test_data2,
            decoded_buffer[next2..][..meta2_from_buffer.data_len]
        );

        // verify meta3 and its data
        let (meta3_from_buffer, next3) = read_type_unaligned::<TestMetaStruct>(
            &decoded_buffer,
            next2 + meta2_from_buffer.data_len,
        );
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

    fn write_optional_fields(format: AccountBlockFormat) {
        let mut test_epoch = 5432312;
        let mut test_write_version = 231;

        let mut writer = ByteBlockWriter::new(format);
        let mut opt_fields_vec = vec![];
        let mut some_count = 0;

        // prepare a vector of optional fields that contains all combinations
        // of Some and None.
        for rent_epoch in [None, Some(test_epoch)] {
            for account_hash in [None, Some(Hash::new_unique())] {
                for write_version in [None, Some(test_write_version)] {
                    some_count += rent_epoch.map_or(0, |_| 1)
                        + account_hash.map_or(0, |_| 1)
                        + write_version.map_or(0, |_| 1);

                    opt_fields_vec.push(AccountMetaOptionalFields {
                        rent_epoch,
                        account_hash,
                        write_version,
                    });
                    test_write_version += 1;
                }
            }
            test_epoch += 1;
        }

        // write all the combinations of the optional fields
        let mut expected_size = 0;
        for opt_fields in &opt_fields_vec {
            writer.write_optional_fields(opt_fields).unwrap();
            expected_size += opt_fields.size();
        }

        let buffer = writer.finish().unwrap();
        let decoded_buffer = if format == AccountBlockFormat::AlignedRaw {
            buffer
        } else {
            ByteBlockReader::decode(format, &buffer).unwrap()
        };

        // first, verify whether the size of the decoded data matches the
        // expected size.
        assert_eq!(decoded_buffer.len(), expected_size);

        // verify the correctness of the written optional fields
        let mut verified_count = 0;
        let mut offset = 0;
        for opt_fields in &opt_fields_vec {
            if let Some(expected_rent_epoch) = opt_fields.rent_epoch {
                let rent_epoch = read_type::<Epoch>(&decoded_buffer, offset).unwrap();
                assert_eq!(*rent_epoch, expected_rent_epoch);
                verified_count += 1;
                offset += std::mem::size_of::<Epoch>();
            }
            if let Some(expected_hash) = opt_fields.account_hash {
                let hash = read_type::<Hash>(&decoded_buffer, offset).unwrap();
                assert_eq!(hash, &expected_hash);
                verified_count += 1;
                offset += std::mem::size_of::<Hash>();
            }
            if let Some(expected_write_version) = opt_fields.write_version {
                let write_version =
                    read_type::<StoredMetaWriteVersion>(&decoded_buffer, offset).unwrap();
                assert_eq!(*write_version, expected_write_version);
                verified_count += 1;
                offset += std::mem::size_of::<StoredMetaWriteVersion>();
            }
        }

        // make sure the number of Some fields matches the number of fields we
        // have verified.
        assert_eq!(some_count, verified_count);
    }

    #[test]
    fn test_write_optionl_fields_raw_format() {
        write_optional_fields(AccountBlockFormat::AlignedRaw);
    }

    #[test]
    fn test_write_optional_fields_lz4_format() {
        write_optional_fields(AccountBlockFormat::Lz4);
    }
}
