use solana_sdk::pubkey::Pubkey;

pub(crate) struct PubkeyBinCalculator16 {
    shift_bits: u32,
}

impl PubkeyBinCalculator16 {
    const fn num_bits<T>() -> usize {
        std::mem::size_of::<T>() * 8
    }

    fn log_2(x: u32) -> u32 {
        assert!(x > 0);
        Self::num_bits::<u32>() as u32 - x.leading_zeros() - 1
    }

    pub fn new(bins: usize) -> Self {
        const MAX_BITS: u32 = 16;
        assert!(bins > 0);
        let max_plus_1 = 1 << MAX_BITS;
        assert!(bins <= max_plus_1);
        assert!(bins.is_power_of_two());
        let bits = Self::log_2(bins as u32);
        // how many bits to shift away to ignore when calculating bin
        Self {
            shift_bits: MAX_BITS - bits,
        }
    }

    // this function is coupled to get_shift_bits_from_bins
    pub fn bin_from_pubkey(&self, pubkey: &Pubkey) -> usize {
        let as_ref = pubkey.as_ref();
        ((as_ref[0] as usize * 256 + as_ref[1] as usize) as usize) >> self.shift_bits
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_pubkey_bins_log2() {
        assert_eq!(PubkeyBinCalculator16::num_bits::<u8>(), 8);
        assert_eq!(PubkeyBinCalculator16::num_bits::<u32>(), 32);
        for i in 0..32 {
            assert_eq!(PubkeyBinCalculator16::log_2(2u32.pow(i)), i);
        }
    }

    #[test]
    #[should_panic(expected = "bins.is_power_of_two()")]
    fn test_pubkey_bins_illegal_bins3() {
        PubkeyBinCalculator16::new(3);
    }

    #[test]
    #[should_panic(expected = "bins <= max_plus_1")]
    fn test_pubkey_bins_illegal_bins2() {
        PubkeyBinCalculator16::new(65537);
    }
    #[test]
    #[should_panic(expected = "bins > 0")]
    fn test_pubkey_bins_illegal_bins() {
        PubkeyBinCalculator16::new(0);
    }
}
