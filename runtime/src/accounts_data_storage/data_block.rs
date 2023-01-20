use {
    serde::{Deserialize, Serialize},
    std::{
        io::{Cursor, Error, Read, Write},
        mem,
    },
};

#[repr(u64)]
#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    PartialEq,
    Deserialize,
    num_enum::IntoPrimitive,
    Serialize,
    num_enum::TryFromPrimitive,
)]
#[serde(into = "u64", try_from = "u64")]
pub enum AccountDataBlockFormat {
    AlignedRaw = 0u64,
    Lz4 = 1u64,
}

enum AccountDataBlockEncoder {
    Raw(Cursor<Vec<u8>>),
    Lz4(lz4::Encoder<Vec<u8>>),
}

pub struct AccountDataBlockWriter {
    len: usize,
    encoder: AccountDataBlockEncoder,
}

impl AccountDataBlockWriter {
    pub fn new(encoding: AccountDataBlockFormat) -> Self {
        Self {
            len: 0,
            encoder: match encoding {
                AccountDataBlockFormat::AlignedRaw => {
                    AccountDataBlockEncoder::Raw(Cursor::new(Vec::new()))
                }
                AccountDataBlockFormat::Lz4 => AccountDataBlockEncoder::Lz4(
                    lz4::EncoderBuilder::new()
                        .level(1)
                        .build(Vec::new())
                        .unwrap(),
                ),
            },
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn write_type<T>(&mut self, value: &T) -> Result<usize, Error> {
        let len = mem::size_of::<T>();
        unsafe {
            let ptr = std::slice::from_raw_parts((value as *const T) as *const u8, len);
            // self.len has been updated here
            self.write(ptr, len)?;
        }
        Ok(len)
    }

    pub fn write(&mut self, buf: &[u8], len: usize) -> Result<(), Error> {
        let result = match &mut self.encoder {
            AccountDataBlockEncoder::Raw(cursor) => cursor.write_all(&buf[0..len]),
            AccountDataBlockEncoder::Lz4(lz4_encoder) => lz4_encoder.write_all(&buf[0..len]),
        };
        if result.is_ok() {
            self.len += len;
        }
        result
    }

    pub fn finish(self) -> Result<(Vec<u8>, usize), Error> {
        match self.encoder {
            AccountDataBlockEncoder::Raw(cursor) => Ok((cursor.into_inner(), self.len)),
            AccountDataBlockEncoder::Lz4(lz4_encoder) => {
                let (compressed_block, result) = lz4_encoder.finish();
                result?;
                Ok((compressed_block, self.len))
            }
        }
    }
}

pub struct AccountDataBlock {}

impl AccountDataBlock {
    pub fn decode(encoding: AccountDataBlockFormat, input: &[u8]) -> Result<Vec<u8>, Error> {
        match encoding {
            AccountDataBlockFormat::Lz4 => {
                let mut decoder = lz4::Decoder::new(input).unwrap();
                let mut output: Vec<u8> = vec![];
                decoder.read_to_end(&mut output)?;
                Ok(output)
            }
            AccountDataBlockFormat::AlignedRaw => panic!("Not implemented"),
        }
    }
}

/*
   #[test]
   fn test_compress_and_decompress() {
       let path = get_append_vec_path("test_compress_and_decompress");

       const DATA_SIZE: usize = 10 * 1024 * 1024;
       let mut expected_data: Vec<u8> = Vec::with_capacity(DATA_SIZE);
       let compressed_len_full;
       let compressed_len_partial;
       let mut byte = rand::random::<u8>();
       for i in 0..DATA_SIZE {
           if i % 64 == 0 {
               byte = rand::random::<u8>();
           }
           expected_data.push(byte);
       }

       {
           let ads = AccountsDataStorage::new_for_test(&path.path, true);
           compressed_len_full = ads
               .compress_and_write(&expected_data, expected_data.len())
               .unwrap();
           compressed_len_partial = ads
               .compress_and_write(&expected_data, expected_data.len() / 2)
               .unwrap();
       }
       // Make sure we do compress data
       assert!(compressed_len_full > 0);
       assert!(DATA_SIZE / 2 > compressed_len_full);

       let ads = AccountsDataStorage::new_for_test(&path.path, false);

       // full data
       let mut data = ads.decompress_and_read(compressed_len_full).unwrap();
       assert_eq!(expected_data, data);

       // partial data
       data = ads.decompress_and_read(compressed_len_partial).unwrap();
       assert_eq!(data.len(), expected_data.len() / 2);
       assert_eq!(&expected_data[0..expected_data.len() / 2], data);
   }

*/
