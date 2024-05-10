pub mod auth_encryption;
pub mod elgamal;
pub mod grouped_elgamal;
pub mod pedersen;

macro_rules! impl_from_str {
    (TYPE = $type:ident, BYTES_LEN = $bytes_len:expr, BASE64_LEN = $base64_len:expr) => {
        impl std::str::FromStr for $type {
            type Err = crate::errors::ParseError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                if s.len() > $base64_len {
                    return Err(Self::Err::WrongSize);
                }
                let mut bytes = [0u8; $bytes_len];
                let decoded_len = BASE64_STANDARD
                    .decode_slice(s, &mut bytes)
                    .map_err(|_| Self::Err::Invalid)?;
                if decoded_len != $bytes_len {
                    Err(Self::Err::WrongSize)
                } else {
                    Ok($type(bytes))
                }
            }
        }
    };
}
pub(crate) use impl_from_str;
