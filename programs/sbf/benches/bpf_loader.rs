#![feature(test)]
#![cfg(feature = "sbf_c")]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::arithmetic_side_effects)]

use {
    solana_rbpf::memory_region::MemoryState,
    solana_sdk::feature_set::bpf_account_data_direct_mapping, std::slice,
};

extern crate test;

use {
    byteorder::{ByteOrder, LittleEndian, WriteBytesExt},
    solana_bpf_loader_program::{
        create_vm, serialization::serialize_parameters,
        syscalls::create_program_runtime_environment_v1,
    },
    solana_measure::measure::Measure,
    solana_program_runtime::{compute_budget::ComputeBudget, invoke_context::InvokeContext},
    solana_rbpf::{
        ebpf::MM_INPUT_START, elf::Executable, memory_region::MemoryRegion,
        verifier::RequisiteVerifier, vm::ContextObject,
    },
    solana_runtime::{
        bank::Bank,
        bank_client::BankClient,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        loader_utils::{load_program, load_program_from_file},
    },
    solana_sdk::{
        account::AccountSharedData,
        bpf_loader,
        client::SyncClient,
        entrypoint::SUCCESS,
        feature_set::FeatureSet,
        instruction::{AccountMeta, Instruction},
        message::Message,
        native_loader,
        pubkey::Pubkey,
        signature::Signer,
        transaction_context::InstructionAccount,
    },
    std::{mem, sync::Arc},
    test::Bencher,
};

const ARMSTRONG_LIMIT: u64 = 500;
const ARMSTRONG_EXPECTED: u64 = 5;

macro_rules! with_mock_invoke_context {
    ($invoke_context:ident, $loader_id:expr, $account_size:expr) => {
        let program_key = Pubkey::new_unique();
        let transaction_accounts = vec![
            (
                $loader_id,
                AccountSharedData::new(0, 0, &native_loader::id()),
            ),
            (program_key, AccountSharedData::new(1, 0, &$loader_id)),
            (
                Pubkey::new_unique(),
                AccountSharedData::new(2, $account_size, &program_key),
            ),
        ];
        let instruction_accounts = vec![InstructionAccount {
            index_in_transaction: 2,
            index_in_caller: 2,
            index_in_callee: 0,
            is_signer: false,
            is_writable: true,
        }];
        solana_program_runtime::with_mock_invoke_context!(
            $invoke_context,
            transaction_context,
            transaction_accounts
        );
        $invoke_context
            .transaction_context
            .get_next_instruction_context()
            .unwrap()
            .configure(&[0, 1], &instruction_accounts, &[]);
        $invoke_context.push().unwrap();
    };
}

#[bench]
fn bench_program_create_executable(bencher: &mut Bencher) {
    let elf = load_program_from_file("bench_alu");

    let program_runtime_environment = create_program_runtime_environment_v1(
        &FeatureSet::default(),
        &ComputeBudget::default(),
        true,
        false,
    );
    let program_runtime_environment = Arc::new(program_runtime_environment.unwrap());
    bencher.iter(|| {
        let _ = Executable::<InvokeContext>::from_elf(&elf, program_runtime_environment.clone())
            .unwrap();
    });
}

