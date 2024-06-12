//! File i/o helper functions.
#[cfg(unix)]
use std::os::unix::prelude::FileExt;
use std::{fs::File, ops::Range};

/// `buffer` contains `valid_bytes` of data at its end.
/// Move those valid bytes to the beginning of `buffer`, then read from `offset` to fill the rest of `buffer`.
/// Update `offset` for the next read and update `valid_bytes` to specify valid portion of `buffer`.
/// `valid_file_len` is # of valid bytes in the file. This may be <= file length.
pub fn read_more_buffer(
    file: &File,
    valid_file_len: usize,
    offset: &mut usize,
    buffer: &mut [u8],
    valid_bytes: &mut Range<usize>,
) -> std::io::Result<()> {
    // copy remainder of `valid_bytes` into beginning of `buffer`.
    // These bytes were left over from the last read but weren't enough for the next desired contiguous read chunk.
    buffer.copy_within(valid_bytes.clone(), 0);

    // read the rest of `buffer`
    let bytes_read = read_into_buffer(
        file,
        valid_file_len,
        *offset,
        &mut buffer[valid_bytes.len()..],
    )?;
    *offset += bytes_read;
    *valid_bytes = 0..(valid_bytes.len() + bytes_read);

    Ok(())
}

#[cfg(unix)]
/// Read, starting at `start_offset`, until `buffer` is full or we read past `valid_file_len`/eof.
/// `valid_file_len` is # of valid bytes in the file. This may be <= file length.
/// return # bytes read
pub fn read_into_buffer(
    file: &File,
    valid_file_len: usize,
    start_offset: usize,
    buffer: &mut [u8],
) -> std::io::Result<usize> {
    let mut offset = start_offset;
    let mut buffer_offset = 0;
    let mut total_bytes_read = 0;
    if start_offset >= valid_file_len {
        return Ok(0);
    }

    while buffer_offset < buffer.len() {
        match file.read_at(&mut buffer[buffer_offset..], offset as u64) {
            Err(err) => {
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            Ok(bytes_read_this_time) => {
                total_bytes_read += bytes_read_this_time;
                if total_bytes_read + start_offset >= valid_file_len {
                    total_bytes_read -= (total_bytes_read + start_offset) - valid_file_len;
                    // we've read all there is in the file
                    break;
                }
                // There is possibly more to read. `read_at` may have returned partial results, so prepare to loop and read again.
                buffer_offset += bytes_read_this_time;
                offset += bytes_read_this_time;
            }
        }
    }
    Ok(total_bytes_read)
}

#[cfg(not(unix))]
/// this cannot be supported if we're not on unix-os
pub fn read_into_buffer(
    _file: &File,
    _valid_file_len: usize,
    _start_offset: usize,
    _buffer: &mut [u8],
) -> std::io::Result<usize> {
    panic!("unimplemented");
}

#[cfg(all(unix, test))]
mod tests {

    use {super::*, std::io::Write, tempfile::tempfile};

    #[test]
    fn test_read_into_buffer() {
        // Setup a sample file with 32 bytes of data
        let mut sample_file = tempfile().unwrap();
        let bytes: Vec<u8> = (0..32).collect();
        sample_file.write_all(&bytes).unwrap();

        // Read all 32 bytes into buffer
        let mut buffer = [0; 32];
        let num_bytes_read = read_into_buffer(&sample_file, 32, 0, &mut buffer).unwrap();
        assert_eq!(num_bytes_read, 32);
        assert_eq!(bytes, buffer);

        // Given a 64-byte buffer, it should only read 32 bytes into the buffer
        let mut buffer = [0; 64];
        let num_bytes_read = read_into_buffer(&sample_file, 32, 0, &mut buffer).unwrap();
        assert_eq!(num_bytes_read, 32);
        assert_eq!(bytes, buffer[0..32]);
        assert_eq!(buffer[32..64], [0; 32]);

        // Given the `valid_file_len` is 16, it should only read 16 bytes into the buffer
        let mut buffer = [0; 32];
        let num_bytes_read = read_into_buffer(&sample_file, 16, 0, &mut buffer).unwrap();
        assert_eq!(num_bytes_read, 16);
        assert_eq!(bytes[0..16], buffer[0..16]);
        // As a side effect of the `read_into_buffer` the data passed `valid_file_len` was
        // read and put into the buffer, though these data should not be
        // consumed.
        assert_eq!(buffer[16..32], bytes[16..32]);

        // Given the start offset 8, it should only read 24 bytes into buffer
        let mut buffer = [0; 32];
        let num_bytes_read = read_into_buffer(&sample_file, 32, 8, &mut buffer).unwrap();
        assert_eq!(num_bytes_read, 24);
        assert_eq!(buffer[0..24], bytes[8..32]);
        assert_eq!(buffer[24..32], [0; 8])
    }

    #[test]
    fn test_read_more_buffer() {
        // Setup a sample file with 32 bytes of data
        let mut sample_file = tempfile().unwrap();
        let bytes: Vec<u8> = (0..32).collect();
        sample_file.write_all(&bytes).unwrap();

        // Should move left-over 8 bytes to and read 24 bytes from file
        let mut buffer = [0xFFu8; 32];
        let mut offset = 0;
        let mut valid_bytes = 24..32;
        read_more_buffer(&sample_file, 32, &mut offset, &mut buffer, &mut valid_bytes).unwrap();
        assert_eq!(offset, 24);
        assert_eq!(valid_bytes, 0..32);
        assert_eq!(buffer[0..8], [0xFFu8; 8]);
        assert_eq!(buffer[8..32], bytes[0..24]);

        // Should move left-over 8 bytes to and read 16 bytes from file due to EOF
        let mut buffer = [0xFFu8; 32];
        let mut offset = 16;
        let mut valid_bytes = 24..32;
        read_more_buffer(&sample_file, 32, &mut offset, &mut buffer, &mut valid_bytes).unwrap();
        assert_eq!(offset, 32);
        assert_eq!(valid_bytes, 0..24);
        assert_eq!(buffer[0..8], [0xFFu8; 8]);
        assert_eq!(buffer[8..24], bytes[16..32]);
    }
}
