#![feature(test)]
extern crate bincode;
extern crate rand;
extern crate rayon;
extern crate solana;
extern crate test;

use rand::{thread_rng, Rng};
use rayon::prelude::*;
use solana::bank::Bank;
use solana::banking_stage::{BankingStage, NUM_THREADS};
use solana::entry::Entry;
use solana::mint::Mint;
use solana::packet::{to_packets_chunked, PacketRecycler};
use solana::signature::{KeypairUtil, Pubkey, Signature};
use solana::system_transaction::SystemTransaction;
use solana::transaction::Transaction;
use std::iter;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;
use test::Bencher;

fn check_txs(receiver: &Receiver<Vec<Entry>>, ref_tx_count: usize) {
    let mut total = 0;
    loop {
        let entries = receiver.recv_timeout(Duration::new(1, 0));
        if let Ok(entries) = entries {
            for entry in &entries {
                total += entry.transactions.len();
            }
        } else {
            break;
        }
        if total >= ref_tx_count {
            break;
        }
    }
    assert_eq!(total, ref_tx_count);
}

#[bench]
fn bench_banking_stage_multi_accounts(bencher: &mut Bencher) {
    let txes = 1000 * NUM_THREADS;
    let mint_total = 1_000_000_000_000;
    let mint = Mint::new(mint_total);

    let (verified_sender, verified_receiver) = channel();
    let packet_recycler = PacketRecycler::default();
    let bank = Arc::new(Bank::new(&mint));
    let dummy = Transaction::system_move(
        &mint.keypair(),
        mint.keypair().pubkey(),
        1,
        mint.last_id(),
        0,
    );
    let transactions: Vec<_> = (0..txes)
        .into_par_iter()
        .map(|_| {
            let mut new = dummy.clone();
            let from: Vec<u8> = (0..64).map(|_| thread_rng().gen()).collect();
            let to: Vec<u8> = (0..64).map(|_| thread_rng().gen()).collect();
            let sig: Vec<u8> = (0..64).map(|_| thread_rng().gen()).collect();
            new.keys[0] = Pubkey::new(&from[0..32]);
            new.keys[1] = Pubkey::new(&to[0..32]);
            new.signature = Signature::new(&sig[0..64]);
            new
        }).collect();
    // fund all the accounts
    transactions.iter().for_each(|tx| {
        let fund = Transaction::system_move(
            &mint.keypair(),
            tx.keys[0],
            mint_total / txes as i64,
            mint.last_id(),
            0,
        );
        assert!(bank.process_transaction(&fund).is_ok());
    });
    //sanity check, make sure all the transactions can execute sequentially
    transactions.iter().for_each(|tx| {
        let res = bank.process_transaction(&tx);
        assert!(res.is_ok(), "sanity test transactions");
    });
    bank.clear_signatures();
    //sanity check, make sure all the transactions can execute in parallel
    let res = bank.process_transactions(&transactions);
    for r in res {
        assert!(r.is_ok(), "sanity parallel execution");
    }
    bank.clear_signatures();
    let verified: Vec<_> = to_packets_chunked(&packet_recycler, &transactions.clone(), 192)
        .into_iter()
        .map(|x| {
            let len = x.read().packets.len();
            (x, iter::repeat(1).take(len).collect())
        }).collect();
    let (_stage, signal_receiver) =
        BankingStage::new(bank.clone(), verified_receiver, Default::default());
    bencher.iter(move || {
        for v in verified.chunks(verified.len() / NUM_THREADS) {
            verified_sender.send(v.to_vec()).unwrap();
        }
        check_txs(&signal_receiver, txes);
        bank.clear_signatures();
        // make sure the tx last id is still registered
        bank.register_entry_id(&mint.last_id());
    });
}
