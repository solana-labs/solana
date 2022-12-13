#![feature(test)]
#![cfg(feature = "sbf_c")]

extern crate test;
#[macro_use]
extern crate solana_bpf_loader_program;

use {
    byteorder::{ByteOrder, LittleEndian, WriteBytesExt},
    solana_bpf_loader_program::{
        create_vm, serialization::serialize_parameters, syscalls::create_loader,
    },
    solana_measure::measure::Measure,
    solana_program_runtime::{
        compute_budget::ComputeBudget,
        invoke_context::{with_mock_invoke_context, InvokeContext},
    },
    solana_rbpf::{
        ebpf::MM_INPUT_START,
        elf::Executable,
        memory_region::MemoryRegion,
        verifier::RequisiteVerifier,
        vm::{ContextObject, VerifiedExecutable},
    },
    solana_runtime::{
        bank::Bank,
        bank_client::BankClient,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        loader_utils::load_program,
    },
    solana_sdk::{
        bpf_loader,
        client::SyncClient,
        entrypoint::SUCCESS,
        feature_set::FeatureSet,
        instruction::{AccountMeta, Instruction},
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
    },
    std::{env, fs::File, io::Read, mem, path::PathBuf, sync::Arc},
    test::Bencher,
};

/// SBF program file extension
const PLATFORM_FILE_EXTENSION_SBF: &str = "so";
/// Create a SBF program file name
fn create_sbf_path(name: &str) -> PathBuf {
    let mut pathbuf = {
        let current_exe = env::current_exe().unwrap();
        PathBuf::from(current_exe.parent().unwrap().parent().unwrap())
    };
    pathbuf.push("sbf/");
    pathbuf.push(name);
    pathbuf.set_extension(PLATFORM_FILE_EXTENSION_SBF);
    pathbuf
}

fn load_elf(name: &str) -> Result<Vec<u8>, std::io::Error> {
    let path = create_sbf_path(name);
    let mut file = File::open(&path).expect(&format!("Unable to open {:?}", path));
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();
    Ok(elf)
}

fn load_sbf_program(
    bank_client: &BankClient,
    loader_id: &Pubkey,
    payer_keypair: &Keypair,
    name: &str,
) -> Pubkey {
    let path = create_sbf_path(name);
    let mut file = File::open(path).unwrap();
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();
    load_program(bank_client, payer_keypair, loader_id, elf)
}

const ARMSTRONG_LIMIT: u64 = 500;
const ARMSTRONG_EXPECTED: u64 = 5;

