use crate::result::{Result, TransactionParsingError};

/// Check that the buffer has at least `len` bytes remaining starting at
/// `offset`. Returns Err if the buffer is too short.
///
/// Assumptions:
/// - The current offset is not greater than `bytes.len()`.
#[inline(always)]
pub fn check_remaining(bytes: &[u8], offset: usize, len: usize) -> Result<()> {
    if len > bytes.len().wrapping_sub(offset) {
        Err(TransactionParsingError)
    } else {
        Ok(())
    }
}

/// Check that the buffer has at least 1 byte remaining starting at `offset`.
/// Returns Err if the buffer is too short.
#[inline(always)]
pub fn read_byte(bytes: &[u8], offset: &mut usize) -> Result<u8> {
    // Implicitly checks that the offset is within bounds, no need
    // to call `check_remaining` explicitly here.
    let value = bytes.get(*offset).copied().ok_or(TransactionParsingError);
    *offset = offset.wrapping_add(1);
    value
}

/// Read a compressed u16 from `bytes` starting at `offset`.
/// If the buffer is too short or the encoding is invalid, return Err.
/// `offset` is updated to point to the byte after the compressed u16.
///
/// Assumptions:
/// - The current offset is not greater than `bytes.len()`.
#[inline(always)]
pub fn read_compressed_u16(bytes: &[u8], offset: &mut usize) -> Result<u16> {
    let mut result = 0u16;
    let mut shift = 0u16;

    for i in 0..3 {
        // Implicitly checks that the offset is within bounds, no need
        // to call check_remaining explicitly here.
        let byte = *bytes
            .get(offset.wrapping_add(i))
            .ok_or(TransactionParsingError)?;
        // non-minimal encoding or overflow
        if (i > 0 && byte == 0) || (i == 2 && byte > 3) {
            return Err(TransactionParsingError);
        }
        result |= ((byte & 0x7F) as u16) << shift;
        shift = shift.wrapping_add(7);
        if byte & 0x80 == 0 {
            *offset = offset.wrapping_add(i).wrapping_add(1);
            return Ok(result);
        }
    }

    // if we reach here, it means that all 3 bytes were used
    *offset = offset.wrapping_add(3);
    Ok(result)
}

/// Domain-specific optimization for reading a compressed u16.
/// The compressed u16's are only used for array-lengths in our transaction
/// format. The transaction packet has a maximum size of 1232 bytes.
/// This means that the maximum array length within a **valid** transaction is
/// 1232. This has a minimally encoded length of 2 bytes.
/// Although the encoding scheme allows for more, any arrays with this length
/// would be too large to fit in a packet. This function optimizes for this
/// case, and reads a maximum of 2 bytes.
/// If the buffer is too short or the encoding is invalid, return Err.
/// `offset` is updated to point to the byte after the compressed u16.
#[inline(always)]
pub fn optimized_read_compressed_u16(bytes: &[u8], offset: &mut usize) -> Result<u16> {
    let mut result = 0u16;

    // First byte
    let byte1 = *bytes.get(*offset).ok_or(TransactionParsingError)?;
    result |= (byte1 & 0x7F) as u16;
    if byte1 & 0x80 == 0 {
        *offset = offset.wrapping_add(1);
        return Ok(result);
    }

    // Second byte
    let byte2 = *bytes
        .get(offset.wrapping_add(1))
        .ok_or(TransactionParsingError)?;
    if byte2 == 0 || byte2 & 0x80 != 0 {
        return Err(TransactionParsingError); // non-minimal encoding or overflow
    }
    result |= ((byte2 & 0x7F) as u16) << 7;
    *offset = offset.wrapping_add(2);

    Ok(result)
}

