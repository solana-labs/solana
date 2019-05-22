//! The `sigverify` module provides digital signature verification functions.
//! By default, signatures are verified in parallel using all available CPU
//! cores.  When `--features=cuda` is enabled, signature verification is
//! offloaded to the GPU.
//!

use crate::packet::{Packet, Packets};
use crate::result::Result;
use bincode::serialized_size;
use solana_metrics::inc_new_counter_debug;
use solana_sdk::message::MessageHeader;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::short_vec::decode_len;
use solana_sdk::signature::Signature;
#[cfg(test)]
use solana_sdk::transaction::Transaction;
use std::mem::size_of;

type TxOffsets = (Vec<u32>, Vec<u32>, Vec<u32>, Vec<u32>, Vec<Vec<u32>>);

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
        total_packets: u32,
        total_signatures: u32,
        message_lens: *const u32,
        pubkey_offsets: *const u32,
        signature_offsets: *const u32,
        signed_message_offsets: *const u32,
        out: *mut u8, //combined length of all the items in vecs
        use_non_default_stream: u8,
    ) -> u32;

    pub fn chacha_cbc_encrypt_many_sample(
        input: *const u8,
        sha_state: *mut u8,
        in_len: usize,
        keys: *const u8,
        ivec: *mut u8,
        num_keys: u32,
        samples: *const u64,
        num_samples: u32,
        starting_block: u64,
        time_us: *mut f32,
    );

    pub fn chacha_init_sha_state(sha_state: *mut u8, num_keys: u32);
    pub fn chacha_end_sha_state(sha_state_in: *const u8, out: *mut u8, num_keys: u32);
}

#[cfg(not(feature = "cuda"))]
pub fn init() {
    // stub
}

fn verify_packet(packet: &Packet) -> u8 {
    let (sig_len, sig_start, msg_start, pubkey_start) = get_packet_offsets(packet, 0);
    let mut sig_start = sig_start as usize;
    let mut pubkey_start = pubkey_start as usize;
    let msg_start = msg_start as usize;

    if packet.meta.size <= msg_start {
        return 0;
    }

    let msg_end = packet.meta.size;
    for _ in 0..sig_len {
        let pubkey_end = pubkey_start as usize + size_of::<Pubkey>();
        let sig_end = sig_start as usize + size_of::<Signature>();

        if pubkey_end >= packet.meta.size || sig_end >= packet.meta.size {
            return 0;
        }

        let signature = Signature::new(&packet.data[sig_start..sig_end]);
        if !signature.verify(
            &packet.data[pubkey_start..pubkey_end],
            &packet.data[msg_start..msg_end],
        ) {
            return 0;
        }
        pubkey_start += size_of::<Pubkey>();
        sig_start += size_of::<Signature>();
    }
    1
}

fn batch_size(batches: &[Packets]) -> usize {
    batches.iter().map(|p| p.packets.len()).sum()
}

#[cfg(not(feature = "cuda"))]
pub fn ed25519_verify(batches: &[Packets]) -> Vec<Vec<u8>> {
    ed25519_verify_cpu(batches)
}

pub fn get_packet_offsets(packet: &Packet, current_offset: u32) -> (u32, u32, u32, u32) {
    let (sig_len, sig_size) = decode_len(&packet.data);
    let msg_start_offset = sig_size + sig_len * size_of::<Signature>();

    let (_pubkey_len, pubkey_size) = decode_len(&packet.data[msg_start_offset..]);

    let sig_start = current_offset as usize + sig_size;
    let msg_start = current_offset as usize + msg_start_offset;
    let pubkey_start =
        msg_start + serialized_size(&MessageHeader::default()).unwrap() as usize + pubkey_size;

    (
        sig_len as u32,
        sig_start as u32,
        msg_start as u32,
        pubkey_start as u32,
    )
}

pub fn generate_offsets(batches: &[Packets]) -> Result<TxOffsets> {
    let mut signature_offsets: Vec<_> = Vec::new();
    let mut pubkey_offsets: Vec<_> = Vec::new();
    let mut msg_start_offsets: Vec<_> = Vec::new();
    let mut msg_sizes: Vec<_> = Vec::new();
    let mut current_packet = 0;
    let mut v_sig_lens = Vec::new();
    batches.iter().for_each(|p| {
        let mut sig_lens = Vec::new();
        p.packets.iter().for_each(|packet| {
            let current_offset = current_packet as u32 * size_of::<Packet>() as u32;

            let (sig_len, sig_start, msg_start_offset, pubkey_offset) =
                get_packet_offsets(packet, current_offset);
            let mut pubkey_offset = pubkey_offset;

            sig_lens.push(sig_len);

            trace!("pubkey_offset: {}", pubkey_offset);
            let mut sig_offset = sig_start;
            for _ in 0..sig_len {
                signature_offsets.push(sig_offset);
                sig_offset += size_of::<Signature>() as u32;

                pubkey_offsets.push(pubkey_offset);
                pubkey_offset += size_of::<Pubkey>() as u32;

                msg_start_offsets.push(msg_start_offset);

                msg_sizes.push(current_offset + (packet.meta.size as u32) - msg_start_offset);
            }
            current_packet += 1;
        });
        v_sig_lens.push(sig_lens);
    });
    Ok((
        signature_offsets,
        pubkey_offsets,
        msg_start_offsets,
        msg_sizes,
        v_sig_lens,
    ))
}

