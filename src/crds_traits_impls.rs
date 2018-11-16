use bloom::BloomHashIndex;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;

fn slice_hash(slice: &[u8], hash_index: u64) -> u64 {
    let len = slice.len();
    assert!(len < 256);
    let mut rv = 0u64;
    for i in 0..8 {
        let pos = (hash_index >> i) & 0xff;
        rv |= u64::from(slice[pos as usize % len]) << i;
    }
    rv
}

impl BloomHashIndex for Pubkey {
    fn hash(&self, hash_index: u64) -> u64 {
        slice_hash(self.as_ref(), hash_index)
    }
}

impl BloomHashIndex for Hash {
    fn hash(&self, hash_index: u64) -> u64 {
        slice_hash(self.as_ref(), hash_index)
    }
}