#[bench]
fn bench_program_alu(bencher: &mut Bencher) {
    let ns_per_s = 1000000000;
    let one_million = 1000000;
    let mut inner_iter = vec![];
    inner_iter
        .write_u64::<LittleEndian>(ARMSTRONG_LIMIT)
        .unwrap();
    inner_iter.write_u64::<LittleEndian>(0).unwrap();
    let elf = load_program_from_file("bench_alu");
    with_mock_invoke_context!(invoke_context, bpf_loader::id(), 10000001);

    let program_runtime_environment = create_program_runtime_environment_v1(
        &invoke_context.feature_set,
        &ComputeBudget::default(),
        true,
        false,
    );
    let mut executable =
        Executable::<InvokeContext>::from_elf(&elf, Arc::new(program_runtime_environment.unwrap()))
            .unwrap();

    executable.verify::<RequisiteVerifier>().unwrap();

    executable.jit_compile().unwrap();
    create_vm!(
        vm,
        &executable,
        vec![MemoryRegion::new_writable(&mut inner_iter, MM_INPUT_START)],
        vec![],
        &mut invoke_context,
    );
    let mut vm = vm.unwrap();

    println!("Interpreted:");
    vm.context_object_pointer
        .mock_set_remaining(std::i64::MAX as u64);
    let (instructions, result) = vm.execute_program(&executable, true);
    assert_eq!(SUCCESS, result.unwrap());
    assert_eq!(ARMSTRONG_LIMIT, LittleEndian::read_u64(&inner_iter));
    assert_eq!(
        ARMSTRONG_EXPECTED,
        LittleEndian::read_u64(&inner_iter[mem::size_of::<u64>()..])
    );

    bencher.iter(|| {
        vm.context_object_pointer
            .mock_set_remaining(std::i64::MAX as u64);
        vm.execute_program(&executable, true).1.unwrap();
    });
    let summary = bencher.bench(|_bencher| Ok(())).unwrap().unwrap();
    println!("  {:?} instructions", instructions);
    println!("  {:?} ns/iter median", summary.median as u64);
    assert!(0f64 != summary.median);
    let mips = (instructions * (ns_per_s / summary.median as u64)) / one_million;
    println!("  {:?} MIPS", mips);
    println!("{{ \"type\": \"bench\", \"name\": \"bench_program_alu_interpreted_mips\", \"median\": {:?}, \"deviation\": 0 }}", mips);

    println!("JIT to native:");
    assert_eq!(SUCCESS, vm.execute_program(&executable, false).1.unwrap());
    assert_eq!(ARMSTRONG_LIMIT, LittleEndian::read_u64(&inner_iter));
    assert_eq!(
        ARMSTRONG_EXPECTED,
        LittleEndian::read_u64(&inner_iter[mem::size_of::<u64>()..])
    );

    bencher.iter(|| {
        vm.context_object_pointer
            .mock_set_remaining(std::i64::MAX as u64);
        vm.execute_program(&executable, false).1.unwrap();
    });
    let summary = bencher.bench(|_bencher| Ok(())).unwrap().unwrap();
    println!("  {:?} instructions", instructions);
    println!("  {:?} ns/iter median", summary.median as u64);
    assert!(0f64 != summary.median);
    let mips = (instructions * (ns_per_s / summary.median as u64)) / one_million;
    println!("  {:?} MIPS", mips);
    println!("{{ \"type\": \"bench\", \"name\": \"bench_program_alu_jit_to_native_mips\", \"median\": {:?}, \"deviation\": 0 }}", mips);
}

#[bench]
fn bench_program_execute_noop(bencher: &mut Bencher) {
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let bank = Bank::new_for_benches(&genesis_config);
    let bank = Arc::new(bank);
    let mut bank_client = BankClient::new_shared(bank.clone());

    let invoke_program_id = load_program(&bank_client, &bpf_loader::id(), &mint_keypair, "noop");
    let bank = bank_client
        .advance_slot(1, &Pubkey::default())
        .expect("Failed to advance the slot");

    let mint_pubkey = mint_keypair.pubkey();
    let account_metas = vec![AccountMeta::new(mint_pubkey, true)];

    let instruction =
        Instruction::new_with_bincode(invoke_program_id, &[u8::MAX, 0, 0, 0], account_metas);
    let message = Message::new(&[instruction], Some(&mint_pubkey));

    bank_client
        .send_and_confirm_message(&[&mint_keypair], message.clone())
        .unwrap();

    bencher.iter(|| {
        bank.clear_signatures();
        bank_client
            .send_and_confirm_message(&[&mint_keypair], message.clone())
            .unwrap();
    });
}