pub fn ed25519_verify_cpu(batches: &[Packets]) -> Vec<Vec<u8>> {
    use rayon::prelude::*;
    let count = batch_size(batches);
    debug!("CPU ECDSA for {}", batch_size(batches));
    let rv = batches
        .into_par_iter()
        .map(|p| p.packets.par_iter().map(verify_packet).collect())
        .collect();
    inc_new_counter_debug!("ed25519_verify_cpu", count);
    rv
}

pub fn ed25519_verify_disabled(batches: &[Packets]) -> Vec<Vec<u8>> {
    use rayon::prelude::*;
    let count = batch_size(batches);
    debug!("disabled ECDSA for {}", batch_size(batches));
    let rv = batches
        .into_par_iter()
        .map(|p| vec![1u8; p.packets.len()])
        .collect();
    inc_new_counter_debug!("ed25519_verify_disabled", count);
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
pub fn ed25519_verify(batches: &[Packets]) -> Vec<Vec<u8>> {
    use crate::packet::PACKET_DATA_SIZE;
    let count = batch_size(batches);

    // micro-benchmarks show GPU time for smallest batch around 15-20ms
    // and CPU speed for 64-128 sigverifies around 10-20ms. 64 is a nice
    // power-of-two number around that accounting for the fact that the CPU
    // may be busy doing other things while being a real fullnode
    // TODO: dynamically adjust this crossover
    if count < 64 {
        return ed25519_verify_cpu(batches);
    }

    let (signature_offsets, pubkey_offsets, msg_start_offsets, msg_sizes, sig_lens) =
        generate_offsets(batches).unwrap();

    debug!("CUDA ECDSA for {}", batch_size(batches));
    let mut out = Vec::new();
    let mut elems = Vec::new();
    let mut rvs = Vec::new();

    let mut num_packets = 0;
    for p in batches {
        elems.push(Elems {
            elems: p.packets.as_ptr(),
            num: p.packets.len() as u32,
        });
        let mut v = Vec::new();
        v.resize(p.packets.len(), 0);
        rvs.push(v);
        num_packets += p.packets.len();
    }
    out.resize(signature_offsets.len(), 0);
    trace!("Starting verify num packets: {}", num_packets);
    trace!("elem len: {}", elems.len() as u32);
    trace!("packet sizeof: {}", size_of::<Packet>() as u32);
    trace!("len offset: {}", PACKET_DATA_SIZE as u32);
    const USE_NON_DEFAULT_STREAM: u8 = 1;
    unsafe {
        let res = ed25519_verify_many(
            elems.as_ptr(),
            elems.len() as u32,
            size_of::<Packet>() as u32,
            num_packets as u32,
            signature_offsets.len() as u32,
            msg_sizes.as_ptr(),
            pubkey_offsets.as_ptr(),
            signature_offsets.as_ptr(),
            msg_start_offsets.as_ptr(),
            out.as_mut_ptr(),
            USE_NON_DEFAULT_STREAM,
        );
        if res != 0 {
            trace!("RETURN!!!: {}", res);
        }
    }
    trace!("done verify");
    let mut num = 0;
    for (vs, sig_vs) in rvs.iter_mut().zip(sig_lens.iter()) {
        for (v, sig_v) in vs.iter_mut().zip(sig_vs.iter()) {
            let mut vout = 1;
            for _ in 0..*sig_v {
                if 0 == out[num] {
                    vout = 0;
                }
                num += 1;
            }
            *v = vout;
            if *v != 0 {
                trace!("VERIFIED PACKET!!!!!");
            }
        }
    }
    inc_new_counter_debug!("ed25519_verify_gpu", count);
    rvs
}

#[cfg(test)]
pub fn make_packet_from_transaction(tx: Transaction) -> Packet {
    use bincode::serialize;

    let tx_bytes = serialize(&tx).unwrap();
    let mut packet = Packet::default();
    packet.meta.size = tx_bytes.len();
    packet.data[..packet.meta.size].copy_from_slice(&tx_bytes);
    return packet;
}

#[cfg(test)]
mod tests {
    use crate::packet::{Packet, Packets};
    use crate::sigverify;
    use crate::test_tx::{test_multisig_tx, test_tx};
    use bincode::{deserialize, serialize};
    use solana_sdk::transaction::Transaction;

    const SIG_OFFSET: usize = 1;

    pub fn memfind<A: Eq>(a: &[A], b: &[A]) -> Option<usize> {
        assert!(a.len() >= b.len());
        let end = a.len() - b.len() + 1;
        for i in 0..end {
            if a[i..i + b.len()] == b[..] {
                return Some(i);
            }
        }
        None
    }

    #[test]
    fn test_layout() {
        let tx = test_tx();
        let tx_bytes = serialize(&tx).unwrap();
        let packet = serialize(&tx).unwrap();
        assert_matches!(memfind(&packet, &tx_bytes), Some(0));
        assert_matches!(memfind(&packet, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]), None);
    }

    #[test]
    fn test_system_transaction_layout() {
        let tx = test_tx();
        let tx_bytes = serialize(&tx).unwrap();
        let message_data = tx.message_data();
        let packet = sigverify::make_packet_from_transaction(tx.clone());

        let (sig_len, sig_start, msg_start_offset, pubkey_offset) =
            sigverify::get_packet_offsets(&packet, 0);

        assert_eq!(
            memfind(&tx_bytes, &tx.signatures[0].as_ref()),
            Some(SIG_OFFSET)
        );
        assert_eq!(
            memfind(&tx_bytes, &tx.message().account_keys[0].as_ref()),
            Some(pubkey_offset as usize)
        );
        assert_eq!(
            memfind(&tx_bytes, &message_data),
            Some(msg_start_offset as usize)
        );
        assert_eq!(
            memfind(&tx_bytes, &tx.signatures[0].as_ref()),
            Some(sig_start as usize)
        );
        assert_eq!(sig_len, 1);
    }

    #[test]
    fn test_system_transaction_data_layout() {
        use crate::packet::PACKET_DATA_SIZE;
        let mut tx0 = test_tx();
        tx0.message.instructions[0].data = vec![1, 2, 3];
        let message0a = tx0.message_data();
        let tx_bytes = serialize(&tx0).unwrap();
        assert!(tx_bytes.len() < PACKET_DATA_SIZE);
        assert_eq!(
            memfind(&tx_bytes, &tx0.signatures[0].as_ref()),
            Some(SIG_OFFSET)
        );
        let tx1 = deserialize(&tx_bytes).unwrap();
        assert_eq!(tx0, tx1);
        assert_eq!(tx1.message().instructions[0].data, vec![1, 2, 3]);

        tx0.message.instructions[0].data = vec![1, 2, 4];
        let message0b = tx0.message_data();
        assert_ne!(message0a, message0b);
    }

    // Just like get_packet_offsets, but not returning redundant information.
    fn get_packet_offsets_from_tx(tx: Transaction, current_offset: u32) -> (u32, u32, u32, u32) {
        let packet = sigverify::make_packet_from_transaction(tx);
        let (sig_len, sig_start, msg_start_offset, pubkey_offset) =
            sigverify::get_packet_offsets(&packet, current_offset);
        (
            sig_len,
            sig_start - current_offset,
            msg_start_offset - sig_start,
            pubkey_offset - msg_start_offset,
        )
    }

    #[test]
    fn test_get_packet_offsets() {
        assert_eq!(get_packet_offsets_from_tx(test_tx(), 0), (1, 1, 64, 4));
        assert_eq!(get_packet_offsets_from_tx(test_tx(), 100), (1, 1, 64, 4));

        // Ensure we're not indexing packet by the `current_offset` parameter.
        assert_eq!(
            get_packet_offsets_from_tx(test_tx(), 1_000_000),
            (1, 1, 64, 4)
        );

        // Ensure we're returning sig_len, not sig_size.
        assert_eq!(
            get_packet_offsets_from_tx(test_multisig_tx(), 0),
            (2, 1, 128, 4)
        );
    }

    fn generate_packet_vec(
        packet: &Packet,
        num_packets_per_batch: usize,
        num_batches: usize,
    ) -> Vec<Packets> {
        // generate packet vector
        let batches: Vec<_> = (0..num_batches)
            .map(|_| {
                let mut packets = Packets::default();
                packets.packets.resize(0, Packet::default());
                for _ in 0..num_packets_per_batch {
                    packets.packets.push(packet.clone());
                }
                assert_eq!(packets.packets.len(), num_packets_per_batch);
                packets
            })
            .collect();
        assert_eq!(batches.len(), num_batches);

        batches
    }

    fn test_verify_n(n: usize, modify_data: bool) {
        let tx = test_tx();
        let mut packet = sigverify::make_packet_from_transaction(tx);

        // jumble some data to test failure
        if modify_data {
            packet.data[20] = packet.data[20].wrapping_add(10);
        }

        let batches = generate_packet_vec(&packet, n, 2);

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
    fn test_verify_multisig() {
        solana_logger::setup();

        let tx = test_multisig_tx();
        let mut packet = sigverify::make_packet_from_transaction(tx);

        let n = 4;
        let num_batches = 3;
        let mut batches = generate_packet_vec(&packet, n, num_batches);

        packet.data[40] = packet.data[40].wrapping_add(8);

        batches[0].packets.push(packet);

        // verify packets
        let ans = sigverify::ed25519_verify(&batches);

        // check result
        let ref_ans = 1u8;
        let mut ref_vec = vec![vec![ref_ans; n]; num_batches];
        ref_vec[0].push(0u8);
        assert_eq!(ans, ref_vec);
    }

    #[test]
    fn test_verify_fail() {
        test_verify_n(5, true);
    }
}
