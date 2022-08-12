#![allow(clippy::integer_arithmetic)]
#![feature(test)]

use {
    crossbeam_channel::unbounded,
    log::*,
    rand::{thread_rng, Rng},
    rayon::prelude::*,
    solana_core::packet_deserializer::PacketDeserializer,
    solana_measure::measure,
    solana_perf::packet::to_packet_batches,
    solana_sdk::{
        hash::Hash,
        message::Message,
        pubkey,
        signature::{Keypair, Signature, Signer},
        system_instruction, system_transaction,
        timing::duration_as_us,
        transaction::Transaction,
    },
    std::time::{Duration, Instant},
    test::Bencher,
};

extern crate test;

fn make_accounts_txs(txes: usize) -> Vec<Transaction> {
    let mint_keypair = Keypair::new();
    let hash = Hash::new_unique();
    let to_pubkey = pubkey::new_rand();
    let dummy = system_transaction::transfer(&mint_keypair, &to_pubkey, 1, hash);
    (0..txes)
        .into_par_iter()
        .map(|_| {
            let mut new = dummy.clone();
            let sig: Vec<_> = (0..64).map(|_| thread_rng().gen::<u8>()).collect();
            new.message.account_keys[0] = pubkey::new_rand();
            new.message.account_keys[1] = pubkey::new_rand();
            new.signatures = vec![Signature::new(&sig[0..64])];
            new
        })
        .collect()
}

#[allow(clippy::same_item_push)]
fn make_programs_txs(txes: usize) -> Vec<Transaction> {
    let hash = Hash::new_unique();
    let progs = 4;
    (0..txes)
        .map(|_| {
            let mut instructions = vec![];
            let from_key = Keypair::new();
            for _ in 1..progs {
                let to_key = pubkey::new_rand();
                instructions.push(system_instruction::transfer(&from_key.pubkey(), &to_key, 1));
            }
            let message = Message::new(&instructions, Some(&from_key.pubkey()));
            Transaction::new(&[&from_key], message, hash)
        })
        .collect()
}

enum TransactionType {
    Accounts,
    Programs,
}

fn bench_deserializer(bencher: &mut Bencher, tx_type: TransactionType) {
    solana_logger::setup();

    const CHUNKS: usize = 8;
    const PACKETS_PER_BATCH: usize = 192;
    let txes = 8 * PACKETS_PER_BATCH * CHUNKS;

    let (verified_sender, verified_receiver) = unbounded();
    let packet_deserializer = PacketDeserializer::new(verified_receiver);

    let transactions = match tx_type {
        TransactionType::Accounts => make_accounts_txs(txes),
        TransactionType::Programs => make_programs_txs(txes),
    };
    let verified: Vec<_> = to_packet_batches(&transactions, PACKETS_PER_BATCH);

    let mut start = 0;
    let chunk_len = verified.len() / CHUNKS;
    bencher.iter(move || {
        let now = Instant::now();

        let (_, send_time) = measure!({
            verified_sender
                .send((verified[start..start + chunk_len].to_vec(), None))
                .unwrap();
        });

        let (deserialized_packets, deserialization_time) = measure!({
            const RECV_TIMEOUT: Duration = Duration::from_millis(100);
            packet_deserializer
                .handle_received_packets(RECV_TIMEOUT, txes)
                .unwrap()
        });
        // make sure we deserialize all of the packets in the chunk
        // 100ms should be plenty to receive such a small number of packets
        assert_eq!(
            deserialized_packets.deserialized_packets.len(),
            chunk_len * PACKETS_PER_BATCH
        );

        trace!(
            "time: {} send_time_us: {} deserialize_time_us: {} deserialized: {}",
            duration_as_us(&now.elapsed()),
            send_time.as_us(),
            deserialization_time.as_us(),
            deserialized_packets.deserialized_packets.len()
        );

        start += chunk_len;
        start %= verified.len();
    });
}

#[bench]
fn bench_packet_deserializer_accounts_transactions(bencher: &mut Bencher) {
    bench_deserializer(bencher, TransactionType::Accounts);
}

#[bench]
fn bench_packet_deserializer_programs_transactions(bencher: &mut Bencher) {
    bench_deserializer(bencher, TransactionType::Programs);
}
