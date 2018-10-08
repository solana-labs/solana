//! The `sigverify` module provides digital signature verification functions.
//! By default, signatures are verified in parallel using all available CPU
//! cores.  When `--features=cuda` is enabled, signature verification is
//! offloaded to the GPU.
//!

use counter::Counter;
use log::Level;
use packet::{Packet, SharedPackets};
use std::mem::size_of;
use std::sync::atomic::AtomicUsize;
use transaction::{PUB_KEY_OFFSET, SIGNED_DATA_OFFSET, SIG_OFFSET};

pub const TX_OFFSET: usize = 0;

#[cfg(feature = "cuda")]
#[repr(C)]
struct Elems {
    elems: *const Packet,
    num: u32,
}

#[cfg(feature = "cuda")]
#[link(name = "cuda-crypt")]
extern "C" {
    fn ed25519_init() -> bool;
    fn ed25519_set_verbose(val: bool);
    fn ed25519_verify_many(
        vecs: *const Elems,
        num: u32,          //number of vecs
        message_size: u32, //size of each element inside the elems field of the vec
        pubkey_offset: u32,
        signature_offset: u32,
        signed_message_offset: u32,
        signed_message_len_offset: u32,
        out: *mut u8, //combined length of all the items in vecs
    ) -> u32;

    pub fn chacha_cbc_encrypt_many_sample(
        input: *const u8,
        output: *mut u8,
        in_len: usize,
        keys: *const u8,
        ivec: *mut u8,
        num_keys: u32,
        samples: *const u64,
        num_samples: u32,
        starting_block: u64,
        time_us: *mut f32,
    );
}

#[cfg(not(feature = "cuda"))]
pub fn init() {
    // stub
}

fn verify_packet(packet: &Packet) -> u8 {
    use ring::signature;
    use signature::Signature;
    use solana_program_interface::pubkey::Pubkey;
    use untrusted;

    let msg_start = TX_OFFSET + SIGNED_DATA_OFFSET;
    let sig_start = TX_OFFSET + SIG_OFFSET;
    let sig_end = sig_start + size_of::<Signature>();
    let pubkey_start = TX_OFFSET + PUB_KEY_OFFSET;
    let pubkey_end = pubkey_start + size_of::<Pubkey>();

    if packet.meta.size <= msg_start {
        return 0;
    }

    let msg_end = packet.meta.size;
    signature::verify(
        &signature::ED25519,
        untrusted::Input::from(&packet.data[pubkey_start..pubkey_end]),
        untrusted::Input::from(&packet.data[msg_start..msg_end]),
        untrusted::Input::from(&packet.data[sig_start..sig_end]),
    ).is_ok() as u8
}

fn verify_packet_disabled(_packet: &Packet) -> u8 {
    warn!("signature verification is disabled");
    1
}

fn batch_size(batches: &[SharedPackets]) -> usize {
    batches
        .iter()
        .map(|p| p.read().unwrap().packets.len())
        .sum()
}

#[cfg(not(feature = "cuda"))]
pub fn ed25519_verify(batches: &[SharedPackets]) -> Vec<Vec<u8>> {
    ed25519_verify_cpu(batches)
}

pub fn ed25519_verify_cpu(batches: &[SharedPackets]) -> Vec<Vec<u8>> {
    use rayon::prelude::*;
    let count = batch_size(batches);
    info!("CPU ECDSA for {}", batch_size(batches));
    let rv = batches
        .into_par_iter()
        .map(|p| {
            p.read()
                .unwrap()
                .packets
                .par_iter()
                .map(verify_packet)
                .collect()
        }).collect();
    inc_new_counter_info!("ed25519_verify_cpu", count);
    rv
}

pub fn ed25519_verify_disabled(batches: &[SharedPackets]) -> Vec<Vec<u8>> {
    use rayon::prelude::*;
    let count = batch_size(batches);
    info!("disabled ECDSA for {}", batch_size(batches));
    let rv = batches
        .into_par_iter()
        .map(|p| {
            p.read()
                .unwrap()
                .packets
                .par_iter()
                .map(verify_packet_disabled)
                .collect()
        }).collect();
    inc_new_counter_info!("ed25519_verify_disabled", count);
    rv
}

#[cfg(feature = "cuda")]
pub fn init() {
    unsafe {
        ed25519_set_verbose(true);
        if !ed25519_init() {
            panic!("ed25519_init() failed");
        }
        ed25519_set_verbose(false);
    }
}

