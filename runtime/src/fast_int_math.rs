use {
    num_traits::PrimInt,
    std::ops::Sub,
};

const fn num_bits<T>() -> u32 {
    (std::mem::size_of::<T>() * 8) as u32
}

/// compute the floor of log2 for int
pub fn log2_floor<T:PrimInt>(x: T) -> u32 {
    num_bits::<T>() - x.leading_zeros() - 1
}

/// compute the ceiling of log2 for int
pub fn log2_ceiling<T: PrimInt + Sub<u32, Output = T>>(x: T) -> u32 {
    num_bits::<T>() - (x - 1).leading_zeros()
}
