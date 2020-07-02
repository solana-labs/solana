use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RpcFilterType {
    CompareBytes(CompareBytes),
}

impl RpcFilterType {
    pub fn verify(&self) -> Result<(), RpcFilterError> {
        match self {
            RpcFilterType::CompareBytes(compare) => bs58::decode(&compare.bytes)
                .into_vec()
                .map(|_| ())
                .map_err(|e| e.into()),
        }
    }
}

#[derive(Error, Debug)]
pub enum RpcFilterError {
    #[error("bs58 decode error")]
    DecodeError(#[from] bs58::decode::Error),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompareBytes {
    /// Data offset to begin match
    offset: usize,
    /// Base-58 encoded bytes
    bytes: String,
}

impl CompareBytes {
    pub fn bytes_match(&self, data: &[u8]) -> bool {
        let bytes = bs58::decode(&self.bytes).into_vec();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes_match() {
        let data = vec![1, 2, 3, 4, 5];

        // Exact match of data succeeds
        assert!(CompareBytes {
            offset: 0,
            bytes: bs58::encode(vec![1, 2, 3, 4, 5]).into_string(),
        }
        .bytes_match(&data));

        // Partial match of data succeeds
        assert!(CompareBytes {
            offset: 0,
            bytes: bs58::encode(vec![1, 2]).into_string(),
        }
        .bytes_match(&data));

        // Offset partial match of data succeeds
        assert!(CompareBytes {
            offset: 2,
            bytes: bs58::encode(vec![3, 4]).into_string(),
        }
        .bytes_match(&data));

        // Incorrect partial match of data fails
        assert!(!CompareBytes {
            offset: 0,
            bytes: bs58::encode(vec![2]).into_string(),
        }
        .bytes_match(&data));

        // Bytes overrun data fails
        assert!(!CompareBytes {
            offset: 2,
            bytes: bs58::encode(vec![3, 4, 5, 6]).into_string(),
        }
        .bytes_match(&data));

        // Offset outside data fails
        assert!(!CompareBytes {
            offset: 6,
            bytes: bs58::encode(vec![5]).into_string(),
        }
        .bytes_match(&data));

        // Invalid base-58 fails
        assert!(!CompareBytes {
            offset: 0,
            bytes: "III".to_string(),
        }
        .bytes_match(&data));
    }
}
