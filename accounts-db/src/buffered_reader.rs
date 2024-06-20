//! File I/O buffered reader for AppendVec
//! Specialized BufRead-like type for reading account data.
//!
//! Callers can use this type to iterate efficiently over append vecs. They can do so by repeatedly
//! calling read(), advance_offset() and set_required_data_len(account_data_len) once the next account
//! data length is known.
//!
//! Unlike BufRead/BufReader, this type guarantees that on the next read() after calling
//! set_required_data_len(len), the whole account data is buffered _linearly_ in memory and available to
//! be returned.
use {
    crate::{append_vec::ValidSlice, file_io::read_more_buffer},
    std::{fs::File, ops::Range},
};

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum BufferedReaderStatus {
    Eof,
    Success,
}

/// read a file a large buffer at a time and provide access to a slice in that buffer
pub struct BufferedReader<'a> {
    /// when we are next asked to read from file, start at this offset
    file_offset_of_next_read: usize,
    /// the most recently read data. `buf_valid_bytes` specifies the range of `buf` that is valid.
    buf: Box<[u8]>,
    /// specifies the range of `buf` that contains valid data that has not been used by the caller
    buf_valid_bytes: Range<usize>,
    /// offset in the file of the `buf_valid_bytes`.`start`
    file_last_offset: usize,
    /// how many contiguous bytes caller needs
    read_requirements: Option<usize>,
    /// how many bytes are valid in the file. The file's len may be longer.
    file_len_valid: usize,
    /// reference to file handle
    file: &'a File,
    /// we always want at least this many contiguous bytes available or we must read more into the buffer.
    default_min_read_requirement: usize,
}

impl<'a> BufferedReader<'a> {
    /// `buffer_size`: how much to try to read at a time
    /// `file_len_valid`: # bytes that are valid in the file, may be less than overall file len
    /// `default_min_read_requirement`: make sure we always have this much data available if we're asked to read
    pub fn new(
        buffer_size: usize,
        file_len_valid: usize,
        file: &'a File,
        default_min_read_requirement: usize,
    ) -> Self {
        let buffer_size = buffer_size.min(file_len_valid);
        Self {
            file_offset_of_next_read: 0,
            buf: vec![0; buffer_size].into_boxed_slice(),
            buf_valid_bytes: 0..0,
            file_last_offset: 0,
            read_requirements: None,
            file_len_valid,
            file,
            default_min_read_requirement,
        }
    }
    /// read to make sure we have the minimum amount of data
    pub fn read(&mut self) -> std::io::Result<BufferedReaderStatus> {
        let must_read = self
            .read_requirements
            .unwrap_or(self.default_min_read_requirement);
        if self.buf_valid_bytes.len() < must_read {
            // we haven't used all the bytes we read last time, so adjust the effective offset
            debug_assert!(self.buf_valid_bytes.len() <= self.file_offset_of_next_read);
            self.file_last_offset = self.file_offset_of_next_read - self.buf_valid_bytes.len();
            read_more_buffer(
                self.file,
                self.file_len_valid,
                &mut self.file_offset_of_next_read,
                &mut self.buf,
                &mut self.buf_valid_bytes,
            )?;
            if self.buf_valid_bytes.len() < must_read {
                return Ok(BufferedReaderStatus::Eof);
            }
        }
        // reset this once we have checked that we had this much data once
        self.read_requirements = None;
        Ok(BufferedReaderStatus::Success)
    }
    /// return the biggest slice of valid data starting at the current offset
    fn get_data(&'a self) -> ValidSlice<'a> {
        ValidSlice::new(&self.buf[self.buf_valid_bytes.clone()])
    }
    /// return offset within `file` of start of read at current offset
    pub fn get_offset_and_data(&'a self) -> (usize, ValidSlice<'a>) {
        (
            self.file_last_offset + self.buf_valid_bytes.start,
            self.get_data(),
        )
    }
    /// advance the offset of where to read next by `delta`
    pub fn advance_offset(&mut self, delta: usize) {
        if self.buf_valid_bytes.len() >= delta {
            self.buf_valid_bytes.start += delta;
        } else {
            let additional_amount_to_skip = delta - self.buf_valid_bytes.len();
            self.buf_valid_bytes = 0..0;
            self.file_offset_of_next_read += additional_amount_to_skip;
        }
    }
    /// specify the amount of data required to read next time `read` is called
    pub fn set_required_data_len(&mut self, len: usize) {
        self.read_requirements = Some(len);
    }
}

#[cfg(all(unix, test))]
mod tests {
    use {super::*, std::io::Write, tempfile::tempfile};

