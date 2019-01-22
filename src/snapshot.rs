use crate::bank::Bank;
use crate::result::Result;
use bincode::deserialize;
use bincode::serialize;
use solana_sdk::hash::Hash;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::mem::size_of;
use std::sync::Arc;

pub fn create_snapshot(
    path: &str,
    bank: &Arc<Bank>,
    entry_id: Hash,
    entry_height: u64,
) -> Result<()> {
    let tmp = format!("{}.tmp", path);
    {
        let mut snapshot_file = File::create(tmp.clone())?;

        let id = serialize(&entry_id).unwrap();
        snapshot_file.write_all(&id)?;
        let height = serialize(&entry_height).unwrap();
        snapshot_file.write_all(&height)?;

        let v = bank.serialize();
        snapshot_file.write_all(&v)?;
    }
    fs::rename(tmp, path)?;

    Ok(())
}

pub fn load_from_snapshot(path: &str) -> Result<(Bank, u64, Hash)> {
    let bytes = fs::read(path)?;
    info!("loading bank from snapshot");
    let mut cur = size_of::<Hash>();
    let last_entry_id = deserialize(&bytes[..cur])?;
    let entry_height = deserialize(&bytes[cur..cur + 8])?;
    cur += 8;
    let bank = Bank::new_from_snapshot(&bytes[cur..])?;
    info!(
        "loaded: height: {} entry id: {}",
        entry_height, last_entry_id
    );
    Ok((bank, entry_height, last_entry_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mint::Mint;
    use solana_sdk::hash::{hash, Hash};

    #[test]
    fn test_snapshot() {
        let mint = Mint::new(1000);
        let entry_height = 42;
        let bank = Bank::new(&mint);
        let hash0 = Hash::default();
        let id = hash(&hash0.as_ref());
        let path = "test_snapshot";
        create_snapshot(path, &Arc::new(bank), id, entry_height).unwrap();

        let (_bank1, entry_height1, hash1) = load_from_snapshot(path).unwrap();
        assert_eq!(hash1, id);
        assert_eq!(entry_height1, entry_height);
    }
}
