// Module used by validators to approve storage mining proofs
// // in parallel using the GPU

use crate::blocktree::Blocktree;
use crate::chacha::{CHACHA_BLOCK_SIZE, CHACHA_KEY_SIZE};
use crate::sigverify::{
    chacha_cbc_encrypt_many_sample, chacha_end_sha_state, chacha_init_sha_state,
};
use solana_sdk::hash::Hash;
use std::io;
use std::mem::size_of;
use std::sync::Arc;

// Encrypt a file with multiple starting IV states, determined by ivecs.len()
//
// Then sample each block at the offsets provided by samples argument with sha256
// and return the vec of sha states
pub fn chacha_cbc_encrypt_file_many_keys(
    blocktree: &Arc<Blocktree>,
    segment: u64,
    slots_per_segment: u64,
    ivecs: &mut [u8],
    samples: &[u64],
) -> io::Result<Vec<Hash>> {
    if ivecs.len() % CHACHA_BLOCK_SIZE != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "bad IV length({}) not divisible by {} ",
                ivecs.len(),
                CHACHA_BLOCK_SIZE,
            ),
        ));
    }

    const BUFFER_SIZE: usize = 8 * 1024;
    let mut buffer = [0; BUFFER_SIZE];
    let num_keys = ivecs.len() / CHACHA_BLOCK_SIZE;
    let mut sha_states = vec![0; num_keys * size_of::<Hash>()];
    let mut int_sha_states = vec![0; num_keys * 112];
    let keys: Vec<u8> = vec![0; num_keys * CHACHA_KEY_SIZE]; // keys not used ATM, uniqueness comes from IV
    let mut current_slot = segment * slots_per_segment;
    let mut start_index = 0;
    let start_slot = current_slot;
    let mut total_size = 0;
    let mut time: f32 = 0.0;
    unsafe {
        chacha_init_sha_state(int_sha_states.as_mut_ptr(), num_keys as u32);
    }
    loop {
        match blocktree.get_data_shreds(current_slot, start_index, std::u64::MAX, &mut buffer) {
            Ok((last_index, mut size)) => {
                debug!(
                    "chacha_cuda: encrypting segment: {} num_shreds: {} data_len: {}",
                    segment,
                    last_index.saturating_sub(start_index),
                    size
                );

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

                unsafe {
                    chacha_cbc_encrypt_many_sample(
                        buffer[..size].as_ptr(),
                        int_sha_states.as_mut_ptr(),
                        size,
                        keys.as_ptr(),
                        ivecs.as_mut_ptr(),
                        num_keys as u32,
                        samples.as_ptr(),
                        samples.len() as u32,
                        total_size,
                        &mut time,
                    );
                }

                total_size += size as u64;
                start_index = last_index + 1;
            }
            Err(e) => {
                info!("Error encrypting file: {:?}", e);
                break;
            }
        }
    }
    unsafe {
        chacha_end_sha_state(
            int_sha_states.as_ptr(),
            sha_states.as_mut_ptr(),
            num_keys as u32,
        );
    }
    let mut res = Vec::new();
    for x in 0..num_keys {
        let start = x * size_of::<Hash>();
        let end = start + size_of::<Hash>();
        res.push(Hash::new(&sha_states[start..end]));
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use crate::blocktree::get_tmp_ledger_path;
    use crate::blocktree::Blocktree;
    use crate::chacha::chacha_cbc_encrypt_ledger;
    use crate::chacha_cuda::chacha_cbc_encrypt_file_many_keys;
    use crate::entry::make_tiny_test_entries;
    use crate::replicator::sample_file;
    use solana_sdk::clock::DEFAULT_SLOTS_PER_SEGMENT;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::fs::{remove_dir_all, remove_file};
    use std::path::Path;
    use std::sync::Arc;

    #[test]
    fn test_encrypt_file_many_keys_single() {
        solana_logger::setup();

        let slots_per_segment = 32;
        let entries = make_tiny_test_entries(slots_per_segment);
        let ledger_dir = "test_encrypt_file_many_keys_single";
        let ledger_path = get_tmp_ledger_path(ledger_dir);
        let ticks_per_slot = 16;
        let blocktree = Arc::new(Blocktree::open(&ledger_path).unwrap());

        blocktree
            .write_entries(
                0,
                0,
                0,
                ticks_per_slot,
                Some(0),
                true,
                &Arc::new(Keypair::new()),
                &entries,
            )
            .unwrap();

        let out_path = Path::new("test_chacha_encrypt_file_many_keys_single_output.txt.enc");

        let samples = [0];
        let mut ivecs = hex!(
            "abcd1234abcd1234abcd1234abcd1234 abcd1234abcd1234abcd1234abcd1234
                              abcd1234abcd1234abcd1234abcd1234 abcd1234abcd1234abcd1234abcd1234"
        );

        let mut cpu_iv = ivecs.clone();
        chacha_cbc_encrypt_ledger(
            &blocktree,
            0,
            slots_per_segment as u64,
            out_path,
            &mut cpu_iv,
        )
        .unwrap();

        let ref_hash = sample_file(&out_path, &samples).unwrap();

        let hashes = chacha_cbc_encrypt_file_many_keys(
            &blocktree,
            0,
            slots_per_segment as u64,
            &mut ivecs,
            &samples,
        )
        .unwrap();

        assert_eq!(hashes[0], ref_hash);

        let _ignored = remove_dir_all(&ledger_path);
        let _ignored = remove_file(out_path);
    }

    #[test]
    fn test_encrypt_file_many_keys_multiple_keys() {
        solana_logger::setup();

        let entries = make_tiny_test_entries(32);
        let ledger_dir = "test_encrypt_file_many_keys_multiple";
        let ledger_path = get_tmp_ledger_path(ledger_dir);
        let ticks_per_slot = 16;
        let blocktree = Arc::new(Blocktree::open(&ledger_path).unwrap());
        blocktree
            .write_entries(
                0,
                0,
                0,
                ticks_per_slot,
                Some(0),
                true,
                &Arc::new(Keypair::new()),
                &entries,
            )
            .unwrap();

        let out_path = Path::new("test_chacha_encrypt_file_many_keys_multiple_output.txt.enc");

        let samples = [0, 1, 3, 4, 5, 150];
        let mut ivecs = Vec::new();
        let mut ref_hashes: Vec<Hash> = vec![];
        for i in 0..2 {
            let mut ivec = hex!(
                "abc123abc123abc123abc123abc123abc123abababababababababababababab
                                 abc123abc123abc123abc123abc123abc123abababababababababababababab"
            );
            ivec[0] = i;
            ivecs.extend(ivec.clone().iter());
            chacha_cbc_encrypt_ledger(
                &blocktree.clone(),
                0,
                DEFAULT_SLOTS_PER_SEGMENT,
                out_path,
                &mut ivec,
            )
            .unwrap();

            ref_hashes.push(sample_file(&out_path, &samples).unwrap());
            info!(
                "ivec: {:?} hash: {:?} ivecs: {:?}",
                ivec.to_vec(),
                ref_hashes.last(),
                ivecs
            );
        }

        let hashes = chacha_cbc_encrypt_file_many_keys(
            &blocktree,
            0,
            DEFAULT_SLOTS_PER_SEGMENT,
            &mut ivecs,
            &samples,
        )
        .unwrap();

        assert_eq!(hashes, ref_hashes);

        let _ignored = remove_dir_all(&ledger_path);
        let _ignored = remove_file(out_path);
    }

    #[test]
    fn test_encrypt_file_many_keys_bad_key_length() {
        let mut keys = hex!("abc123");
        let ledger_dir = "test_encrypt_file_many_keys_bad_key_length";
        let ledger_path = get_tmp_ledger_path(ledger_dir);
        let samples = [0];
        let blocktree = Arc::new(Blocktree::open(&ledger_path).unwrap());
        assert!(chacha_cbc_encrypt_file_many_keys(
            &blocktree,
            0,
            DEFAULT_SLOTS_PER_SEGMENT,
            &mut keys,
            &samples,
        )
        .is_err());
    }
}
