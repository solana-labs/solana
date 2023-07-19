#![allow(clippy::arithmetic_side_effects)]
#![feature(test)]
extern crate test;

use {
    bincode::{deserialize, serialize},
    solana_ledger::{
        blockstore::Blockstore,
        blockstore_db::{columns as cf, LedgerColumn},
        get_tmp_ledger_path,
    },
    solana_runtime::bank::RewardType,
    solana_sdk::{clock::Slot, pubkey},
    solana_transaction_status::{Reward, Rewards},
    std::path::Path,
    test::Bencher,
};

fn create_rewards() -> Rewards {
    (0..100)
        .map(|i| Reward {
            pubkey: pubkey::new_rand().to_string(),
            lamports: 42 + i,
            post_balance: std::u64::MAX,
            reward_type: Some(RewardType::Fee),
            commission: None,
        })
        .collect()
}

fn write_bincode_rewards(rewards_cf: &LedgerColumn<cf::Rewards>, slot: Slot, rewards: Rewards) {
    let data = serialize(&rewards).unwrap();
    rewards_cf.put_bytes(slot, &data).unwrap();
}

fn write_protobuf_rewards(rewards_cf: &LedgerColumn<cf::Rewards>, slot: Slot, rewards: Rewards) {
    let rewards = rewards.into();
    rewards_cf.put_protobuf(slot, &rewards).unwrap();
}

fn read_bincode_rewards(rewards_cf: &LedgerColumn<cf::Rewards>, slot: Slot) -> Option<Rewards> {
    rewards_cf
        .get_bytes(slot)
        .unwrap()
        .map(|data| deserialize::<Rewards>(&data).unwrap())
}

fn read_protobuf_rewards(rewards_cf: &LedgerColumn<cf::Rewards>, slot: Slot) -> Option<Rewards> {
    rewards_cf.get_protobuf(slot).unwrap().map(|r| r.into())
}

fn bench_write_rewards<F>(bench: &mut Bencher, ledger_path: &Path, write_method: F)
where
    F: Fn(&LedgerColumn<cf::Rewards>, Slot, Rewards),
{
    let blockstore =
        Blockstore::open(ledger_path).expect("Expected to be able to open database ledger");
    let rewards = create_rewards();
    let mut slot = 0;
    let rewards_cf = blockstore.db().column::<cf::Rewards>();
    bench.iter(move || {
        write_method(&rewards_cf, slot, rewards.clone());
        slot += 1;
    });
    Blockstore::destroy(ledger_path).expect("Expected successful database destruction");
}

fn bench_read_rewards<F, G>(
    bench: &mut Bencher,
    ledger_path: &Path,
    write_method: F,
    read_method: G,
) where
    F: Fn(&LedgerColumn<cf::Rewards>, Slot, Rewards),
    G: Fn(&LedgerColumn<cf::Rewards>, Slot) -> Option<Rewards>,
{
    let blockstore =
        Blockstore::open(ledger_path).expect("Expected to be able to open database ledger");
    let rewards = create_rewards();
    let slot = 1;
    let rewards_cf = blockstore.db().column::<cf::Rewards>();
    write_method(&rewards_cf, slot, rewards);
    bench.iter(move || read_method(&rewards_cf, slot));
    Blockstore::destroy(ledger_path).expect("Expected successful database destruction");
}

#[bench]
fn bench_serialize_write_bincode(bencher: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path!();
    bench_write_rewards(bencher, &ledger_path, write_bincode_rewards);
}

#[bench]
fn bench_serialize_write_protobuf(bencher: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path!();
    bench_write_rewards(bencher, &ledger_path, write_protobuf_rewards);
}

#[bench]
fn bench_read_bincode(bencher: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path!();
    bench_read_rewards(
        bencher,
        &ledger_path,
        write_bincode_rewards,
        read_bincode_rewards,
    );
}

#[bench]
fn bench_read_protobuf(bencher: &mut Bencher) {
    let ledger_path = get_tmp_ledger_path!();
    bench_read_rewards(
        bencher,
        &ledger_path,
        write_protobuf_rewards,
        read_protobuf_rewards,
    );
}
