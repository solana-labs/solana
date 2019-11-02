use solana_ledger::blocktree::Blocktree;
use solana_sdk::clock::Slot;
use std::fs::File;
use std::io;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;

pub use solana_chacha_sys::chacha_cbc_encrypt;

pub const CHACHA_BLOCK_SIZE: usize = 64;
pub const CHACHA_KEY_SIZE: usize = 32;

pub fn chacha_cbc_encrypt_ledger(
    blocktree: &Arc<Blocktree>,
    start_slot: Slot,
    slots_per_segment: u64,
    out_path: &Path,
    ivec: &mut [u8; CHACHA_BLOCK_SIZE],
) -> io::Result<usize> {
    let mut out_file =
        BufWriter::new(File::create(out_path).expect("Can't open ledger encrypted data file"));
    const BUFFER_SIZE: usize = 8 * 1024;
    let mut buffer = [0; BUFFER_SIZE];
    let mut encrypted_buffer = [0; BUFFER_SIZE];
    let key = [0; CHACHA_KEY_SIZE];
    let mut total_size = 0;
    let mut current_slot = start_slot;
    let mut start_index = 0;
    loop {
        match blocktree.get_data_shreds(current_slot, start_index, std::u64::MAX, &mut buffer) {
            Ok((last_index, mut size)) => {
                debug!(
                    "chacha: encrypting slice: {} num_shreds: {} data_len: {}",
                    current_slot,
                    last_index.saturating_sub(start_index),
                    size
                );
                debug!("read {} bytes", size);

                if size == 0 {
                    if current_slot.saturating_sub(start_slot) < slots_per_segment {
                        current_slot += 1;
                        start_index = 0;
                        continue;
                    } else {
                        break;
                    }
                }

                if size < BUFFER_SIZE {
                    // round to the nearest key_size boundary
                    size = (size + CHACHA_KEY_SIZE - 1) & !(CHACHA_KEY_SIZE - 1);
                }
                total_size += size;

                chacha_cbc_encrypt(&buffer[..size], &mut encrypted_buffer[..size], &key, ivec);
                if let Err(res) = out_file.write(&encrypted_buffer[..size]) {
                    warn!("Error writing file! {:?}", res);
                    return Err(res);
                }

                start_index = last_index + 1;
            }
            Err(e) => {
                info!("Error encrypting file: {:?}", e);
                break;
            }
        }
    }
    Ok(total_size)
}

#[cfg(test)]
mod tests {
    use crate::chacha::chacha_cbc_encrypt_ledger;
    use crate::gen_keys::GenKeys;
    use solana_ledger::blocktree::get_tmp_ledger_path;
    use solana_ledger::blocktree::Blocktree;
    use solana_ledger::entry::Entry;
    use solana_sdk::hash::{hash, Hash, Hasher};
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::KeypairUtil;
    use solana_sdk::system_transaction;
    use std::fs::remove_file;
    use std::fs::File;
    use std::io::Read;
    use std::sync::Arc;

    fn make_tiny_deterministic_test_entries(num: usize) -> Vec<Entry> {
        let zero = Hash::default();
        let one = hash(&zero.as_ref());

        let seed = [2u8; 32];
        let mut rnd = GenKeys::new(seed);
        let keypair = rnd.gen_keypair();

        let mut id = one;
        let mut num_hashes = 0;
        (0..num)
            .map(|_| {
                Entry::new_mut(
                    &mut id,
                    &mut num_hashes,
                    vec![system_transaction::transfer(
                        &keypair,
                        &keypair.pubkey(),
                        1,
                        one,
                    )],
                )
            })
            .collect()
    }

    use std::{env, fs::create_dir_all, path::PathBuf};
    fn tmp_file_path(name: &str) -> PathBuf {
        let out_dir = env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());
        let mut path = PathBuf::new();
        path.push(out_dir);
        path.push("tmp");
        create_dir_all(&path).unwrap();

        path.push(format!("{}-{}", name, Pubkey::new_rand()));
        path
    }

    #[test]
    fn test_encrypt_ledger() {
        solana_logger::setup();
        let ledger_dir = "chacha_test_encrypt_file";
        let ledger_path = get_tmp_ledger_path(ledger_dir);
        let ticks_per_slot = 16;
        let slots_per_segment = 32;
        let blocktree = Arc::new(Blocktree::open(&ledger_path).unwrap());
        let out_path = tmp_file_path("test_encrypt_ledger");

        let seed = [2u8; 32];
        let mut rnd = GenKeys::new(seed);
        let keypair = rnd.gen_keypair();

        let entries = make_tiny_deterministic_test_entries(slots_per_segment);
        blocktree
            .write_entries(
                0,
                0,
                0,
                ticks_per_slot,
                None,
                true,
                &Arc::new(keypair),
                entries,
            )
            .unwrap();

        let mut key = hex!(
            "abcd1234abcd1234abcd1234abcd1234 abcd1234abcd1234abcd1234abcd1234
                            abcd1234abcd1234abcd1234abcd1234 abcd1234abcd1234abcd1234abcd1234"
        );
        chacha_cbc_encrypt_ledger(&blocktree, 0, slots_per_segment as u64, &out_path, &mut key)
            .unwrap();
        let mut out_file = File::open(&out_path).unwrap();
        let mut buf = vec![];
        let size = out_file.read_to_end(&mut buf).unwrap();
        let mut hasher = Hasher::default();
        hasher.hash(&buf[..size]);

        //  golden needs to be updated if blob stuff changes....
        let golden: Hash = "BdmY3efqu7zbnFuGRAeFANwa35HkDdQ7hwhYez3xGXiM"
            .parse()
            .unwrap();

        assert_eq!(hasher.result(), golden);
        remove_file(&out_path).unwrap();
    }
}
