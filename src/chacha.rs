use crate::db_ledger::{DbLedger, DEFAULT_SLOT_HEIGHT};
use std::fs::File;
use std::io;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Arc;

use crate::storage_stage::ENTRIES_PER_SEGMENT;

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
    db_ledger: &Arc<DbLedger>,
    slice: u64,
    out_path: &Path,
    ivec: &mut [u8; CHACHA_BLOCK_SIZE],
) -> io::Result<()> {
    let mut out_file =
        BufWriter::new(File::create(out_path).expect("Can't open ledger encrypted data file"));
    let mut buffer = [0; 4 * 1024];
    let mut encrypted_buffer = [0; 4 * 1024];
    let key = [0; CHACHA_KEY_SIZE];
    let mut total_entries = 0;
    let mut entry = slice;

    loop {
        match db_ledger.get_blob_bytes(
            entry,
            ENTRIES_PER_SEGMENT - total_entries,
            &mut buffer,
            DEFAULT_SLOT_HEIGHT,
        ) {
            Ok((num_entries, entry_len)) => {
                debug!("read {} bytes", entry_len);
                let size = entry_len as usize;
                if size == 0 {
                    break;
                }
                chacha_cbc_encrypt(&buffer[..size], &mut encrypted_buffer[..size], &key, ivec);
                if let Err(res) = out_file.write(&encrypted_buffer[..size]) {
                    println!("Error writing file! {:?}", res);
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::chacha::chacha_cbc_encrypt_ledger;
    use crate::db_ledger::{DbLedger, DEFAULT_SLOT_HEIGHT};
    use crate::entry::Entry;
    use crate::ledger::get_tmp_ledger_path;
    use ring::signature::Ed25519KeyPair;
    use solana_sdk::budget_transaction::BudgetTransaction;
    use solana_sdk::hash::{hash, Hash, Hasher};
    use solana_sdk::signature::KeypairUtil;
    use solana_sdk::transaction::Transaction;
    use std::fs::remove_file;
    use std::fs::File;
    use std::io::Read;
    use std::path::Path;
    use std::sync::Arc;
    use untrusted::Input;

    fn make_tiny_deterministic_test_entries(num: usize) -> Vec<Entry> {
        let zero = Hash::default();
        let one = hash(&zero.as_ref());
        let pkcs = [
            48, 83, 2, 1, 1, 48, 5, 6, 3, 43, 101, 112, 4, 34, 4, 32, 109, 148, 235, 20, 97, 127,
            43, 194, 109, 43, 121, 76, 54, 38, 234, 14, 108, 68, 209, 227, 137, 191, 167, 144, 177,
            174, 57, 182, 79, 198, 196, 93, 161, 35, 3, 33, 0, 116, 121, 255, 78, 31, 95, 179, 172,
            30, 125, 206, 87, 88, 78, 46, 145, 25, 154, 161, 252, 3, 58, 235, 116, 39, 148, 193,
            150, 111, 61, 20, 226,
        ];
        let keypair = Ed25519KeyPair::from_pkcs8(Input::from(&pkcs)).unwrap();

        let mut id = one;
        let mut num_hashes = 0;
        (0..num)
            .map(|_| {
                Entry::new_mut(
                    &mut id,
                    &mut num_hashes,
                    vec![Transaction::budget_new_signature(
                        &keypair,
                        keypair.pubkey(),
                        keypair.pubkey(),
                        one,
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
        let db_ledger = Arc::new(DbLedger::open(&ledger_path).unwrap());
        let out_path = Path::new("test_chacha_encrypt_file_output.txt.enc");

        let entries = make_tiny_deterministic_test_entries(32);
        db_ledger
            .write_entries(DEFAULT_SLOT_HEIGHT, 0, &entries)
            .unwrap();

        let mut key = hex!(
            "abcd1234abcd1234abcd1234abcd1234 abcd1234abcd1234abcd1234abcd1234
                            abcd1234abcd1234abcd1234abcd1234 abcd1234abcd1234abcd1234abcd1234"
        );
        assert!(chacha_cbc_encrypt_ledger(&db_ledger, 0, out_path, &mut key).is_ok());
        let mut out_file = File::open(out_path).unwrap();
        let mut buf = vec![];
        let size = out_file.read_to_end(&mut buf).unwrap();
        let mut hasher = Hasher::default();
        hasher.hash(&buf[..size]);
        assert_eq!(
            hasher.result(),
            Hash::new(&hex!(
                "58433c941060af56b72bfeaca161f19ed6df531efb28961dc6b83f53fbf66ffe"
            )),
        );
        remove_file(out_path).unwrap();
    }
}
