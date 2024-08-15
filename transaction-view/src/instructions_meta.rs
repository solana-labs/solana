use {
    crate::{
        bytes::{
            advance_offset_for_array, check_remaining, optimized_read_compressed_u16, read_byte,
        },
        result::Result,
    },
    solana_svm_transaction::instruction::SVMInstruction,
};

/// Contains metadata about the instructions in a transaction packet.
#[derive(Default)]
pub struct InstructionsMeta {
    /// The number of instructions in the transaction.
    pub(crate) num_instructions: u16,
    /// The offset to the first instruction in the transaction.
    pub(crate) offset: u16,
}

impl InstructionsMeta {
    /// Get the number of instructions and offset to the first instruction.
    /// The offset will be updated to point to the first byte after the last
    /// instruction.
    /// This function will parse each individual instruction to ensure the
    /// instruction data is well-formed, but will not cache data related to
    /// these instructions.
    #[inline(always)]
    pub fn try_new(bytes: &[u8], offset: &mut usize) -> Result<Self> {
        // Read the number of instructions at the current offset.
        // Each instruction needs at least 3 bytes, so do a sanity check here to
        // ensure we have enough bytes to read the number of instructions.
        let num_instructions = optimized_read_compressed_u16(bytes, offset)?;
        check_remaining(
            bytes,
            *offset,
            3usize.wrapping_mul(usize::from(num_instructions)),
        )?;

        // We know the offset does not exceed packet length, and our packet
        // length is less than u16::MAX, so we can safely cast to u16.
        let instructions_offset = *offset as u16;

        // The instructions do not have a fixed size. So we must iterate over
        // each instruction to find the total size of the instructions,
        // and check for any malformed instructions or buffer overflows.
        for _index in 0..num_instructions {
            // Each instruction has 3 pieces:
            // 1. Program ID index (u8)
            // 2. Accounts indexes ([u8])
            // 3. Data ([u8])

            // Read the program ID index.
            let _program_id_index = read_byte(bytes, offset)?;

            // Read the number of account indexes, and then update the offset
            // to skip over the account indexes.
            let num_accounts = optimized_read_compressed_u16(bytes, offset)?;
            advance_offset_for_array::<u8>(bytes, offset, num_accounts)?;

            // Read the length of the data, and then update the offset to skip
            // over the data.
            let data_len = optimized_read_compressed_u16(bytes, offset)?;
            advance_offset_for_array::<u8>(bytes, offset, data_len)?
        }

        Ok(Self {
            num_instructions,
            offset: instructions_offset,
        })
    }
}

pub struct InstructionsIterator<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) offset: usize,
    pub(crate) num_instructions: u16,
    pub(crate) index: u16,
}

impl<'a> Iterator for InstructionsIterator<'a> {
    type Item = SVMInstruction<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.num_instructions {
            // Each instruction has 3 pieces:
            // 1. Program ID index (u8)
            // 2. Accounts indexes ([u8])
            // 3. Data ([u8])

            // Read the program ID index.
            let program_id_index = read_byte(self.bytes, &mut self.offset).ok()?;

            // Read the number of account indexes, and then update the offset
            // to skip over the account indexes.
            let num_accounts = optimized_read_compressed_u16(self.bytes, &mut self.offset).ok()?;
            // SAFETY: Only returned after we check that there are enough bytes.
            let accounts = unsafe {
                core::slice::from_raw_parts(
                    self.bytes.as_ptr().add(self.offset),
                    usize::from(num_accounts),
                )
            };
            advance_offset_for_array::<u8>(self.bytes, &mut self.offset, num_accounts).ok()?;

            // Read the length of the data, and then update the offset to skip
            // over the data.
            let data_len = optimized_read_compressed_u16(self.bytes, &mut self.offset).ok()?;
            // SAFETY: Only returned after we check that there are enough bytes.
            let data = unsafe {
                core::slice::from_raw_parts(
                    self.bytes.as_ptr().add(self.offset),
                    usize::from(data_len),
                )
            };
            advance_offset_for_array::<u8>(self.bytes, &mut self.offset, data_len).ok()?;
            self.index = self.index.wrapping_add(1);

            Some(SVMInstruction {
                program_id_index,
                accounts,
                data,
            })
        } else {
            None
        }
    }
}

