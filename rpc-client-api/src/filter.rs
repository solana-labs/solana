#![allow(deprecated)]
use {
    crate::version_req::VersionReq,
    solana_sdk::account::{AccountSharedData, ReadableAccount},
    spl_token_2022::{generic_token_account::GenericTokenAccount, state::Account},
    std::borrow::Cow,
    thiserror::Error,
};

const MAX_DATA_SIZE: usize = 128;
const MAX_DATA_BASE58_SIZE: usize = 175;
const MAX_DATA_BASE64_SIZE: usize = 172;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RpcFilterType {
    DataSize(u64),
    Memcmp(Memcmp),
    TokenAccountState,
}

impl RpcFilterType {
    pub fn verify(&self) -> Result<(), RpcFilterError> {
        match self {
            RpcFilterType::DataSize(_) => Ok(()),
            RpcFilterType::Memcmp(compare) => {
                let encoding = compare.encoding.as_ref().unwrap_or(&MemcmpEncoding::Binary);
                match encoding {
                    MemcmpEncoding::Binary => {
                        use MemcmpEncodedBytes::*;
                        match &compare.bytes {
                            // DEPRECATED
                            Binary(bytes) => {
                                if bytes.len() > MAX_DATA_BASE58_SIZE {
                                    return Err(RpcFilterError::Base58DataTooLarge);
                                }
                                let bytes = bs58::decode(&bytes)
                                    .into_vec()
                                    .map_err(RpcFilterError::DecodeError)?;
                                if bytes.len() > MAX_DATA_SIZE {
                                    Err(RpcFilterError::Base58DataTooLarge)
                                } else {
                                    Ok(())
                                }
                            }
                            Base58(bytes) => {
                                if bytes.len() > MAX_DATA_BASE58_SIZE {
                                    return Err(RpcFilterError::DataTooLarge);
                                }
                                let bytes = bs58::decode(&bytes).into_vec()?;
                                if bytes.len() > MAX_DATA_SIZE {
                                    Err(RpcFilterError::DataTooLarge)
                                } else {
                                    Ok(())
                                }
                            }
                            Base64(bytes) => {
                                if bytes.len() > MAX_DATA_BASE64_SIZE {
                                    return Err(RpcFilterError::DataTooLarge);
                                }
                                let bytes = base64::decode(bytes)?;
                                if bytes.len() > MAX_DATA_SIZE {
                                    Err(RpcFilterError::DataTooLarge)
                                } else {
                                    Ok(())
                                }
                            }
                            Bytes(bytes) => {
                                if bytes.len() > MAX_DATA_SIZE {
                                    return Err(RpcFilterError::DataTooLarge);
                                }
                                Ok(())
                            }
                        }
                    }
                }
            }
            RpcFilterType::TokenAccountState => Ok(()),
        }
    }

    pub fn allows(&self, account: &AccountSharedData) -> bool {
        match self {
            RpcFilterType::DataSize(size) => account.data().len() as u64 == *size,
            RpcFilterType::Memcmp(compare) => compare.bytes_match(account.data()),
            RpcFilterType::TokenAccountState => Account::valid_account_data(account.data()),
        }
    }
}

#[derive(Error, PartialEq, Eq, Debug)]
pub enum RpcFilterError {
    #[error("encoded binary data should be less than 129 bytes")]
    DataTooLarge,
    #[deprecated(
        since = "1.8.1",
        note = "Error for MemcmpEncodedBytes::Binary which is deprecated"
    )]
    #[error("encoded binary (base 58) data should be less than 129 bytes")]
    Base58DataTooLarge,
    #[deprecated(
        since = "1.8.1",
        note = "Error for MemcmpEncodedBytes::Binary which is deprecated"
    )]
    #[error("bs58 decode error")]
    DecodeError(bs58::decode::Error),
    #[error("base58 decode error")]
    Base58DecodeError(#[from] bs58::decode::Error),
    #[error("base64 decode error")]
    Base64DecodeError(#[from] base64::DecodeError),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemcmpEncoding {
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum MemcmpEncodedBytes {
    #[deprecated(
        since = "1.8.1",
        note = "Please use MemcmpEncodedBytes::Base58 instead"
    )]
    Binary(String),
    Base58(String),
    Base64(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "RpcMemcmp", from = "RpcMemcmp")]
