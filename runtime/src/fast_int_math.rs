use {num_traits::PrimInt, std::ops::Sub};

/// return the bit width of the integer type
const fn num_bits<T>() -> u32 {
    (std::mem::size_of::<T>() * 8) as u32
}

/// compute the floor of log2 for int
pub fn log2_floor<T: PrimInt>(x: T) -> u32 {
    num_bits::<T>() - x.leading_zeros() - 1
}

/// compute the ceiling of log2 for int
pub fn log2_ceiling<T: PrimInt + Sub<u32, Output = T>>(x: T) -> u32 {
    num_bits::<T>() - (x - 1).leading_zeros()
}

/// compute pow(2, exponent)
pub fn pow2(exponent: u32) -> u64 {
    1_u64 << exponent
}

/// compute pow(2, exponent) checked
pub fn pow2_checked(exponent: u32) -> u64 {
    1_u64.checked_shl(exponent)
}
