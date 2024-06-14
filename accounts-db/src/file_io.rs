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
        let file_size = 32;
        let bytes: Vec<u8> = (0..file_size as u8).collect();
        sample_file.write_all(&bytes).unwrap();

        // Read all 32 bytes into buffer
        let mut buffer = [0; 32];
        let mut buffer_len = buffer.len();
        let mut valid_len = 32;
        let mut start_offset = 0;
        let num_bytes_read =
            read_into_buffer(&sample_file, valid_len, start_offset, &mut buffer).unwrap();
        assert_eq!(num_bytes_read, buffer_len);
        assert_eq!(bytes, buffer);

        // Given a 64-byte buffer, it should only read 32 bytes into the buffer
        let mut buffer = [0; 64];
        buffer_len = buffer.len();
        let num_bytes_read =
            read_into_buffer(&sample_file, valid_len, start_offset, &mut buffer).unwrap();
        assert_eq!(num_bytes_read, valid_len);
        assert_eq!(bytes, buffer[0..valid_len]);
        assert_eq!(buffer[valid_len..buffer_len], [0; 32]);

        // Given the `valid_file_len` is 16, it should only read 16 bytes into the buffer
        let mut buffer = [0; 32];
        buffer_len = buffer.len();
        valid_len = 16;
        let num_bytes_read =
            read_into_buffer(&sample_file, valid_len, start_offset, &mut buffer).unwrap();
        assert_eq!(num_bytes_read, valid_len);
        assert_eq!(bytes[0..valid_len], buffer[0..valid_len]);
        // As a side effect of the `read_into_buffer` the data passed `valid_file_len` was
        // read and put into the buffer, though these data should not be
        // consumed.
        assert_eq!(buffer[valid_len..buffer_len], bytes[valid_len..buffer_len]);

        // Given the start offset 8, it should only read 24 bytes into buffer
        let mut buffer = [0; 32];
        buffer_len = buffer.len();
        valid_len = 32;
        start_offset = 8;
        let num_bytes_read =
            read_into_buffer(&sample_file, valid_len, start_offset, &mut buffer).unwrap();
        assert_eq!(num_bytes_read, valid_len - start_offset);
        assert_eq!(buffer[0..num_bytes_read], bytes[start_offset..buffer_len]);
        assert_eq!(buffer[num_bytes_read..buffer_len], [0; 8])
    }

    #[test]
    fn test_read_more_buffer() {
        // Setup a sample file with 32 bytes of data
        let mut sample_file = tempfile().unwrap();
        let file_size = 32;
        let bytes: Vec<u8> = (0..file_size as u8).collect();
        sample_file.write_all(&bytes).unwrap();

        // Should move left-over 8 bytes to and read 24 bytes from file
        let mut buffer = [0xFFu8; 32];
        let buffer_len = buffer.len();
        let mut offset = 0;
        let mut valid_bytes = 24..32;
        let mut valid_bytes_len = valid_bytes.len();
        let valid_len = 32;
        read_more_buffer(
            &sample_file,
            valid_len,
            &mut offset,
            &mut buffer,
            &mut valid_bytes,
        )
        .unwrap();
        assert_eq!(offset, buffer_len - valid_bytes_len);
        assert_eq!(valid_bytes, 0..buffer_len);
        assert_eq!(buffer[0..valid_bytes_len], [0xFFu8; 8]);
        assert_eq!(
            buffer[valid_bytes_len..buffer_len],
            bytes[0..buffer_len - valid_bytes_len]
        );

        // Should move left-over 8 bytes to and read 16 bytes from file due to EOF
        let mut buffer = [0xFFu8; 32];
        let start_offset = 16;
        let mut offset = 16;
        let mut valid_bytes = 24..32;
        valid_bytes_len = valid_bytes.len();
        read_more_buffer(
            &sample_file,
            valid_len,
            &mut offset,
            &mut buffer,
            &mut valid_bytes,
        )
        .unwrap();
        assert_eq!(offset, file_size);
        assert_eq!(valid_bytes, 0..24);
        assert_eq!(buffer[0..valid_bytes_len], [0xFFu8; 8]);
        assert_eq!(
            buffer[valid_bytes_len..valid_bytes.end],
            bytes[start_offset..file_size]
        );
    }
}