pub struct Memcmp {
    /// Data offset to begin match
    #[deprecated(
        since = "1.15.0",
        note = "Field will be made private in future. Please use a constructor method instead."
    )]
    pub offset: usize,
    /// Bytes, encoded with specified encoding, or default Binary
    #[deprecated(
        since = "1.15.0",
        note = "Field will be made private in future. Please use a constructor method instead."
    )]
    pub bytes: MemcmpEncodedBytes,
    /// Optional encoding specification
    #[deprecated(
        since = "1.11.2",
        note = "Field has no server-side effect. Specify encoding with `MemcmpEncodedBytes` variant instead. \
            Field will be made private in future. Please use a constructor method instead."
    )]
    pub encoding: Option<MemcmpEncoding>,
}

impl Memcmp {
    pub fn new(offset: usize, encoded_bytes: MemcmpEncodedBytes) -> Self {
        Self {
            offset,
            bytes: encoded_bytes,
            encoding: None,
        }
    }

    pub fn new_raw_bytes(offset: usize, bytes: Vec<u8>) -> Self {
        Self {
            offset,
            bytes: MemcmpEncodedBytes::Bytes(bytes),
            encoding: None,
        }
    }

    pub fn new_base58_encoded(offset: usize, bytes: &[u8]) -> Self {
        Self {
            offset,
            bytes: MemcmpEncodedBytes::Base58(bs58::encode(bytes).into_string()),
            encoding: None,
        }
    }

    pub fn bytes(&self) -> Option<Cow<Vec<u8>>> {
        use MemcmpEncodedBytes::*;
        match &self.bytes {
            Binary(bytes) | Base58(bytes) => bs58::decode(bytes).into_vec().ok().map(Cow::Owned),
            Base64(bytes) => base64::decode(bytes).ok().map(Cow::Owned),
            Bytes(bytes) => Some(Cow::Borrowed(bytes)),
        }
    }

