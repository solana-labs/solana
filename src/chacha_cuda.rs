use hash::Hash;
use ledger::LedgerWindow;
use sigverify::chacha_cbc_encrypt_many_sample;
use std::io;

const CHACHA_BLOCK_SIZE: usize = 64;
const SHA256_BLOCK_SIZE: usize = 32;
const CHACHA_KEY_LEN: usize = 32;
const ENTRIES_PER_SLICE: u64 = 16;

pub fn chacha_cbc_encrypt_file_many_keys(
    in_path: &str,
    slice: u64,
    keys: &[u8],
    samples: &[u64],
) -> io::Result<Vec<Hash>> {
    if keys.len() % CHACHA_KEY_LEN != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "bad key length({}) not divisible by {} ",
                keys.len(),
                CHACHA_KEY_LEN
            ),
        ));
    }

    let mut ledger_window = LedgerWindow::open(in_path)?;
    let mut buffer = [0; 8 * 1024];
    let num_keys = keys.len() / CHACHA_KEY_LEN;
    let mut sha_states = vec![0; num_keys * SHA256_BLOCK_SIZE];
    let mut ivecs: Vec<u8> = vec![0; num_keys * CHACHA_BLOCK_SIZE];
    let mut entry = slice;
    let mut total_entries = 0;
    let mut total_entry_len = 0;
    let mut time: f32 = 0.0;
    loop {
        match ledger_window.get_entries_bytes(entry, ENTRIES_PER_SLICE - total_entries, &mut buffer)
        {
            Ok((num_entries, entry_len)) => {
                debug!(
                    "encrypting slice: {} num_entries: {} entry_len: {}",
                    slice, num_entries, entry_len
                );
                let entry_len_usz = entry_len as usize;
                unsafe {
                    chacha_cbc_encrypt_many_sample(
                        buffer[..entry_len_usz].as_ptr(),
                        sha_states.as_mut_ptr(),
                        entry_len_usz,
                        keys.as_ptr(),
                        ivecs.as_mut_ptr(),
                        num_keys as u32,
                        samples.as_ptr(),
                        samples.len() as u32,
                        total_entry_len,
                        &mut time,
                    );
                }

                total_entry_len += entry_len;
                total_entries += num_entries;
                entry += num_entries;
                debug!(
                    "total entries: {} entry: {} slice: {} entries_per_slice: {}",
                    total_entries, entry, slice, ENTRIES_PER_SLICE
                );
                if (entry - slice) >= ENTRIES_PER_SLICE {
                    break;
                }
            }
            Err(e) => {
                info!("Error encrypting file: {:?}", e);
                break;
            }
        }
    }
    let mut res = Vec::new();
    for x in 0..num_keys {
        let start = x * SHA256_BLOCK_SIZE;
        let end = start + SHA256_BLOCK_SIZE;
        res.push(Hash::new(&sha_states[start..end]));
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use chacha::chacha_cbc_encrypt_files;
    use chacha_cuda::chacha_cbc_encrypt_file_many_keys;
    use ledger::LedgerWriter;
    use ledger::{get_tmp_ledger_path, make_tiny_test_entries, LEDGER_DATA_FILE};
    use replicator::sample_file;
    use std::fs::{remove_dir_all, remove_file};
    use std::path::Path;

    #[test]
    fn test_encrypt_file_many_keys() {
        use logger;
        logger::setup();

        let entries = make_tiny_test_entries(16);
        let ledger_dir = "test_encrypt_file_many_keys";
        let ledger_path = get_tmp_ledger_path(ledger_dir);
        {
            let mut writer = LedgerWriter::open(&ledger_path, true).unwrap();
            writer.write_entries(&entries).unwrap();
        }

        let out_path = Path::new("test_chacha_encrypt_file_many_keys_output.txt.enc");

        let samples = [0];
        let key = "thetestkeyabcdefghijklmnopqrstuvwxyzthetestkeyabcdefghijklmnopqr";

        assert!(
            chacha_cbc_encrypt_files(
                &Path::new(&ledger_path).join(LEDGER_DATA_FILE),
                out_path,
                key.to_string()
            ).is_ok()
        );

        let ref_hash = sample_file(&out_path, &samples).unwrap();

        let hashes =
            chacha_cbc_encrypt_file_many_keys(&ledger_path, 0, key.as_bytes(), &samples).unwrap();

        assert_eq!(hashes[0], ref_hash);

        let _ignored = remove_dir_all(&ledger_path);
        let _ignored = remove_file(out_path);
    }

    #[test]
    fn test_encrypt_file_many_keys_bad_key_length() {
        let keys = "thetestkey";
        let ledger_dir = "test_encrypt_file_many_keys_bad_key_length";
        let ledger_path = get_tmp_ledger_path(ledger_dir);
        let samples = [0];
        assert!(
            chacha_cbc_encrypt_file_many_keys(&ledger_path, 0, keys.as_bytes(), &samples,).is_err()
        );
    }
}