    #[test]
    fn test_buffered_reader() {
        // Setup a sample file with 32 bytes of data
        let file_size = 32;
        let mut sample_file = tempfile().unwrap();
        let bytes: Vec<u8> = (0..file_size as u8).collect();
        sample_file.write_all(&bytes).unwrap();

        // First read 16 bytes to fill buffer
        let buffer_size = 16;
        let file_len_valid = 32;
        let default_min_read = 8;
        let mut reader =
            BufferedReader::new(buffer_size, file_len_valid, &sample_file, default_min_read);
        let result = reader.read().unwrap();
        assert_eq!(result, BufferedReaderStatus::Success);
        let (offset, slice) = reader.get_offset_and_data();
        let mut expected_offset = 0;
        assert_eq!(offset, expected_offset);
        assert_eq!(slice.len(), buffer_size);
        assert_eq!(slice.slice(), &bytes[0..buffer_size]);

        // Consume the data and attempt to read next 32 bytes, expect to hit EOF and only read 16 bytes
        let advance = 16;
        let mut required_len = 32;
        reader.advance_offset(advance);
        reader.set_required_data_len(required_len);
        let result = reader.read().unwrap();
        assert_eq!(result, BufferedReaderStatus::Eof);
        let (offset, slice) = reader.get_offset_and_data();
        expected_offset += advance;
        let expected_slice_len = 16;
        assert_eq!(offset, expected_offset);
        assert_eq!(slice.len(), expected_slice_len);
        assert_eq!(slice.slice(), &bytes[offset..file_size]);

        // Continue reading should yield EOF and empty slice.
        reader.advance_offset(advance);
        reader.set_required_data_len(required_len);
        let result = reader.read().unwrap();
        assert_eq!(result, BufferedReaderStatus::Eof);
        let (offset, slice) = reader.get_offset_and_data();
        expected_offset += advance;
        assert_eq!(offset, expected_offset);
        let expected_slice_len = 0;
        assert_eq!(slice.len(), expected_slice_len);

        // set_required_data to zero and offset should not change, and slice should be empty.
        required_len = 0;
        reader.set_required_data_len(required_len);
        let result = reader.read().unwrap();
        assert_eq!(result, BufferedReaderStatus::Success);
        let (offset, slice) = reader.get_offset_and_data();
        let expected_offset = file_len_valid;
        assert_eq!(offset, expected_offset);
        let expected_slice_len = 0;
        assert_eq!(slice.len(), expected_slice_len);
    }

    #[test]
    fn test_buffered_reader_with_extra_data_in_file() {
        // Setup a sample file with 32 bytes of data
        let mut sample_file = tempfile().unwrap();
        let file_size = 32;
        let bytes: Vec<u8> = (0..file_size as u8).collect();
        sample_file.write_all(&bytes).unwrap();

        // Set file valid_len to 30 (i.e. 2 garbage bytes at the end of the file)
        let valid_len = 30;

        // First read 16 bytes to fill buffer
        let buffer_size = 16;
        let default_min_read_size = 8;
        let mut reader =
            BufferedReader::new(buffer_size, valid_len, &sample_file, default_min_read_size);
        let result = reader.read().unwrap();
        assert_eq!(result, BufferedReaderStatus::Success);
        let (offset, slice) = reader.get_offset_and_data();
        let mut expected_offset = 0;
        assert_eq!(offset, expected_offset);
        assert_eq!(slice.len(), buffer_size);
        assert_eq!(slice.slice(), &bytes[0..buffer_size]);

        // Consume the data and attempt read next 32 bytes, expect to hit `valid_len`, and only read 14 bytes
        let mut advance = 16;
        let mut required_data_len = 32;
        reader.advance_offset(advance);
        reader.set_required_data_len(required_data_len);
        let result = reader.read().unwrap();
        assert_eq!(result, BufferedReaderStatus::Eof);
        let (offset, slice) = reader.get_offset_and_data();
        expected_offset += advance;
        assert_eq!(offset, expected_offset);
        let expected_slice_len = valid_len - offset;
        assert_eq!(slice.len(), expected_slice_len);
        let expected_slice_range = 16..30;
        assert_eq!(slice.slice(), &bytes[expected_slice_range]);

        // Continue reading should yield EOF and empty slice.
        advance = 14;
        required_data_len = 32;
        reader.advance_offset(advance);
        reader.set_required_data_len(required_data_len);
        let result = reader.read().unwrap();
        assert_eq!(result, BufferedReaderStatus::Eof);
        let (offset, slice) = reader.get_offset_and_data();
        expected_offset += advance;
        assert_eq!(offset, expected_offset);
        let expected_slice_len = 0;
        assert_eq!(slice.len(), expected_slice_len);

        // Move the offset passed `valid_len`, expect to hit EOF and return empty slice.
        advance = 1;
        required_data_len = 8;
        reader.advance_offset(advance);
        reader.set_required_data_len(required_data_len);
        let result = reader.read().unwrap();
        assert_eq!(result, BufferedReaderStatus::Eof);
        let (offset, slice) = reader.get_offset_and_data();
        expected_offset += advance;
        assert_eq!(offset, expected_offset);
        let expected_slice_len = 0;
        assert_eq!(slice.len(), expected_slice_len);

        // Move the offset passed file_len, expect to hit EOF and return empty slice.
        advance = 3;
        required_data_len = 8;
        reader.advance_offset(advance);
        reader.set_required_data_len(required_data_len);
        let result = reader.read().unwrap();
        assert_eq!(result, BufferedReaderStatus::Eof);
        let (offset, slice) = reader.get_offset_and_data();
        expected_offset += advance;
        assert_eq!(offset, expected_offset);
        let expected_slice_len = 0;
        assert_eq!(slice.len(), expected_slice_len);
    }

