use crate::shred::Shred;
use solana_runtime::hard_forks::HardForks;
use solana_sdk::hash::{extend_and_hash, Hash};

pub fn compute_shred_version(genesis_hash: &Hash, hard_forks: Option<&HardForks>) -> u16 {
    use byteorder::{ByteOrder, LittleEndian};

    let mut hash = *genesis_hash;
    if let Some(hard_forks) = hard_forks {
        for (slot, count) in hard_forks.iter() {
            let mut buf = [0u8; 16];
            LittleEndian::write_u64(&mut buf[..8], *slot);
            LittleEndian::write_u64(&mut buf[8..], *count as u64);
            hash = extend_and_hash(&hash, &buf);
        }
    }

    Shred::version_from_hash(&hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_shred_version() {
        assert_eq!(compute_shred_version(&Hash::default(), None), 1);
        let mut hard_forks = HardForks::default();
        assert_eq!(
            compute_shred_version(&Hash::default(), Some(&hard_forks)),
            1
        );
        hard_forks.register(1);
        assert_eq!(
            compute_shred_version(&Hash::default(), Some(&hard_forks)),
            55551
        );
        hard_forks.register(1);
        assert_eq!(
            compute_shred_version(&Hash::default(), Some(&hard_forks)),
            46353
        );
    }
}
