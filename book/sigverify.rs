//! The `sigverify` module provides digital signature verification functions.
//! By default, signatures are verified in parallel using all available CPU
//! cores.  When `--features=cuda` is enabled, signature verification is
//! offloaded to the GPU.
//!

use byteorder::{LittleEndian, ReadBytesExt};
use counter::Counter;
use log::Level;
use packet::{Packet, SharedPackets};
use result::Result;
use signature::Signature;
use solana_sdk::pubkey::Pubkey;
use std::io;
use std::mem::size_of;
use std::sync::atomic::AtomicUsize;
#[cfg(test)]
use transaction::Transaction;

pub const TX_OFFSET: usize = 0;

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
    use ring::signature;
    use signature::Signature;
    use solana_sdk::pubkey::Pubkey;
    use untrusted;

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

        if signature::verify(
            &signature::ED25519,
            untrusted::Input::from(&packet.data[pubkey_start..pubkey_end]),
            untrusted::Input::from(&packet.data[msg_start..msg_end]),
            untrusted::Input::from(&packet.data[sig_start..sig_end]),
        ).is_err()
        {
            return 0;
        }
        pubkey_start += size_of::<Pubkey>();
        sig_start += size_of::<Signature>();
    }
    1
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

pub fn get_packet_offsets(packet: &Packet, current_offset: u32) -> (u32, u32, u32, u32) {
    // Read in u64 as the size of signatures array
    let mut rdr = io::Cursor::new(&packet.data[TX_OFFSET..size_of::<u64>()]);
    let sig_len = rdr.read_u64::<LittleEndian>().unwrap() as u32;

    let msg_start_offset =
        current_offset + size_of::<u64>() as u32 + sig_len * size_of::<Signature>() as u32;
    let pubkey_offset = msg_start_offset + size_of::<u64>() as u32;

    let sig_start = TX_OFFSET as u32 + size_of::<u64>() as u32;

    (sig_len, sig_start, msg_start_offset, pubkey_offset)
}

pub fn generate_offsets(batches: &[SharedPackets]) -> Result<TxOffsets> {
    let mut signature_offsets: Vec<_> = Vec::new();
    let mut pubkey_offsets: Vec<_> = Vec::new();
    let mut msg_start_offsets: Vec<_> = Vec::new();
    let mut msg_sizes: Vec<_> = Vec::new();
    let mut current_packet = 0;
    let mut v_sig_lens = Vec::new();
    batches.into_iter().for_each(|p| {
        let mut sig_lens = Vec::new();
        p.read().unwrap().packets.iter().for_each(|packet| {
            let current_offset = current_packet as u32 * size_of::<Packet>() as u32;

            let (sig_len, _sig_start, msg_start_offset, pubkey_offset) =
                get_packet_offsets(packet, current_offset);
            let mut pubkey_offset = pubkey_offset;

            sig_lens.push(sig_len);

            trace!("pubkey_offset: {}", pubkey_offset);
            let mut sig_offset = current_offset + size_of::<u64>() as u32;
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

    let (signature_offsets, pubkey_offsets, msg_start_offsets, msg_sizes, sig_lens) =
        generate_offsets(batches).unwrap();

    info!("CUDA ECDSA for {}", batch_size(batches));
    let mut out = Vec::new();
    let mut elems = Vec::new();
    let mut locks = Vec::new();
    let mut rvs = Vec::new();

    for packets in batches {
        locks.push(packets.read().unwrap());
    }
    let mut num_packets = 0;
    for p in locks {
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
        );
        if res != 0 {
            trace!("RETURN!!!: {}", res);
        }
    }
    trace!("done verify");
    let mut num = 0;
    for (vs, sig_vs) in rvs.iter_mut().zip(sig_lens.iter()) {
        for (mut v, sig_v) in vs.iter_mut().zip(sig_vs.iter()) {
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
    inc_new_counter_info!("ed25519_verify_gpu", count);
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
    use bincode::serialize;
    use budget_program::BudgetState;
    use packet::{Packet, SharedPackets};
    use signature::{Keypair, KeypairUtil};
    use sigverify;
    use solana_sdk::hash::Hash;
    use solana_sdk::system_instruction::SystemInstruction;
    use system_program::SystemProgram;
    use system_transaction::{memfind, test_tx};
    use transaction;
    use transaction::Transaction;

    #[test]
    fn test_layout() {
        let tx = test_tx();
        let tx_bytes = serialize(&tx).unwrap();
        let packet = serialize(&tx).unwrap();
        assert_matches!(memfind(&packet, &tx_bytes), Some(sigverify::TX_OFFSET));
        assert_matches!(memfind(&packet, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]), None);
    }

    #[test]
    fn test_get_packet_offsets() {
        let tx = test_tx();
        let packet = sigverify::make_packet_from_transaction(tx);
        let (sig_len, sig_start, msg_start_offset, pubkey_offset) =
            sigverify::get_packet_offsets(&packet, 0);
        assert_eq!(sig_len, 1);
        assert_eq!(sig_start, 8);
        assert_eq!(msg_start_offset, 72);
        assert_eq!(pubkey_offset, 80);
    }

    fn generate_packet_vec(
        packet: &Packet,
        num_packets_per_batch: usize,
        num_batches: usize,
    ) -> Vec<SharedPackets> {
        // generate packet vector
        let batches: Vec<_> = (0..num_batches)
            .map(|_| {
                let packets = SharedPackets::default();
                packets
                    .write()
                    .unwrap()
                    .packets
                    .resize(0, Default::default());
                for _ in 0..num_packets_per_batch {
                    packets.write().unwrap().packets.push(packet.clone());
                }
                assert_eq!(packets.read().unwrap().packets.len(), num_packets_per_batch);
                packets
            }).collect();
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
    fn test_verify_multi_sig() {
        use logger;
        logger::setup();
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let keypairs = vec![&keypair0, &keypair1];
        let tokens = 5;
        let fee = 2;
        let last_id = Hash::default();

        let keys = vec![keypair0.pubkey(), keypair1.pubkey()];

        let system_instruction = SystemInstruction::Move { tokens };

        let program_ids = vec![SystemProgram::id(), BudgetState::id()];

        let instructions = vec![transaction::Instruction::new(
            0,
            &system_instruction,
            vec![0, 1],
        )];

        let tx = Transaction::new_with_instructions(
            &keypairs,
            &keys,
            last_id,
            fee,
            program_ids,
            instructions,
        );

        let mut packet = sigverify::make_packet_from_transaction(tx);

        let n = 4;
        let num_batches = 3;
        let batches = generate_packet_vec(&packet, n, num_batches);

        packet.data[40] = packet.data[40].wrapping_add(8);

        batches[0].write().unwrap().packets.push(packet);

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