    #[test]
    fn test_buffered_reader_partial_consume() {
        // Setup a sample file with 32 bytes of data
        let mut sample_file = tempfile().unwrap();
        let file_size = 32;
        let bytes: Vec<u8> = (0..file_size as u8).collect();
        sample_file.write_all(&bytes).unwrap();

        // First read 16 bytes to fill buffer
        let buffer_size = 16;
        let file_len_valid = 32;
        let default_min_read_size = 8;
        let mut reader = BufferedReader::new(
            buffer_size,
            file_len_valid,
            &sample_file,
            default_min_read_size,
        );
        let result = reader.read().unwrap();
        assert_eq!(result, BufferedReaderStatus::Success);
        let (offset, slice) = reader.get_offset_and_data();
        let mut expected_offset = 0;
        assert_eq!(offset, expected_offset);
        assert_eq!(slice.len(), buffer_size);
        assert_eq!(slice.slice(), &bytes[0..buffer_size]);

        // Consume the partial data (8 byte) and attempt to read next 8 bytes
        let mut advance = 8;
        let mut required_len = 8;
        reader.advance_offset(advance);
        reader.set_required_data_len(required_len);
        let result = reader.read().unwrap();
        assert_eq!(result, BufferedReaderStatus::Success);
        let (offset, slice) = reader.get_offset_and_data();
        expected_offset += advance;
        assert_eq!(offset, expected_offset);
        assert_eq!(slice.len(), required_len);
        assert_eq!(
            slice.slice(),
            &bytes[expected_offset..expected_offset + required_len]
        ); // no need to read more

        // Continue reading should succeed and read the rest 16 bytes.
        advance = 8;
        required_len = 16;
        reader.advance_offset(advance);
        reader.set_required_data_len(required_len);
        let result = reader.read().unwrap();
        assert_eq!(result, BufferedReaderStatus::Success);
        let (offset, slice) = reader.get_offset_and_data();
        expected_offset += advance;
        assert_eq!(offset, expected_offset);
        assert_eq!(slice.len(), required_len);
        assert_eq!(
            slice.slice(),
            &bytes[expected_offset..expected_offset + required_len]
        );

        // Continue reading should yield EOF and empty slice.
        advance = 16;
        required_len = 32;
        reader.advance_offset(advance);
        reader.set_required_data_len(required_len);
        let result = reader.read().unwrap();
        assert_eq!(result, BufferedReaderStatus::Eof);
        let (offset, slice) = reader.get_offset_and_data();
        expected_offset += advance;
        assert_eq!(offset, expected_offset);
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_buffered_reader_partial_consume_with_move() {
        // Setup a sample file with 32 bytes of data
        let mut sample_file = tempfile().unwrap();
        let file_size = 32;
        let bytes: Vec<u8> = (0..file_size as u8).collect();
        sample_file.write_all(&bytes).unwrap();

        // First read 16 bytes to fill buffer
        let buffer_size = 16;
        let valid_len = 32;
        let default_min_read = 8;
        let mut reader =
            BufferedReader::new(buffer_size, valid_len, &sample_file, default_min_read);
        let result = reader.read().unwrap();
        assert_eq!(result, BufferedReaderStatus::Success);
        let (offset, slice) = reader.get_offset_and_data();
        let mut expected_offset = 0;
        assert_eq!(offset, expected_offset);
        assert_eq!(slice.len(), buffer_size);
        assert_eq!(slice.slice(), &bytes[0..buffer_size]);

        // Consume the partial data (8 bytes) and attempt to read next 16 bytes
        // This will move the leftover 8bytes and read next 8 bytes.
        let mut advance = 8;
        let mut required_data_len = 16;
        reader.advance_offset(advance);
        reader.set_required_data_len(required_data_len);
        let result = reader.read().unwrap();
        assert_eq!(result, BufferedReaderStatus::Success);
        let (offset, slice) = reader.get_offset_and_data();
        expected_offset += advance;
        assert_eq!(offset, expected_offset);
        assert_eq!(slice.len(), required_data_len);
        assert_eq!(
            slice.slice(),
            &bytes[expected_offset..expected_offset + required_data_len]
        );

        // Continue reading should succeed and read the rest 8 bytes.
        advance = 16;
        required_data_len = 8;
        reader.advance_offset(advance);
        reader.set_required_data_len(required_data_len);
        let result = reader.read().unwrap();
        assert_eq!(result, BufferedReaderStatus::Success);
        let (offset, slice) = reader.get_offset_and_data();
        expected_offset += advance;
        assert_eq!(offset, expected_offset);
        assert_eq!(slice.len(), required_data_len);
        assert_eq!(
            slice.slice(),
            &bytes[expected_offset..expected_offset + required_data_len]
        );
    }
}