#[bench]
fn bench_create_vm(bencher: &mut Bencher) {
    let elf = load_program_from_file("noop");
    with_mock_invoke_context!(invoke_context, bpf_loader::id(), 10000001);
    const BUDGET: u64 = 200_000;
    invoke_context.mock_set_remaining(BUDGET);

    let direct_mapping = invoke_context
        .feature_set
        .is_active(&bpf_account_data_direct_mapping::id());
    let program_runtime_environment = create_program_runtime_environment_v1(
        &invoke_context.feature_set,
        &ComputeBudget::default(),
        true,
        false,
    );
    let executable =
        Executable::<InvokeContext>::from_elf(&elf, Arc::new(program_runtime_environment.unwrap()))
            .unwrap();

    executable.verify::<RequisiteVerifier>().unwrap();

    // Serialize account data
    let (_serialized, regions, account_lengths) = serialize_parameters(
        invoke_context.transaction_context,
        invoke_context
            .transaction_context
            .get_current_instruction_context()
            .unwrap(),
        true,            // should_cap_ix_accounts
        !direct_mapping, // copy_account_data
    )
    .unwrap();

    bencher.iter(|| {
        create_vm!(
            vm,
            &executable,
            clone_regions(&regions),
            account_lengths.clone(),
            &mut invoke_context,
        );
        vm.unwrap();
    });
}

#[bench]
fn bench_instruction_count_tuner(_bencher: &mut Bencher) {
    let elf = load_program_from_file("tuner");
    with_mock_invoke_context!(invoke_context, bpf_loader::id(), 10000001);
    const BUDGET: u64 = 200_000;
    invoke_context.mock_set_remaining(BUDGET);

    let direct_mapping = invoke_context
        .feature_set
        .is_active(&bpf_account_data_direct_mapping::id());

    // Serialize account data
    let (_serialized, regions, account_lengths) = serialize_parameters(
        invoke_context.transaction_context,
        invoke_context
            .transaction_context
            .get_current_instruction_context()
            .unwrap(),
        true,            // should_cap_ix_accounts
        !direct_mapping, // copy_account_data
    )
    .unwrap();

    let program_runtime_environment = create_program_runtime_environment_v1(
        &invoke_context.feature_set,
        &ComputeBudget::default(),
        true,
        false,
    );
    let executable =
        Executable::<InvokeContext>::from_elf(&elf, Arc::new(program_runtime_environment.unwrap()))
            .unwrap();

    executable.verify::<RequisiteVerifier>().unwrap();

    create_vm!(
        vm,
        &executable,
        regions,
        account_lengths,
        &mut invoke_context,
    );
    let mut vm = vm.unwrap();

    let mut measure = Measure::start("tune");
    let (instructions, _result) = vm.execute_program(&executable, true);
    measure.stop();

    assert_eq!(
        0,
        vm.context_object_pointer.get_remaining(),
        "Tuner must consume the whole budget"
    );
    println!(
        "{:?} compute units took {:?} us ({:?} instructions)",
        BUDGET - vm.context_object_pointer.get_remaining(),
        measure.as_us(),
        instructions,
    );
}

fn clone_regions(regions: &[MemoryRegion]) -> Vec<MemoryRegion> {
    unsafe {
        regions
            .iter()
            .map(|region| match region.state.get() {
                MemoryState::Readable => MemoryRegion::new_readonly(
                    slice::from_raw_parts(region.host_addr.get() as *const _, region.len as usize),
                    region.vm_addr,
                ),
                MemoryState::Writable => MemoryRegion::new_writable(
                    slice::from_raw_parts_mut(
                        region.host_addr.get() as *mut _,
                        region.len as usize,
                    ),
                    region.vm_addr,
                ),
                MemoryState::Cow(id) => MemoryRegion::new_cow(
                    slice::from_raw_parts(region.host_addr.get() as *const _, region.len as usize),
                    region.vm_addr,
                    id,
                ),
            })
            .collect()
    }
}