#[bench]
fn bench_program_create_executable(bencher: &mut Bencher) {
    let elf = load_elf("bench_alu").unwrap();

    let loader = create_loader(
        &FeatureSet::default(),
        &ComputeBudget::default(),
        true,
        true,
        false,
    )
    .unwrap();
    bencher.iter(|| {
        let _ = Executable::<InvokeContext>::from_elf(&elf, loader.clone()).unwrap();
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
    let elf = load_elf("bench_alu").unwrap();
    let loader_id = bpf_loader::id();
    with_mock_invoke_context(loader_id, 10000001, false, |invoke_context| {
        let loader = create_loader(
            &invoke_context.feature_set,
            &ComputeBudget::default(),
            true,
            true,
            false,
        )
        .unwrap();
        let executable = Executable::<InvokeContext>::from_elf(&elf, loader).unwrap();

        let mut verified_executable =
            VerifiedExecutable::<RequisiteVerifier, InvokeContext>::from_executable(executable)
                .unwrap();

        verified_executable.jit_compile().unwrap();
        let mut vm = create_vm(
            &verified_executable,
            vec![MemoryRegion::new_writable(&mut inner_iter, MM_INPUT_START)],
            vec![],
            invoke_context,
        )
        .unwrap();

        println!("Interpreted:");
        vm.env
            .context_object_pointer
            .mock_set_remaining(std::i64::MAX as u64);
        let (instructions, result) = vm.execute_program(true);
        assert_eq!(SUCCESS, result.unwrap());
        assert_eq!(ARMSTRONG_LIMIT, LittleEndian::read_u64(&inner_iter));
        assert_eq!(
            ARMSTRONG_EXPECTED,
            LittleEndian::read_u64(&inner_iter[mem::size_of::<u64>()..])
        );

        bencher.iter(|| {
            vm.env
                .context_object_pointer
                .mock_set_remaining(std::i64::MAX as u64);
            vm.execute_program(true).1.unwrap();
        });
        let summary = bencher.bench(|_bencher| Ok(())).unwrap().unwrap();
        println!("  {:?} instructions", instructions);
        println!("  {:?} ns/iter median", summary.median as u64);
        assert!(0f64 != summary.median);
        let mips = (instructions * (ns_per_s / summary.median as u64)) / one_million;
        println!("  {:?} MIPS", mips);
        println!("{{ \"type\": \"bench\", \"name\": \"bench_program_alu_interpreted_mips\", \"median\": {:?}, \"deviation\": 0 }}", mips);

        println!("JIT to native:");
        assert_eq!(SUCCESS, vm.execute_program(false).1.unwrap());
        assert_eq!(ARMSTRONG_LIMIT, LittleEndian::read_u64(&inner_iter));
        assert_eq!(
            ARMSTRONG_EXPECTED,
            LittleEndian::read_u64(&inner_iter[mem::size_of::<u64>()..])
        );

        bencher.iter(|| {
            vm.env
                .context_object_pointer
                .mock_set_remaining(std::i64::MAX as u64);
            vm.execute_program(false).1.unwrap();
        });
        let summary = bencher.bench(|_bencher| Ok(())).unwrap().unwrap();
        println!("  {:?} instructions", instructions);
        println!("  {:?} ns/iter median", summary.median as u64);
        assert!(0f64 != summary.median);
        let mips = (instructions * (ns_per_s / summary.median as u64)) / one_million;
        println!("  {:?} MIPS", mips);
        println!("{{ \"type\": \"bench\", \"name\": \"bench_program_alu_jit_to_native_mips\", \"median\": {:?}, \"deviation\": 0 }}", mips);
    });
}

#[bench]
fn bench_program_execute_noop(bencher: &mut Bencher) {
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let mut bank = Bank::new_for_benches(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let invoke_program_id =
        load_sbf_program(&bank_client, &bpf_loader::id(), &mint_keypair, "noop");

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
    let elf = load_elf("noop").unwrap();
    let loader_id = bpf_loader::id();
    with_mock_invoke_context(loader_id, 10000001, false, |invoke_context| {
        const BUDGET: u64 = 200_000;
        invoke_context.mock_set_remaining(BUDGET);

        // Serialize account data
        let (_serialized, regions, account_lengths) = serialize_parameters(
            invoke_context.transaction_context,
            invoke_context
                .transaction_context
                .get_current_instruction_context()
                .unwrap(),
            true, // should_cap_ix_accounts
        )
        .unwrap();

        let loader = create_loader(
            &invoke_context.feature_set,
            &ComputeBudget::default(),
            true,
            true,
            false,
        )
        .unwrap();
        let executable = Executable::<InvokeContext>::from_elf(&elf, loader).unwrap();

        let verified_executable =
            VerifiedExecutable::<RequisiteVerifier, InvokeContext>::from_executable(executable)
                .unwrap();

        bencher.iter(|| {
            let _ = create_vm(
                &verified_executable,
                regions.clone(),
                account_lengths.clone(),
                invoke_context,
            )
            .unwrap();
        });
    });
}

#[bench]
fn bench_instruction_count_tuner(_bencher: &mut Bencher) {
    let elf = load_elf("tuner").unwrap();
    let loader_id = bpf_loader::id();
    with_mock_invoke_context(loader_id, 10000001, true, |invoke_context| {
        const BUDGET: u64 = 200_000;
        invoke_context.mock_set_remaining(BUDGET);

        // Serialize account data
        let (_serialized, regions, account_lengths) = serialize_parameters(
            invoke_context.transaction_context,
            invoke_context
                .transaction_context
                .get_current_instruction_context()
                .unwrap(),
            true, // should_cap_ix_accounts
        )
        .unwrap();

        let loader = create_loader(
            &invoke_context.feature_set,
            &ComputeBudget::default(),
            true,
            true,
            false,
        )
        .unwrap();
        let executable = Executable::<InvokeContext>::from_elf(&elf, loader).unwrap();

        let verified_executable =
            VerifiedExecutable::<RequisiteVerifier, InvokeContext>::from_executable(executable)
                .unwrap();

        let mut vm = create_vm(
            &verified_executable,
            regions,
            account_lengths,
            invoke_context,
        )
        .unwrap();

        let mut measure = Measure::start("tune");
        let (instructions, _result) = vm.execute_program(true);
        measure.stop();

        assert_eq!(
            0,
            vm.env.context_object_pointer.get_remaining(),
            "Tuner must consume the whole budget"
        );
        println!(
            "{:?} compute units took {:?} us ({:?} instructions)",
            BUDGET - vm.env.context_object_pointer.get_remaining(),
            measure.as_us(),
            instructions,
        );
    });
}
