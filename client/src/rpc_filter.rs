#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RpcFilterType {
    CompareBytes(CompareBytes),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompareBytes {
    offset: usize,
    bytes: Vec<u8>,
}

impl CompareBytes {
    pub fn bytes_match(&self, data: &[u8]) -> bool {
        if self.offset > data.len() {
            return false;
        }
        if data[self.offset..].len() < self.bytes.len() {
            return false;
        }
        data[self.offset..self.offset + self.bytes.len()] == self.bytes[..]
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
            bytes: vec![1, 2, 3, 4, 5],
        }
        .bytes_match(&data));

        // Partial match of data succeeds
        assert!(CompareBytes {
            offset: 0,
            bytes: vec![1, 2],
        }
        .bytes_match(&data));

        // Offset partial match of data succeeds
        assert!(CompareBytes {
            offset: 2,
            bytes: vec![3, 4],
        }
        .bytes_match(&data));

        // Incorrect partial match of data fails
        assert!(!CompareBytes {
            offset: 0,
            bytes: vec![2],
        }
        .bytes_match(&data));

        // Bytes overrun data fails
        assert!(!CompareBytes {
            offset: 2,
            bytes: vec![3, 4, 5, 6],
        }
        .bytes_match(&data));

        // Offset outside data fails
        assert!(!CompareBytes {
            offset: 6,
            bytes: vec![5],
        }
        .bytes_match(&data));
    }
}
