macro_rules! dispatch {
    ($vis:vis fn $name:ident(&self $(, $arg:ident : $ty:ty)?) $(-> $out:ty)?) => {
        #[inline]
        $vis fn $name(&self $(, $arg:$ty)?) $(-> $out)? {
            match self {
                Self::Legacy(shred) => shred.$name($($arg, )?),
                Self::Merkle(shred) => shred.$name($($arg, )?),
            }
        }
    };
    ($vis:vis fn $name:ident(self $(, $arg:ident : $ty:ty)?) $(-> $out:ty)?) => {
        #[inline]
        $vis fn $name(self $(, $arg:$ty)?) $(-> $out)? {
            match self {
                Self::Legacy(shred) => shred.$name($($arg, )?),
                Self::Merkle(shred) => shred.$name($($arg, )?),
            }
        }
    };
    ($vis:vis fn $name:ident(&mut self $(, $arg:ident : $ty:ty)?) $(-> $out:ty)?) => {
        #[inline]
        $vis fn $name(&mut self $(, $arg:$ty)?) $(-> $out)? {
            match self {
                Self::Legacy(shred) => shred.$name($($arg, )?),
                Self::Merkle(shred) => shred.$name($($arg, )?),
            }
        }
    }
}

macro_rules! impl_shred_common {
    () => {
        #[inline]
        fn common_header(&self) -> &ShredCommonHeader {
            &self.common_header
        }

        #[inline]
        fn payload(&self) -> &Vec<u8> {
            &self.payload
        }

        fn into_payload(self) -> Vec<u8> {
            self.payload
        }

        fn set_signature(&mut self, signature: Signature) {
            bincode::serialize_into(&mut self.payload[..], &signature).unwrap();
            self.common_header.signature = signature;
        }

        // Only for tests.
        fn set_index(&mut self, index: u32) {
            match self.common_header.shred_variant {
                ShredVariant::LegacyCode | ShredVariant::LegacyData => {
                    self.common_header.index = index;
                    bincode::serialize_into(&mut self.payload[..], &self.common_header).unwrap();
                }
                ShredVariant::MerkleCode(_) | ShredVariant::MerkleData(_) => {
                    panic!("Not Implemented!");
                }
            }
        }

        // Only for tests.
        fn set_slot(&mut self, slot: Slot) {
            match self.common_header.shred_variant {
                ShredVariant::LegacyCode | ShredVariant::LegacyData => {
                    self.common_header.slot = slot;
                    bincode::serialize_into(&mut self.payload[..], &self.common_header).unwrap();
                }
                ShredVariant::MerkleCode(_) | ShredVariant::MerkleData(_) => {
                    panic!("Not Implemented!");
                }
            }
        }
    };
}

pub(super) use {dispatch, impl_shred_common};