    pub fn convert_to_raw_bytes(&mut self) -> Result<(), RpcFilterError> {
        use MemcmpEncodedBytes::*;
        match &self.bytes {
            Binary(bytes) | Base58(bytes) => {
                let bytes = bs58::decode(bytes).into_vec()?;
                self.bytes = Bytes(bytes);
                Ok(())
            }
            Base64(bytes) => {
                let bytes = base64::decode(bytes)?;
                self.bytes = Bytes(bytes);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn bytes_match(&self, data: &[u8]) -> bool {
        match self.bytes() {
            Some(bytes) => {
                if self.offset > data.len() {
                    return false;
                }
                if data[self.offset..].len() < bytes.len() {
                    return false;
                }
                data[self.offset..self.offset + bytes.len()] == bytes[..]
            }
            None => false,
        }
    }
}

// Internal struct to hold Memcmp filter data as either encoded String or raw Bytes
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
enum DataType {
    Encoded(String),
    Raw(Vec<u8>),
}

// Internal struct used to specify explicit Base58 and Base64 encoding
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum RpcMemcmpEncoding {
    Base58,
    Base64,
    // This variant exists only to preserve backward compatibility with generic `Memcmp` serde
    #[serde(other)]
    Binary,
}

// Internal struct to enable Memcmp filters with explicit Base58 and Base64 encoding. The From
// implementations emulate `#[serde(tag = "encoding", content = "bytes")]` for
// `MemcmpEncodedBytes`. On the next major version, all these internal elements should be removed
// and replaced with adjacent tagging of `MemcmpEncodedBytes`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct RpcMemcmp {
    offset: usize,
    bytes: DataType,
    encoding: Option<RpcMemcmpEncoding>,
}

impl From<Memcmp> for RpcMemcmp {
    fn from(memcmp: Memcmp) -> RpcMemcmp {
        let (bytes, encoding) = match memcmp.bytes {
            MemcmpEncodedBytes::Binary(string) => {
                (DataType::Encoded(string), Some(RpcMemcmpEncoding::Binary))
            }
            MemcmpEncodedBytes::Base58(string) => {
                (DataType::Encoded(string), Some(RpcMemcmpEncoding::Base58))
            }
            MemcmpEncodedBytes::Base64(string) => {
                (DataType::Encoded(string), Some(RpcMemcmpEncoding::Base64))
            }
            MemcmpEncodedBytes::Bytes(vector) => (DataType::Raw(vector), None),
        };
        RpcMemcmp {
            offset: memcmp.offset,
            bytes,
            encoding,
        }
    }
}

impl From<RpcMemcmp> for Memcmp {
    fn from(memcmp: RpcMemcmp) -> Memcmp {
        let encoding = memcmp.encoding.unwrap_or(RpcMemcmpEncoding::Binary);
        let bytes = match (encoding, memcmp.bytes) {
            (RpcMemcmpEncoding::Binary, DataType::Encoded(string))
            | (RpcMemcmpEncoding::Base58, DataType::Encoded(string)) => {
                MemcmpEncodedBytes::Base58(string)
            }
            (RpcMemcmpEncoding::Binary, DataType::Raw(vector)) => MemcmpEncodedBytes::Bytes(vector),
            (RpcMemcmpEncoding::Base64, DataType::Encoded(string)) => {
                MemcmpEncodedBytes::Base64(string)
            }
            _ => unreachable!(),
        };
        Memcmp {
            offset: memcmp.offset,
            bytes,
            encoding: None,
        }
    }
}

pub fn maybe_map_filters(
    node_version: Option<semver::Version>,
    filters: &mut [RpcFilterType],
) -> Result<(), String> {
    let version_reqs = VersionReq::from_strs(&["<1.11.2", "~1.13"])?;
    let needs_mapping = node_version
        .map(|version| version_reqs.matches_any(&version))
        .unwrap_or(true);
    if needs_mapping {
        for filter in filters.iter_mut() {
            if let RpcFilterType::Memcmp(memcmp) = filter {
                match &memcmp.bytes {
                    MemcmpEncodedBytes::Base58(string) => {
                        memcmp.bytes = MemcmpEncodedBytes::Binary(string.clone());
                    }
                    MemcmpEncodedBytes::Base64(_) => {
                        return Err("RPC node on old version does not support base64 \
                            encoding for memcmp filters"
                            .to_string());
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worst_case_encoded_tx_goldens() {
        let ff_data = vec![0xffu8; MAX_DATA_SIZE];
        let data58 = bs58::encode(&ff_data).into_string();
        assert_eq!(data58.len(), MAX_DATA_BASE58_SIZE);
        let data64 = base64::encode(&ff_data);
        assert_eq!(data64.len(), MAX_DATA_BASE64_SIZE);
    }

    #[test]
    fn test_bytes_match() {
        let data = vec![1, 2, 3, 4, 5];

        // Exact match of data succeeds
        assert!(Memcmp {
            offset: 0,
            bytes: MemcmpEncodedBytes::Base58(bs58::encode(vec![1, 2, 3, 4, 5]).into_string()),
            encoding: None,
        }
        .bytes_match(&data));

        // Partial match of data succeeds
        assert!(Memcmp {
            offset: 0,
            bytes: MemcmpEncodedBytes::Base58(bs58::encode(vec![1, 2]).into_string()),
            encoding: None,
        }
        .bytes_match(&data));

        // Offset partial match of data succeeds
        assert!(Memcmp {
            offset: 2,
            bytes: MemcmpEncodedBytes::Base58(bs58::encode(vec![3, 4]).into_string()),
            encoding: None,
        }
        .bytes_match(&data));

        // Incorrect partial match of data fails
        assert!(!Memcmp {
            offset: 0,
            bytes: MemcmpEncodedBytes::Base58(bs58::encode(vec![2]).into_string()),
            encoding: None,
        }
        .bytes_match(&data));

        // Bytes overrun data fails
        assert!(!Memcmp {
            offset: 2,
            bytes: MemcmpEncodedBytes::Base58(bs58::encode(vec![3, 4, 5, 6]).into_string()),
            encoding: None,
        }
        .bytes_match(&data));

        // Offset outside data fails
        assert!(!Memcmp {
            offset: 6,
            bytes: MemcmpEncodedBytes::Base58(bs58::encode(vec![5]).into_string()),
            encoding: None,
        }
        .bytes_match(&data));

        // Invalid base-58 fails
        assert!(!Memcmp {
            offset: 0,
            bytes: MemcmpEncodedBytes::Base58("III".to_string()),
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
                bytes: MemcmpEncodedBytes::Base58(base58_bytes.to_string()),
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
                bytes: MemcmpEncodedBytes::Base58(base58_bytes.to_string()),
                encoding: None,
            })
            .verify(),
            Err(RpcFilterError::DataTooLarge)
        );
    }
}