/// Update the `offset` to point to the byte after an array of length `len` and
/// of type `T`. If the buffer is too short, return Err.
///
/// Assumptions:
/// 1. The current offset is not greater than `bytes.len()`.
/// 2. The size of `T` is small enough such that a usize will not overflow if
///    given the maximum array size (u16::MAX).
#[inline(always)]
pub fn offset_array_len<T: Sized>(bytes: &[u8], offset: &mut usize, len: u16) -> Result<()> {
    let array_len_bytes = usize::from(len).wrapping_mul(core::mem::size_of::<T>());
    check_remaining(bytes, *offset, array_len_bytes)?;
    *offset = offset.wrapping_add(array_len_bytes);
    Ok(())
}

/// Update the `offset` to point t the byte after the `T`.
/// If the buffer is too short, return Err.
///
/// Assumptions:
/// 1. The current offset is not greater than `bytes.len()`.
/// 2. The size of `T` is small enough such that a usize will not overflow.
#[inline(always)]
pub fn offset_type<T: Sized>(bytes: &[u8], offset: &mut usize) -> Result<()> {
    let type_size = core::mem::size_of::<T>();
    check_remaining(bytes, *offset, type_size)?;
    *offset = offset.wrapping_add(type_size);
    Ok(())
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        bincode::{serialize_into, DefaultOptions, Options},
        solana_sdk::{packet::PACKET_DATA_SIZE, short_vec::ShortU16},
    };

    #[test]
    fn test_check_remaining() {
        // Empty buffer checks
        assert!(check_remaining(&[], 0, 0).is_ok());
        assert!(check_remaining(&[], 0, 1).is_err());

        // Buffer with data checks
        assert!(check_remaining(&[1, 2, 3], 0, 0).is_ok());
        assert!(check_remaining(&[1, 2, 3], 0, 1).is_ok());
        assert!(check_remaining(&[1, 2, 3], 0, 3).is_ok());
        assert!(check_remaining(&[1, 2, 3], 0, 4).is_err());

        // Non-zero offset.
        assert!(check_remaining(&[1, 2, 3], 1, 0).is_ok());
        assert!(check_remaining(&[1, 2, 3], 1, 1).is_ok());
        assert!(check_remaining(&[1, 2, 3], 1, 2).is_ok());
        assert!(check_remaining(&[1, 2, 3], 1, usize::MAX).is_err());
    }

    #[test]
    fn test_read_byte() {
        let bytes = [5, 6, 7];
        let mut offset = 0;
        assert_eq!(read_byte(&bytes, &mut offset), Ok(5));
        assert_eq!(offset, 1);
        assert_eq!(read_byte(&bytes, &mut offset), Ok(6));
        assert_eq!(offset, 2);
        assert_eq!(read_byte(&bytes, &mut offset), Ok(7));
        assert_eq!(offset, 3);
        assert!(read_byte(&bytes, &mut offset).is_err());
    }

    #[test]
    fn test_read_compressed_u16() {
        let mut buffer = [0u8; 1024];
        let options = DefaultOptions::new().with_fixint_encoding(); // Ensure fixed-int encoding

        // Test all possible u16 values
        for value in 0..=u16::MAX {
            let mut offset;
            let short_u16 = ShortU16(value);

            // Serialize the value into the buffer
            serialize_into(&mut buffer[..], &short_u16).expect("Serialization failed");

            // Use bincode's size calculation to determine the length of the serialized data
            let serialized_len = options
                .serialized_size(&short_u16)
                .expect("Failed to get serialized size");

            // Reset offset
            offset = 0;

            // Read the value back using unchecked_read_u16_compressed
            let read_value = read_compressed_u16(&buffer, &mut offset);

            // Assert that the read value matches the original value
            assert_eq!(read_value, Ok(value), "Value mismatch for: {}", value);

            // Assert that the offset matches the serialized length
            assert_eq!(
                offset, serialized_len as usize,
                "Offset mismatch for: {}",
                value
            );
        }

        // Test bounds.
        // All 0s => 0
        assert_eq!(Ok(0), read_compressed_u16(&[0; 3], &mut 0));
        // Overflow
        assert!(read_compressed_u16(&[0xFF, 0xFF, 0x04], &mut 0).is_err());
        assert_eq!(
            read_compressed_u16(&[0xFF, 0xFF, 0x03], &mut 0),
            Ok(u16::MAX)
        );

        // overflow errors
        assert!(read_compressed_u16(&[u8::MAX; 1], &mut 0).is_err());
        assert!(read_compressed_u16(&[u8::MAX; 2], &mut 0).is_err());

        // Minimal encoding checks
        assert!(read_compressed_u16(&[0x81, 0x80, 0x00], &mut 0).is_err());
    }

    #[test]
    fn test_optimized_read_compressed_u16() {
        let mut buffer = [0u8; 1024];
        let options = DefaultOptions::new().with_fixint_encoding(); // Ensure fixed-int encoding

        // Test all possible u16 values under the packet length
        for value in 0..=PACKET_DATA_SIZE as u16 {
            let mut offset;
            let short_u16 = ShortU16(value);

            // Serialize the value into the buffer
            serialize_into(&mut buffer[..], &short_u16).expect("Serialization failed");

            // Use bincode's size calculation to determine the length of the serialized data
            let serialized_len = options
                .serialized_size(&short_u16)
                .expect("Failed to get serialized size");

            // Reset offset
            offset = 0;

            // Read the value back using unchecked_read_u16_compressed
            let read_value = optimized_read_compressed_u16(&buffer, &mut offset);

            // Assert that the read value matches the original value
            assert_eq!(read_value, Ok(value), "Value mismatch for: {}", value);

            // Assert that the offset matches the serialized length
            assert_eq!(
                offset, serialized_len as usize,
                "Offset mismatch for: {}",
                value
            );
        }

        // Test bounds.
        // All 0s => 0
        assert_eq!(Ok(0), optimized_read_compressed_u16(&[0; 3], &mut 0));
        // Overflow
        assert!(optimized_read_compressed_u16(&[0xFF, 0xFF, 0x04], &mut 0).is_err());
        assert!(optimized_read_compressed_u16(&[0xFF, 0x80], &mut 0).is_err());

        // overflow errors
        assert!(optimized_read_compressed_u16(&[u8::MAX; 1], &mut 0).is_err());
        assert!(optimized_read_compressed_u16(&[u8::MAX; 2], &mut 0).is_err());

        // Minimal encoding checks
        assert!(optimized_read_compressed_u16(&[0x81, 0x00], &mut 0).is_err());
    }

    #[test]
    fn test_offset_array_len() {
        #[repr(C)]
        struct MyStruct {
            _a: u8,
            _b: u8,
        }
        const _: () = assert!(core::mem::size_of::<MyStruct>() == 2);

        // Test with a buffer that is too short
        let bytes = [0u8; 1];
        let mut offset = 0;
        assert!(offset_array_len::<MyStruct>(&bytes, &mut offset, 1).is_err());

        // Test with a buffer that is long enough
        let bytes = [0u8; 4];
        let mut offset = 0;
        assert!(offset_array_len::<MyStruct>(&bytes, &mut offset, 2).is_ok());
        assert_eq!(offset, 4);
    }

    #[test]
    fn test_offset_type() {
        #[repr(C)]
        struct MyStruct {
            _a: u8,
            _b: u8,
        }
        const _: () = assert!(core::mem::size_of::<MyStruct>() == 2);

        // Test with a buffer that is too short
        let bytes = [0u8; 1];
        let mut offset = 0;
        assert!(offset_type::<MyStruct>(&bytes, &mut offset).is_err());

        // Test with a buffer that is long enough
        let bytes = [0u8; 4];
        let mut offset = 0;
        assert!(offset_type::<MyStruct>(&bytes, &mut offset).is_ok());
        assert_eq!(offset, 2);
    }
}