impl ExactSizeIterator for InstructionsIterator<'_> {
    fn len(&self) -> usize {
        usize::from(self.num_instructions.wrapping_sub(self.index))
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{instruction::CompiledInstruction, short_vec::ShortVec},
    };

    #[test]
    fn test_zero_instructions() {
        let bytes = bincode::serialize(&ShortVec(Vec::<CompiledInstruction>::new())).unwrap();
        let mut offset = 0;
        let instructions_meta = InstructionsMeta::try_new(&bytes, &mut offset).unwrap();

        assert_eq!(instructions_meta.num_instructions, 0);
        assert_eq!(instructions_meta.offset, 1);
        assert_eq!(offset, bytes.len());
    }

    #[test]
    fn test_num_instructions_too_high() {
        let mut bytes = bincode::serialize(&ShortVec(vec![CompiledInstruction {
            program_id_index: 0,
            accounts: vec![],
            data: vec![],
        }]))
        .unwrap();
        // modify the number of instructions to be too high
        bytes[0] = 0x02;
        let mut offset = 0;
        assert!(InstructionsMeta::try_new(&bytes, &mut offset).is_err());
    }

    #[test]
    fn test_single_instruction() {
        let bytes = bincode::serialize(&ShortVec(vec![CompiledInstruction {
            program_id_index: 0,
            accounts: vec![1, 2, 3],
            data: vec![4, 5, 6, 7, 8, 9, 10],
        }]))
        .unwrap();
        let mut offset = 0;
        let instructions_meta = InstructionsMeta::try_new(&bytes, &mut offset).unwrap();
        assert_eq!(instructions_meta.num_instructions, 1);
        assert_eq!(instructions_meta.offset, 1);
        assert_eq!(offset, bytes.len());
    }

    #[test]
    fn test_multiple_instructions() {
        let bytes = bincode::serialize(&ShortVec(vec![
            CompiledInstruction {
                program_id_index: 0,
                accounts: vec![1, 2, 3],
                data: vec![4, 5, 6, 7, 8, 9, 10],
            },
            CompiledInstruction {
                program_id_index: 1,
                accounts: vec![4, 5, 6],
                data: vec![7, 8, 9, 10, 11, 12, 13],
            },
        ]))
        .unwrap();
        let mut offset = 0;
        let instructions_meta = InstructionsMeta::try_new(&bytes, &mut offset).unwrap();
        assert_eq!(instructions_meta.num_instructions, 2);
        assert_eq!(instructions_meta.offset, 1);
        assert_eq!(offset, bytes.len());
    }

    #[test]
    fn test_invalid_instruction_accounts_vec() {
        let mut bytes = bincode::serialize(&ShortVec(vec![CompiledInstruction {
            program_id_index: 0,
            accounts: vec![1, 2, 3],
            data: vec![4, 5, 6, 7, 8, 9, 10],
        }]))
        .unwrap();

        // modify the number of accounts to be too high
        bytes[2] = 127;

        let mut offset = 0;
        assert!(InstructionsMeta::try_new(&bytes, &mut offset).is_err());
    }

    #[test]
    fn test_invalid_instruction_data_vec() {
        let mut bytes = bincode::serialize(&ShortVec(vec![CompiledInstruction {
            program_id_index: 0,
            accounts: vec![1, 2, 3],
            data: vec![4, 5, 6, 7, 8, 9, 10],
        }]))
        .unwrap();

        // modify the number of data bytes to be too high
        bytes[6] = 127;

        let mut offset = 0;
        assert!(InstructionsMeta::try_new(&bytes, &mut offset).is_err());
    }
}
