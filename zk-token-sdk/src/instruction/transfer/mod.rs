mod encryption;
mod with_fee;
mod without_fee;

#[cfg(not(target_os = "solana"))]
use {
    crate::{
        encryption::{
            elgamal::ElGamalCiphertext,
            pedersen::{PedersenCommitment, PedersenOpening},
        },
        instruction::errors::InstructionError,
    },
    curve25519_dalek::scalar::Scalar,
};
#[cfg(not(target_os = "solana"))]
pub use {
    encryption::{FeeEncryption, TransferAmountCiphertext},
    with_fee::TransferWithFeePubkeys,
    without_fee::TransferPubkeys,
};
pub use {
    with_fee::{TransferWithFeeData, TransferWithFeeProofContext},
    without_fee::{TransferData, TransferProofContext},
};

#[cfg(not(target_os = "solana"))]
#[derive(Debug, Copy, Clone)]
pub enum Role {
    Source,
    Destination,
    Auditor,
    WithdrawWithheldAuthority,
}

/// Takes in a 64-bit number `amount` and a bit length `bit_length`. It returns:
///  - the `bit_length` low bits of `amount` interpreted as u64
///  - the (64 - `bit_length`) high bits of `amount` interpreted as u64
#[deprecated(since = "1.18.0", note = "please use `try_split_u64` instead")]
#[cfg(not(target_os = "solana"))]
pub fn split_u64(amount: u64, bit_length: usize) -> (u64, u64) {
    if bit_length == 64 {
        (amount, 0)
    } else {
        let lo = amount << (64 - bit_length) >> (64 - bit_length);
        let hi = amount >> bit_length;
        (lo, hi)
    }
}

/// Takes in a 64-bit number `amount` and a bit length `bit_length`. It returns:
/// - the `bit_length` low bits of `amount` interpretted as u64
/// - the `(64 - bit_length)` high bits of `amount` interpretted as u64
#[cfg(not(target_os = "solana"))]
pub fn try_split_u64(amount: u64, bit_length: usize) -> Result<(u64, u64), InstructionError> {
    match bit_length {
        0 => Ok((0, amount)),
        1..=63 => {
            let bit_length_complement = u64::BITS.checked_sub(bit_length as u32).unwrap();
            // shifts are safe as long as `bit_length` and `bit_length_complement` < 64
            let lo = amount
                .checked_shl(bit_length_complement) // clear out the high bits
                .and_then(|amount| amount.checked_shr(bit_length_complement))
                .unwrap(); // shift back
            let hi = amount.checked_shr(bit_length as u32).unwrap();

            Ok((lo, hi))
        }
        64 => Ok((amount, 0)),
        _ => Err(InstructionError::IllegalAmountBitLength),
    }
}

#[deprecated(since = "1.18.0", note = "please use `try_combine_lo_hi_u64` instead")]
#[cfg(not(target_os = "solana"))]
pub fn combine_lo_hi_u64(amount_lo: u64, amount_hi: u64, bit_length: usize) -> u64 {
    if bit_length == 64 {
        amount_lo
    } else {
        amount_lo + (amount_hi << bit_length)
    }
}

/// Combine two numbers that are interpretted as the low and high bits of a target number. The
/// `bit_length` parameter specifies the number of bits that `amount_hi` is to be shifted by.
#[cfg(not(target_os = "solana"))]
pub fn try_combine_lo_hi_u64(
    amount_lo: u64,
    amount_hi: u64,
    bit_length: usize,
) -> Result<u64, InstructionError> {
    match bit_length {
        0 => Ok(amount_hi),
        1..=63 => {
            // shifts are safe as long as `bit_length` < 64
            let amount_hi = amount_hi.checked_shl(bit_length as u32).unwrap();
            let combined = amount_lo
                .checked_add(amount_hi)
                .ok_or(InstructionError::IllegalAmountBitLength)?;
            Ok(combined)
        }
        64 => Ok(amount_lo),
        _ => Err(InstructionError::IllegalAmountBitLength),
    }
}

#[cfg(not(target_os = "solana"))]
fn try_combine_lo_hi_ciphertexts(
    ciphertext_lo: &ElGamalCiphertext,
    ciphertext_hi: &ElGamalCiphertext,
    bit_length: usize,
) -> Result<ElGamalCiphertext, InstructionError> {
    let two_power = if bit_length < u64::BITS as usize {
        1_u64.checked_shl(bit_length as u32).unwrap()
    } else {
        return Err(InstructionError::IllegalAmountBitLength);
    };
    Ok(ciphertext_lo + &(ciphertext_hi * &Scalar::from(two_power)))
}

#[deprecated(
    since = "1.18.0",
    note = "please use `try_combine_lo_hi_commitments` instead"
)]
#[cfg(not(target_os = "solana"))]
pub fn combine_lo_hi_commitments(
    comm_lo: &PedersenCommitment,
    comm_hi: &PedersenCommitment,
    bit_length: usize,
) -> PedersenCommitment {
    let two_power = (1_u64) << bit_length;
    comm_lo + comm_hi * &Scalar::from(two_power)
}

#[cfg(not(target_os = "solana"))]
pub fn try_combine_lo_hi_commitments(
    comm_lo: &PedersenCommitment,
    comm_hi: &PedersenCommitment,
    bit_length: usize,
) -> Result<PedersenCommitment, InstructionError> {
    let two_power = if bit_length < u64::BITS as usize {
        1_u64.checked_shl(bit_length as u32).unwrap()
    } else {
        return Err(InstructionError::IllegalAmountBitLength);
    };
    Ok(comm_lo + comm_hi * &Scalar::from(two_power))
}

