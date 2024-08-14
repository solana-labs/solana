use {
    agave_transaction_view::transaction_meta::TransactionMeta,
    criterion::{
        black_box, criterion_group, criterion_main, measurement::Measurement, BenchmarkGroup,
        Criterion, Throughput,
    },
    solana_sdk::{
        hash::Hash,
        instruction::Instruction,
        message::{
            v0::{self, MessageAddressTableLookup},
            Message, MessageHeader, VersionedMessage,
        },
        pubkey::Pubkey,
        signature::Keypair,
        signer::Signer,
        system_instruction,
        transaction::VersionedTransaction,
    },
};

const NUM_TRANSACTIONS: usize = 1024;

fn serialize_transactions(transactions: Vec<VersionedTransaction>) -> Vec<Vec<u8>> {
    transactions
        .into_iter()
        .map(|transaction| bincode::serialize(&transaction).unwrap())
        .collect()
}

fn bench_transactions_parsing(
    group: &mut BenchmarkGroup<impl Measurement>,
    serialized_transactions: Vec<Vec<u8>>,
) {
    // Legacy Transaction Parsing
    group.bench_function("VersionedTransaction", |c| {
        c.iter(|| {
            for bytes in serialized_transactions.iter() {
                let _ = bincode::deserialize::<VersionedTransaction>(black_box(bytes)).unwrap();
            }
        });
    });

    // New Transaction Parsing
    group.bench_function("TransactionMeta", |c| {
        c.iter(|| {
            for bytes in serialized_transactions.iter() {
                let _ = TransactionMeta::try_new(black_box(bytes)).unwrap();
            }
        });
    });
}

fn minimum_sized_transactions() -> Vec<VersionedTransaction> {
    (0..NUM_TRANSACTIONS)
        .map(|_| {
            let keypair = Keypair::new();
            VersionedTransaction::try_new(
                VersionedMessage::Legacy(Message::new_with_blockhash(
                    &[],
                    Some(&keypair.pubkey()),
                    &Hash::default(),
                )),
                &[&keypair],
            )
            .unwrap()
        })
        .collect()
}

fn simple_transfers() -> Vec<VersionedTransaction> {
    (0..NUM_TRANSACTIONS)
        .map(|_| {
            let keypair = Keypair::new();
            VersionedTransaction::try_new(
                VersionedMessage::Legacy(Message::new_with_blockhash(
                    &[system_instruction::transfer(
                        &keypair.pubkey(),
                        &Pubkey::new_unique(),
                        1,
                    )],
                    Some(&keypair.pubkey()),
                    &Hash::default(),
                )),
                &[&keypair],
            )
            .unwrap()
        })
        .collect()
}

fn packed_transfers() -> Vec<VersionedTransaction> {
    // Creating transfer instructions between same keys to maximize the number
    // of transfers per transaction. We can fit up to 60 transfers.
    const MAX_TRANSFERS_PER_TX: usize = 60;

    (0..NUM_TRANSACTIONS)
        .map(|_| {
            let keypair = Keypair::new();
            let to_pubkey = Pubkey::new_unique();
            let ixs = system_instruction::transfer_many(
                &keypair.pubkey(),
                &vec![(to_pubkey, 1); MAX_TRANSFERS_PER_TX],
            );
            VersionedTransaction::try_new(
                VersionedMessage::Legacy(Message::new(&ixs, Some(&keypair.pubkey()))),
                &[&keypair],
            )
            .unwrap()
        })
        .collect()
}

fn packed_noops() -> Vec<VersionedTransaction> {
    // Creating noop instructions to maximize the number of instructions per
    // transaction. We can fit up to 355 noops.
    const MAX_INSTRUCTIONS_PER_TRANSACTION: usize = 355;

    (0..NUM_TRANSACTIONS)
        .map(|_| {
            let keypair = Keypair::new();
            let program_id = Pubkey::new_unique();
            let ixs = (0..MAX_INSTRUCTIONS_PER_TRANSACTION)
                .map(|_| Instruction::new_with_bytes(program_id, &[], vec![]));
            VersionedTransaction::try_new(
                VersionedMessage::Legacy(Message::new(
                    &ixs.collect::<Vec<_>>(),
                    Some(&keypair.pubkey()),
                )),
                &[&keypair],
            )
            .unwrap()
        })
        .collect()
}

fn packed_atls() -> Vec<VersionedTransaction> {
    // Creating ATLs to maximize the number of ATLS per transaction. We can fit
    // up to 31.
    const MAX_ATLS_PER_TRANSACTION: usize = 31;

    (0..NUM_TRANSACTIONS)
        .map(|_| {
            let keypair = Keypair::new();
            VersionedTransaction::try_new(
                VersionedMessage::V0(v0::Message {
                    header: MessageHeader {
                        num_required_signatures: 1,
                        num_readonly_signed_accounts: 0,
                        num_readonly_unsigned_accounts: 0,
                    },
                    account_keys: vec![keypair.pubkey()],
                    recent_blockhash: Hash::default(),
                    instructions: vec![],
                    address_table_lookups: Vec::from_iter((0..MAX_ATLS_PER_TRANSACTION).map(
                        |_| MessageAddressTableLookup {
                            account_key: Pubkey::new_unique(),
                            writable_indexes: vec![0],
                            readonly_indexes: vec![],
                        },
                    )),
                }),
                &[&keypair],
            )
            .unwrap()
        })
        .collect()
}

fn bench_parse_min_sized_transactions(c: &mut Criterion) {
    let serialized_transactions = serialize_transactions(minimum_sized_transactions());
    let mut group = c.benchmark_group("min sized transactions");
    group.throughput(Throughput::Elements(serialized_transactions.len() as u64));
    bench_transactions_parsing(&mut group, serialized_transactions);
}

fn bench_parse_simple_transfers(c: &mut Criterion) {
    let serialized_transactions = serialize_transactions(simple_transfers());
    let mut group = c.benchmark_group("simple transfers");
    group.throughput(Throughput::Elements(serialized_transactions.len() as u64));
    bench_transactions_parsing(&mut group, serialized_transactions);
}

fn bench_parse_packed_transfers(c: &mut Criterion) {
    let serialized_transactions = serialize_transactions(packed_transfers());
    let mut group = c.benchmark_group("packed transfers");
    group.throughput(Throughput::Elements(serialized_transactions.len() as u64));
    bench_transactions_parsing(&mut group, serialized_transactions);
}

fn bench_parse_packed_noops(c: &mut Criterion) {
    let serialized_transactions = serialize_transactions(packed_noops());
    let mut group = c.benchmark_group("packed noops");
    group.throughput(Throughput::Elements(serialized_transactions.len() as u64));
    bench_transactions_parsing(&mut group, serialized_transactions);
}

fn bench_parse_packed_atls(c: &mut Criterion) {
    let serialized_transactions = serialize_transactions(packed_atls());
    let mut group = c.benchmark_group("packed atls");
    group.throughput(Throughput::Elements(serialized_transactions.len() as u64));
    bench_transactions_parsing(&mut group, serialized_transactions);
}

criterion_group!(
    benches,
    bench_parse_min_sized_transactions,
    bench_parse_simple_transfers,
    bench_parse_packed_transfers,
    bench_parse_packed_noops,
    bench_parse_packed_atls
);
criterion_main!(benches);