#[cfg(feature = "cuda")]
pub fn ed25519_verify(batches: &[SharedPackets]) -> Vec<Vec<u8>> {
    use packet::PACKET_DATA_SIZE;
    let count = batch_size(batches);

    // micro-benchmarks show GPU time for smallest batch around 15-20ms
    // and CPU speed for 64-128 sigverifies around 10-20ms. 64 is a nice
    // power-of-two number around that accounting for the fact that the CPU
    // may be busy doing other things while being a real fullnode
    // TODO: dynamically adjust this crossover
    if count < 64 {
        return ed25519_verify_cpu(batches);
    }

    info!("CUDA ECDSA for {}", batch_size(batches));
    let mut out = Vec::new();
    let mut elems = Vec::new();
    let mut locks = Vec::new();
    let mut rvs = Vec::new();

    for packets in batches {
        locks.push(packets.read().unwrap());
    }
    let mut num = 0;
    for p in locks {
        elems.push(Elems {
            elems: p.packets.as_ptr(),
            num: p.packets.len() as u32,
        });
        let mut v = Vec::new();
        v.resize(p.packets.len(), 0);
        rvs.push(v);
        num += p.packets.len();
    }
    out.resize(num, 0);
    trace!("Starting verify num packets: {}", num);
    trace!("elem len: {}", elems.len() as u32);
    trace!("packet sizeof: {}", size_of::<Packet>() as u32);
    trace!("pubkey: {}", (TX_OFFSET + PUB_KEY_OFFSET) as u32);
    trace!("signature offset: {}", (TX_OFFSET + SIG_OFFSET) as u32);
    trace!("sign data: {}", (TX_OFFSET + SIGNED_DATA_OFFSET) as u32);
    trace!("len offset: {}", PACKET_DATA_SIZE as u32);
    unsafe {
        let res = ed25519_verify_many(
            elems.as_ptr(),
            elems.len() as u32,
            size_of::<Packet>() as u32,
            (TX_OFFSET + PUB_KEY_OFFSET) as u32,
            (TX_OFFSET + SIG_OFFSET) as u32,
            (TX_OFFSET + SIGNED_DATA_OFFSET) as u32,
            PACKET_DATA_SIZE as u32,
            out.as_mut_ptr(),
        );
        if res != 0 {
            trace!("RETURN!!!: {}", res);
        }
    }
    trace!("done verify");
    let mut num = 0;
    for vs in rvs.iter_mut() {
        for mut v in vs.iter_mut() {
            *v = out[num];
            if *v != 0 {
                trace!("VERIFIED PACKET!!!!!");
            }
            num += 1;
        }
    }
    inc_new_counter_info!("ed25519_verify_gpu", count);
    rvs
}

#[cfg(test)]
mod tests {
    use bincode::serialize;
    use packet::{Packet, SharedPackets};
    use sigverify;
    use system_transaction::{memfind, test_tx};
    use transaction::Transaction;

    #[test]
    fn test_layout() {
        let tx = test_tx();
        let tx_bytes = serialize(&tx).unwrap();
        let packet = serialize(&tx).unwrap();
        assert_matches!(memfind(&packet, &tx_bytes), Some(sigverify::TX_OFFSET));
        assert_matches!(memfind(&packet, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]), None);
    }

    fn make_packet_from_transaction(tx: Transaction) -> Packet {
        let tx_bytes = serialize(&tx).unwrap();
        let mut packet = Packet::default();
        packet.meta.size = tx_bytes.len();
        packet.data[..packet.meta.size].copy_from_slice(&tx_bytes);
        return packet;
    }

    fn test_verify_n(n: usize, modify_data: bool) {
        let tx = test_tx();
        let mut packet = make_packet_from_transaction(tx);

        // jumble some data to test failure
        if modify_data {
            packet.data[20] = packet.data[20].wrapping_add(10);
        }

        // generate packet vector
        let batches: Vec<_> = (0..2)
            .map(|_| {
                let packets = SharedPackets::default();
                packets
                    .write()
                    .unwrap()
                    .packets
                    .resize(0, Default::default());
                for _ in 0..n {
                    packets.write().unwrap().packets.push(packet.clone());
                }
                assert_eq!(packets.read().unwrap().packets.len(), n);
                packets
            }).collect();
        assert_eq!(batches.len(), 2);

        // verify packets
        let ans = sigverify::ed25519_verify(&batches);

        // check result
        let ref_ans = if modify_data { 0u8 } else { 1u8 };
        assert_eq!(ans, vec![vec![ref_ans; n], vec![ref_ans; n]]);
    }

    #[test]
    fn test_verify_zero() {
        test_verify_n(0, false);
    }

    #[test]
    fn test_verify_one() {
        test_verify_n(1, false);
    }

    #[test]
    fn test_verify_seventy_one() {
        test_verify_n(71, false);
    }

    #[test]
    fn test_verify_fail() {
        test_verify_n(5, true);
    }
}
