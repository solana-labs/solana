use crate::blocktree::Blocktree;
use solana_storage_api::SLOTS_PER_SEGMENT;
use std::fs::File;
use std::io;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;

pub const CHACHA_BLOCK_SIZE: usize = 64;
pub const CHACHA_KEY_SIZE: usize = 32;

#[link(name = "cpu-crypt")]
extern "C" {
    fn chacha20_cbc_encrypt(
        input: *const u8,
        output: *mut u8,
        in_len: usize,
        key: *const u8,
        ivec: *mut u8,
    );
}

pub fn chacha_cbc_encrypt(input: &[u8], output: &mut [u8], key: &[u8], ivec: &mut [u8]) {
    unsafe {
        chacha20_cbc_encrypt(
            input.as_ptr(),
            output.as_mut_ptr(),
            input.len(),
            key.as_ptr(),
            ivec.as_mut_ptr(),
        );
    }
}

pub fn chacha_cbc_encrypt_ledger(
    blocktree: &Arc<Blocktree>,
    slice: u64,
    out_path: &Path,
    ivec: &mut [u8; CHACHA_BLOCK_SIZE],
) -> io::Result<usize> {
    let mut out_file =
        BufWriter::new(File::create(out_path).expect("Can't open ledger encrypted data file"));
    const BUFFER_SIZE: usize = 8 * 1024;
    let mut buffer = [0; BUFFER_SIZE];
    let mut encrypted_buffer = [0; BUFFER_SIZE];
    let key = [0; CHACHA_KEY_SIZE];
    let mut total_entries = 0;
    let mut total_size = 0;
    let mut entry = slice;

    loop {
        match blocktree.read_blobs_bytes(0, SLOTS_PER_SEGMENT - total_entries, &mut buffer, entry) {
            Ok((num_entries, entry_len)) => {
                debug!(
                    "chacha: encrypting slice: {} num_entries: {} entry_len: {}",
                    slice, num_entries, entry_len
                );
                debug!("read {} bytes", entry_len);
                let mut size = entry_len as usize;
                if size == 0 {
                    break;
                }

                if size < BUFFER_SIZE {
                    // We are on the last block, round to the nearest key_size
                    // boundary
                    size = (size + CHACHA_KEY_SIZE - 1) & !(CHACHA_KEY_SIZE - 1);
                }
                total_size += size;

                chacha_cbc_encrypt(&buffer[..size], &mut encrypted_buffer[..size], &key, ivec);
                if let Err(res) = out_file.write(&encrypted_buffer[..size]) {
                    warn!("Error writing file! {:?}", res);
                    return Err(res);
                }

                total_entries += num_entries;
                entry += num_entries;
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
    use crate::blocktree::get_tmp_ledger_path;
    use crate::blocktree::Blocktree;
    use crate::chacha::chacha_cbc_encrypt_ledger;
    use crate::entry::Entry;
    use crate::gen_keys::GenKeys;
    use solana_sdk::hash::{hash, Hash, Hasher};
    use solana_sdk::signature::KeypairUtil;
    use solana_sdk::system_transaction;
    use std::fs::remove_file;
    use std::fs::File;
    use std::io::Read;
    use std::path::Path;
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
                    vec![system_transaction::create_user_account(
                        &keypair,
                        &keypair.pubkey(),
                        1,
                        one,
                        0,
                    )],
                )
            })
            .collect()
    }

    #[test]
    fn test_encrypt_ledger() {
        solana_logger::setup();
        let ledger_dir = "chacha_test_encrypt_file";
        let ledger_path = get_tmp_ledger_path(ledger_dir);
        let ticks_per_slot = 16;
        let blocktree = Arc::new(Blocktree::open(&ledger_path).unwrap());
        let out_path = Path::new("test_chacha_encrypt_file_output.txt.enc");

        let entries = make_tiny_deterministic_test_entries(32);
        blocktree
            .write_entries(0, 0, 0, ticks_per_slot, &entries)
            .unwrap();

        let mut key = hex!(
            "abcd1234abcd1234abcd1234abcd1234 abcd1234abcd1234abcd1234abcd1234
                            abcd1234abcd1234abcd1234abcd1234 abcd1234abcd1234abcd1234abcd1234"
        );
        chacha_cbc_encrypt_ledger(&blocktree, 0, out_path, &mut key).unwrap();
        let mut out_file = File::open(out_path).unwrap();
        let mut buf = vec![];
        let size = out_file.read_to_end(&mut buf).unwrap();
        let mut hasher = Hasher::default();
        hasher.hash(&buf[..size]);

        //  golden needs to be updated if blob stuff changes....
        let golden: Hash = "5Pz5KQyNht2nqkJhVd8F9zTFxzoDvbQSzaxQbtCPiyCo"
            .parse()
            .unwrap();

        assert_eq!(hasher.result(), golden);
        remove_file(out_path).unwrap();
    }
}
