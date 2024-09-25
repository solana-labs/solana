use {
    criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput},
    solana_runtime_transaction::signature_details::get_precompile_signature_details,
    solana_sdk::{instruction::CompiledInstruction, pubkey::Pubkey},
    solana_svm_transaction::instruction::SVMInstruction,
};

fn bench_get_signature_details_empty(c: &mut Criterion) {
    let instructions = std::iter::empty();

    c.benchmark_group("bench_get_signature_details_empty")
        .throughput(Throughput::Elements(1))
        .bench_function("0 instructions", |bencher| {
            bencher.iter(|| {
                let instructions = black_box(instructions.clone());
                let _ = get_precompile_signature_details(instructions);
            });
        });
}

fn bench_get_signature_details_no_sigs_unique(c: &mut Criterion) {
    let program_ids = vec![Pubkey::new_unique(); 32];
    for num_instructions in [4, 32] {
        let instructions = (0..num_instructions)
            .map(|i| {
                let program_id = &program_ids[i];
                (
                    program_id,
                    CompiledInstruction {
                        program_id_index: i as u8,
                        accounts: vec![],
                        data: vec![],
                    },
                )
            })
            .collect::<Vec<_>>();

        c.benchmark_group("bench_get_signature_details_no_sigs_unique")
            .throughput(Throughput::Elements(1))
            .bench_function(format!("{num_instructions} instructions"), |bencher| {
                bencher.iter(|| {
                    let instructions =
                        black_box(instructions.iter().map(|(program_id, instruction)| {
                            (*program_id, SVMInstruction::from(instruction))
                        }));
                    let _ = get_precompile_signature_details(instructions);
                });
            });
    }
}

fn bench_get_signature_details_packed_sigs(c: &mut Criterion) {
    let program_ids = [
        solana_sdk::secp256k1_program::id(),
        solana_sdk::ed25519_program::id(),
    ];
    for num_instructions in [4, 64] {
        let instructions = (0..num_instructions)
            .map(|i| {
                let index = i % 2;
                let program_id = &program_ids[index];
                (
                    program_id,
                    CompiledInstruction {
                        program_id_index: index as u8,
                        accounts: vec![],
                        data: vec![4], // some dummy number of signatures
                    },
                )
            })
            .collect::<Vec<_>>();

        c.benchmark_group("bench_get_signature_details_packed_sigs")
            .throughput(Throughput::Elements(1))
            .bench_function(format!("{num_instructions} instructions"), |bencher| {
                bencher.iter(|| {
                    let instructions =
                        black_box(instructions.iter().map(|(program_id, instruction)| {
                            (*program_id, SVMInstruction::from(instruction))
                        }));
                    let _ = get_precompile_signature_details(instructions);
                });
            });
    }
}

fn bench_get_signature_details_mixed_sigs(c: &mut Criterion) {
    let program_ids = [
        solana_sdk::secp256k1_program::id(),
        solana_sdk::ed25519_program::id(),
    ]
    .into_iter()
    .chain((0..6).map(|_| Pubkey::new_unique()))
    .collect::<Vec<_>>();
    for num_instructions in [4, 64] {
        let instructions = (0..num_instructions)
            .map(|i| {
                let index = i % 8;
                let program_id = &program_ids[index];
                (
                    program_id,
                    CompiledInstruction {
                        program_id_index: index as u8,
                        accounts: vec![],
                        data: vec![4], // some dummy number of signatures
                    },
                )
            })
            .collect::<Vec<_>>();

        c.benchmark_group("bench_get_signature_details_mixed_sigs")
            .throughput(Throughput::Elements(1))
            .bench_function(format!("{num_instructions} instructions"), |bencher| {
                bencher.iter(|| {
                    let instructions =
                        black_box(instructions.iter().map(|(program_id, instruction)| {
                            (*program_id, SVMInstruction::from(instruction))
                        }));
                    let _ = get_precompile_signature_details(instructions);
                });
            });
    }
}

criterion_group!(
    benches,
    bench_get_signature_details_empty,
    bench_get_signature_details_no_sigs_unique,
    bench_get_signature_details_packed_sigs,
    bench_get_signature_details_mixed_sigs
);
criterion_main!(benches);
