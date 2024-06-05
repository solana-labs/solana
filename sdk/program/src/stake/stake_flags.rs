#[cfg(feature = "borsh")]
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

/// Additional flags for stake state.
#[cfg_attr(feature = "frozen-abi", derive(AbiExample))]
#[cfg_attr(
    feature = "borsh",
    derive(BorshSerialize, BorshDeserialize, BorshSchema),
    borsh(crate = "borsh")
)]
#[derive(Serialize, Deserialize, Copy, PartialEq, Eq, Clone, PartialOrd, Ord, Hash, Debug)]
pub struct StakeFlags {
    bits: u8,
}

#[cfg(feature = "borsh")]
impl borsh0_10::de::BorshDeserialize for StakeFlags {
    fn deserialize_reader<R: borsh0_10::maybestd::io::Read>(
        reader: &mut R,
    ) -> ::core::result::Result<Self, borsh0_10::maybestd::io::Error> {
        Ok(Self {
            bits: borsh0_10::BorshDeserialize::deserialize_reader(reader)?,
        })
    }
}

#[cfg(feature = "borsh")]
impl borsh0_10::BorshSchema for StakeFlags {
    fn declaration() -> borsh0_10::schema::Declaration {
        "StakeFlags".to_string()
    }
    fn add_definitions_recursively(
        definitions: &mut borsh0_10::maybestd::collections::HashMap<
            borsh0_10::schema::Declaration,
            borsh0_10::schema::Definition,
        >,
    ) {
        let fields = borsh0_10::schema::Fields::NamedFields(<[_]>::into_vec(
            borsh0_10::maybestd::boxed::Box::new([(
                "bits".to_string(),
                <u8 as borsh0_10::BorshSchema>::declaration(),
            )]),
        ));
        let definition = borsh0_10::schema::Definition::Struct { fields };
        Self::add_definition(
            <Self as borsh0_10::BorshSchema>::declaration(),
            definition,
            definitions,
        );
        <u8 as borsh0_10::BorshSchema>::add_definitions_recursively(definitions);
    }
}

#[cfg(feature = "borsh")]
impl borsh0_10::ser::BorshSerialize for StakeFlags {
    fn serialize<W: borsh0_10::maybestd::io::Write>(
        &self,
        writer: &mut W,
    ) -> ::core::result::Result<(), borsh0_10::maybestd::io::Error> {
        borsh0_10::BorshSerialize::serialize(&self.bits, writer)?;
        Ok(())
    }
}

/// Currently, only bit 1 is used. The other 7 bits are reserved for future usage.
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
