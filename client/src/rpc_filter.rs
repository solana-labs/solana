use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RpcFilterType {
    DataSize(u64),
    Memcmp(Memcmp),
}

impl RpcFilterType {
    pub fn verify(&self) -> Result<(), RpcFilterError> {
        match self {
            RpcFilterType::DataSize(_) => Ok(()),
            RpcFilterType::Memcmp(compare) => {
                let encoding = compare.encoding.as_ref().unwrap_or(&MemcmpEncoding::Binary);
                match encoding {
                    MemcmpEncoding::Binary => {
                        let MemcmpEncodedBytes::Binary(bytes) = &compare.bytes;

                        if bytes.len() > 128 {
                            Err(RpcFilterError::Base58DataTooLarge)
                        } else {
                            bs58::decode(&bytes)
                                .into_vec()
                                .map(|_| ())
                                .map_err(|e| e.into())
                        }
                    }
                }
            }
        }
    }
}

#[derive(Error, PartialEq, Debug)]
pub enum RpcFilterError {
    #[error("bs58 decode error")]
    DecodeError(#[from] bs58::decode::Error),
    #[error("encoded binary (base 58) data should be less than 129 bytes")]
    Base58DataTooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemcmpEncoding {
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum MemcmpEncodedBytes {
    Binary(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Memcmp {
    /// Data offset to begin match
    pub offset: usize,
    /// Bytes, encoded with specified encoding, or default Binary
    pub bytes: MemcmpEncodedBytes,
    /// Optional encoding specification
    pub encoding: Option<MemcmpEncoding>,
}

impl Memcmp {
    pub fn bytes_match(&self, data: &[u8]) -> bool {
        match &self.bytes {
            MemcmpEncodedBytes::Binary(bytes) => {
                let bytes = bs58::decode(bytes).into_vec();
                if bytes.is_err() {
                    return false;
                }
                let bytes = bytes.unwrap();
                if self.offset > data.len() {
                    return false;
                }
                if data[self.offset..].len() < bytes.len() {
                    return false;
                }
                data[self.offset..self.offset + bytes.len()] == bytes[..]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes_match() {
        let data = vec![1, 2, 3, 4, 5];

        // Exact match of data succeeds
        assert!(Memcmp {
            offset: 0,
            bytes: MemcmpEncodedBytes::Binary(bs58::encode(vec![1, 2, 3, 4, 5]).into_string()),
            encoding: None,
        }
        .bytes_match(&data));

        // Partial match of data succeeds
        assert!(Memcmp {
            offset: 0,
            bytes: MemcmpEncodedBytes::Binary(bs58::encode(vec![1, 2]).into_string()),
            encoding: None,
        }
        .bytes_match(&data));

        // Offset partial match of data succeeds
        assert!(Memcmp {
            offset: 2,
            bytes: MemcmpEncodedBytes::Binary(bs58::encode(vec![3, 4]).into_string()),
            encoding: None,
        }
        .bytes_match(&data));

        // Incorrect partial match of data fails
        assert!(!Memcmp {
            offset: 0,
            bytes: MemcmpEncodedBytes::Binary(bs58::encode(vec![2]).into_string()),
            encoding: None,
        }
        .bytes_match(&data));

        // Bytes overrun data fails
        assert!(!Memcmp {
            offset: 2,
            bytes: MemcmpEncodedBytes::Binary(bs58::encode(vec![3, 4, 5, 6]).into_string()),
            encoding: None,
        }
        .bytes_match(&data));

        // Offset outside data fails
        assert!(!Memcmp {
            offset: 6,
            bytes: MemcmpEncodedBytes::Binary(bs58::encode(vec![5]).into_string()),
            encoding: None,
        }
        .bytes_match(&data));

        // Invalid base-58 fails
        assert!(!Memcmp {
            offset: 0,
            bytes: MemcmpEncodedBytes::Binary("III".to_string()),
            encoding: None,
        }
        .bytes_match(&data));
    }

    #[test]
    fn test_verify_memcmp() {
        let base58_bytes = "\
            1111111111111111111111111111111111111111111111111111111111111111\
            1111111111111111111111111111111111111111111111111111111111111111";
        assert_eq!(base58_bytes.len(), 128);
        assert_eq!(
            RpcFilterType::Memcmp(Memcmp {
                offset: 0,
                bytes: MemcmpEncodedBytes::Binary(base58_bytes.to_string()),
                encoding: None,
            })
            .verify(),
            Ok(())
        );

        let base58_bytes = "\
            1111111111111111111111111111111111111111111111111111111111111111\
            1111111111111111111111111111111111111111111111111111111111111111\
            1";
        assert_eq!(base58_bytes.len(), 129);
        assert_eq!(
            RpcFilterType::Memcmp(Memcmp {
                offset: 0,
                bytes: MemcmpEncodedBytes::Binary(base58_bytes.to_string()),
                encoding: None,
            })
            .verify(),
            Err(RpcFilterError::Base58DataTooLarge)
        );
    }
}