#[deprecated(
    since = "1.18.0",
    note = "please use `try_combine_lo_hi_openings` instead"
)]
#[cfg(not(target_os = "solana"))]
pub fn combine_lo_hi_openings(
    opening_lo: &PedersenOpening,
    opening_hi: &PedersenOpening,
    bit_length: usize,
) -> PedersenOpening {
    let two_power = (1_u64) << bit_length;
    opening_lo + opening_hi * &Scalar::from(two_power)
}

#[cfg(not(target_os = "solana"))]
pub fn try_combine_lo_hi_openings(
    opening_lo: &PedersenOpening,
    opening_hi: &PedersenOpening,
    bit_length: usize,
) -> Result<PedersenOpening, InstructionError> {
    let two_power = if bit_length < u64::BITS as usize {
        1_u64.checked_shl(bit_length as u32).unwrap()
    } else {
        return Err(InstructionError::IllegalAmountBitLength);
    };
    Ok(opening_lo + opening_hi * &Scalar::from(two_power))
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct FeeParameters {
    /// Fee rate expressed as basis points of the transfer amount, i.e. increments of 0.01%
    pub fee_rate_basis_points: u16,
    /// Maximum fee assessed on transfers, expressed as an amount of tokens
    pub maximum_fee: u64,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_split_u64() {
        assert_eq!((0, 0), try_split_u64(0, 0).unwrap());
        assert_eq!((0, 0), try_split_u64(0, 1).unwrap());
        assert_eq!((0, 0), try_split_u64(0, 5).unwrap());
        assert_eq!((0, 0), try_split_u64(0, 63).unwrap());
        assert_eq!((0, 0), try_split_u64(0, 64).unwrap());
        assert_eq!(
            InstructionError::IllegalAmountBitLength,
            try_split_u64(0, 65).unwrap_err()
        );

        assert_eq!((0, 1), try_split_u64(1, 0).unwrap());
        assert_eq!((1, 0), try_split_u64(1, 1).unwrap());
        assert_eq!((1, 0), try_split_u64(1, 5).unwrap());
        assert_eq!((1, 0), try_split_u64(1, 63).unwrap());
        assert_eq!((1, 0), try_split_u64(1, 64).unwrap());
        assert_eq!(
            InstructionError::IllegalAmountBitLength,
            try_split_u64(1, 65).unwrap_err()
        );

        assert_eq!((0, 33), try_split_u64(33, 0).unwrap());
        assert_eq!((1, 16), try_split_u64(33, 1).unwrap());
        assert_eq!((1, 1), try_split_u64(33, 5).unwrap());
        assert_eq!((33, 0), try_split_u64(33, 63).unwrap());
        assert_eq!((33, 0), try_split_u64(33, 64).unwrap());
        assert_eq!(
            InstructionError::IllegalAmountBitLength,
            try_split_u64(33, 65).unwrap_err()
        );

        let amount = u64::MAX;
        assert_eq!((0, amount), try_split_u64(amount, 0).unwrap());
        assert_eq!((1, (1 << 63) - 1), try_split_u64(amount, 1).unwrap());
        assert_eq!((31, (1 << 59) - 1), try_split_u64(amount, 5).unwrap());
        assert_eq!(((1 << 63) - 1, 1), try_split_u64(amount, 63).unwrap());
        assert_eq!((amount, 0), try_split_u64(amount, 64).unwrap());
        assert_eq!(
            InstructionError::IllegalAmountBitLength,
            try_split_u64(amount, 65).unwrap_err()
        );
    }

    fn test_split_and_combine(amount: u64, bit_length: usize) {
        let (amount_lo, amount_hi) = try_split_u64(amount, bit_length).unwrap();
        assert_eq!(
            try_combine_lo_hi_u64(amount_lo, amount_hi, bit_length).unwrap(),
            amount
        );
    }

    #[test]
    fn test_combine_lo_hi_u64() {
        test_split_and_combine(0, 0);
        test_split_and_combine(0, 1);
        test_split_and_combine(0, 5);
        test_split_and_combine(0, 63);
        test_split_and_combine(0, 64);

        test_split_and_combine(1, 0);
        test_split_and_combine(1, 1);
        test_split_and_combine(1, 5);
        test_split_and_combine(1, 63);
        test_split_and_combine(1, 64);

        test_split_and_combine(33, 0);
        test_split_and_combine(33, 1);
        test_split_and_combine(33, 5);
        test_split_and_combine(33, 63);
        test_split_and_combine(33, 64);

        test_split_and_combine(u64::MAX, 0);
        test_split_and_combine(u64::MAX, 1);
        test_split_and_combine(u64::MAX, 5);
        test_split_and_combine(u64::MAX, 63);
        test_split_and_combine(u64::MAX, 64);

        // illegal amount bit
        let err = try_combine_lo_hi_u64(0, 0, 65).unwrap_err();
        assert_eq!(err, InstructionError::IllegalAmountBitLength);

        // overflow
        let amount_lo = u64::MAX;
        let amount_hi = u64::MAX;
        let err = try_combine_lo_hi_u64(amount_lo, amount_hi, 1).unwrap_err();
        assert_eq!(err, InstructionError::IllegalAmountBitLength);
    }
}
