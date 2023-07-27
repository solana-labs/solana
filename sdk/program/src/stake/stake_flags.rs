use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

/// Additional flags for stake state.
#[allow(dead_code)]
#[derive(
    Serialize,
    Deserialize,
    AbiExample,
    BorshDeserialize,
    BorshSchema,
    BorshSerialize,
    Copy,
    PartialEq,
    Eq,
    Clone,
    PartialOrd,
    Ord,
    Hash,
    Debug,
)]
pub struct StakeFlags {
    bits: u8,
}

/// Currently, only bit 1 is used. The other 7 bits are reserved for future usage.
#[allow(dead_code)]
impl StakeFlags {
    ///  Stake must be fully activated before deactivation is allowed (bit 1).
    pub const MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED: Self =
        Self { bits: 0b0000_0001 };

    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    pub const fn contains(&self, other: Self) -> bool {
        (self.bits & other.bits) == other.bits
    }

    pub fn remove(&mut self, other: Self) {
        self.bits &= !other.bits;
    }

    pub fn set(&mut self, other: Self) {
        self.bits |= other.bits;
    }

    pub const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }
}

#[allow(dead_code)]
impl Default for StakeFlags {
    fn default() -> Self {
        StakeFlags::empty()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_stake_flags() {
        let mut f = StakeFlags::empty();
        assert!(!f.contains(StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED));

        f.set(StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED);
        assert!(f.contains(StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED));

        f.remove(StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED);
        assert!(!f.contains(StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED));

        let f1 = StakeFlags::empty();
        let f2 = StakeFlags::empty();
        let f3 = f1.union(f2);
        assert!(!f3.contains(StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED));

        let f1 = StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED;
        let f2 = StakeFlags::empty();
        let f3 = f1.union(f2);
        assert!(f3.contains(StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED));

        let f1 = StakeFlags::empty();
        let f2 = StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED;
        let f3 = f1.union(f2);
        assert!(f3.contains(StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED));

        let f1 = StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED;
        let f2 = StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED;
        let f3 = f1.union(f2);
        assert!(f3.contains(StakeFlags::MUST_FULLY_ACTIVATE_BEFORE_DEACTIVATION_IS_PERMITTED));
    }
}
