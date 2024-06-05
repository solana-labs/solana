use {
    crate::{instruction::InstructionError, pubkey::Pubkey},
    std::io::{Cursor, Read},
};

pub(crate) fn read_u8<T: AsRef<[u8]>>(cursor: &mut Cursor<T>) -> Result<u8, InstructionError> {
    let mut buf = [0; 1];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(buf[0])
}

pub(crate) fn read_u32<T: AsRef<[u8]>>(cursor: &mut Cursor<T>) -> Result<u32, InstructionError> {
    let mut buf = [0; 4];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(u32::from_le_bytes(buf))
}

pub(crate) fn read_u64<T: AsRef<[u8]>>(cursor: &mut Cursor<T>) -> Result<u64, InstructionError> {
    let mut buf = [0; 8];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(u64::from_le_bytes(buf))
}

pub(crate) fn read_option_u64<T: AsRef<[u8]>>(
    cursor: &mut Cursor<T>,
) -> Result<Option<u64>, InstructionError> {
    let variant = read_u8(cursor)?;
    match variant {
        0 => Ok(None),
        1 => read_u64(cursor).map(Some),
        _ => Err(InstructionError::InvalidAccountData),
    }
}

pub(crate) fn read_i64<T: AsRef<[u8]>>(cursor: &mut Cursor<T>) -> Result<i64, InstructionError> {
    let mut buf = [0; 8];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(i64::from_le_bytes(buf))
}

pub(crate) fn read_pubkey<T: AsRef<[u8]>>(
    cursor: &mut Cursor<T>,
) -> Result<Pubkey, InstructionError> {
    let mut buf = [0; 32];
    cursor
        .read_exact(&mut buf)
        .map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(Pubkey::from(buf))
}

pub(crate) fn read_bool<T: AsRef<[u8]>>(cursor: &mut Cursor<T>) -> Result<bool, InstructionError> {
    let byte = read_u8(cursor)?;
    match byte {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(InstructionError::InvalidAccountData),
    }
}

#[cfg(feature = "borsh")]
#[cfg(test)]
mod test {
    use {super::*, rand::Rng, std::fmt::Debug};

    #[test]
    fn test_read_u8() {
        for _ in 0..100 {
            let test_value = rand::random::<u8>();
            test_read(read_u8, test_value);
        }
    }

    #[test]
    fn test_read_u32() {
        for _ in 0..100 {
            let test_value = rand::random::<u32>();
            test_read(read_u32, test_value);
        }
    }

    #[test]
    fn test_read_u64() {
        for _ in 0..100 {
            let test_value = rand::random::<u64>();
            test_read(read_u64, test_value);
        }
    }

    #[test]
    fn test_read_option_u64() {
        for _ in 0..100 {
            let test_value = rand::random::<Option<u64>>();
            test_read(read_option_u64, test_value);
        }
    }

    #[test]
    fn test_read_i64() {
        for _ in 0..100 {
            let test_value = rand::random::<i64>();
            test_read(read_i64, test_value);
        }
    }

    #[test]
    fn test_read_pubkey() {
        for _ in 0..100 {
            let mut buf = [0; 32];
            rand::thread_rng().fill(&mut buf);
            let test_value = Pubkey::from(buf);
            test_read(read_pubkey, test_value);
        }
    }

    #[test]
    fn test_read_bool() {
        test_read(read_bool, false);
        test_read(read_bool, true);
    }

    fn test_read<T: Debug + PartialEq + serde::Serialize + borsh0_10::BorshSerialize>(
        reader: fn(&mut Cursor<Vec<u8>>) -> Result<T, InstructionError>,
        test_value: T,
    ) {
        let bincode_bytes = bincode::serialize(&test_value).unwrap();
        let mut cursor = Cursor::new(bincode_bytes);
        let bincode_read = reader(&mut cursor).unwrap();

        let borsh_bytes = borsh0_10::to_vec(&test_value).unwrap();
        let mut cursor = Cursor::new(borsh_bytes);
        let borsh_read = reader(&mut cursor).unwrap();

        assert_eq!(test_value, bincode_read);
        assert_eq!(test_value, borsh_read);
    }
}
