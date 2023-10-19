/// Check if given bytes contain only printable ASCII characters
pub fn is_printable_ascii(data: &[u8]) -> bool {
    for &char in data {
        if !(0x20..=0x7e).contains(&char) {
            return false;
        }
    }
    true
}

/// Check if given bytes contain valid UTF8 string
pub fn is_utf8(data: &[u8]) -> bool {
    std::str::from_utf8(data).is_ok()
}


#[cfg(test)]
mod tests {
    use crate::string_utils::is_utf8;
    use crate::string_utils::is_printable_ascii;

    #[test]
    fn test_is_utf8(){
        let data = "hello world".as_bytes();
        assert!(is_utf8(data));
    }

    #[test]
    fn test_is_utf8_invalid_sequence_identifier(){
        let data: [u8; 2] = [0xA0, 0xA1];
        assert!(!is_utf8(data.as_ref()));
    }

    #[test]
    fn test_is_utf8_special_chars(){
        let data = "żółty".as_bytes();
        assert!(is_utf8(data));
    }

    #[test]
    fn test_is_printable_ascii(){
        let data = "hello world".as_bytes();
        assert!(is_printable_ascii(data));
    }

    #[test]
    fn test_is_printable_ascii_empty(){
        let data = b"";
        assert!(is_printable_ascii(data));
    }

    #[test]
    fn test_is_utf8_empty(){
        let data = b"";
        assert!(is_utf8(data));
    }

    #[test]
    fn test_is_utf8_invalid(){
        let mut data = Vec::from([0xc3, 0x28]);
        data.resize(32,0);
        assert!(!is_utf8(data.as_ref()));
    }


}