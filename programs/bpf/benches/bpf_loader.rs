#![feature(test)]
#![cfg(feature = "bpf_c")]

extern crate test;
#[macro_use]
extern crate solana_bpf_loader_program;

use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use solana_bpf_loader_program::syscalls::SyscallError;
use solana_measure::measure::Measure;
use solana_rbpf::vm::{EbpfVm, InstructionMeter};
use solana_runtime::{
    bank::Bank,
    bank_client::BankClient,
    genesis_utils::{create_genesis_config, GenesisConfigInfo},
    loader_utils::load_program,
    process_instruction::{
        ComputeBudget, ComputeMeter, Executor, InvokeContext, Logger, ProcessInstruction,
    },
};
use solana_sdk::{
    account::Account,
    bpf_loader,
    client::SyncClient,
    entrypoint::SUCCESS,
    instruction::{AccountMeta, CompiledInstruction, Instruction, InstructionError},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::{cell::RefCell, env, fs::File, io::Read, mem, path::PathBuf, rc::Rc, sync::Arc};
use test::Bencher;

/// BPF program file extension
const PLATFORM_FILE_EXTENSION_BPF: &str = "so";
/// Create a BPF program file name
fn create_bpf_path(name: &str) -> PathBuf {
    let mut pathbuf = {
        let current_exe = env::current_exe().unwrap();
        PathBuf::from(current_exe.parent().unwrap().parent().unwrap())
    };
    pathbuf.push("bpf/");
    pathbuf.push(name);
    pathbuf.set_extension(PLATFORM_FILE_EXTENSION_BPF);
    pathbuf
}

fn load_elf(name: &str) -> Result<Vec<u8>, std::io::Error> {
    let path = create_bpf_path(name);
    let mut file = File::open(&path).expect(&format!("Unable to open {:?}", path));
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();
    Ok(elf)
}

fn load_bpf_program(
    bank_client: &BankClient,
    loader_id: &Pubkey,
    payer_keypair: &Keypair,
    name: &str,
) -> Pubkey {
    let path = create_bpf_path(name);
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

    bencher.iter(|| {
        let _ =
            EbpfVm::<solana_bpf_loader_program::BPFError>::create_executable_from_elf(&elf, None)
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
    let loader_id = bpf_loader::id();
    let mut invoke_context = MockInvokeContext::default();

    let elf = load_elf("bench_alu").unwrap();
    let executable =
        EbpfVm::<solana_bpf_loader_program::BPFError>::create_executable_from_elf(&elf, None)
            .unwrap();
    let (mut vm, _) = solana_bpf_loader_program::create_vm(
        &loader_id,
        executable.as_ref(),
        &[],
        &mut invoke_context,
    )
    .unwrap();

    println!("Interpreted:");
    assert_eq!(
        SUCCESS,
        vm.execute_program(&mut inner_iter, &[], &[]).unwrap()
    );
    assert_eq!(ARMSTRONG_LIMIT, LittleEndian::read_u64(&inner_iter));
    assert_eq!(
        ARMSTRONG_EXPECTED,
        LittleEndian::read_u64(&inner_iter[mem::size_of::<u64>()..])
    );

    bencher.iter(|| {
        vm.execute_program(&mut inner_iter, &[], &[]).unwrap();
    });
    let instructions = vm.get_total_instruction_count();
    let summary = bencher.bench(|_bencher| {}).unwrap();
    println!("  {:?} instructions", instructions);
    println!("  {:?} ns/iter median", summary.median as u64);
    assert!(0f64 != summary.median);
    let mips = (instructions * (ns_per_s / summary.median as u64)) / one_million;
    println!("  {:?} MIPS", mips);
    println!("{{ \"type\": \"bench\", \"name\": \"bench_program_alu_interpreted_mips\", \"median\": {:?}, \"deviation\": 0 }}", mips);

    // JIT disabled until address translation support is added
    // println!("JIT to native:");
    // vm.jit_compile().unwrap();
    // unsafe {
    //     assert_eq!(
    //         0, /*success*/
    //         vm.execute_program_jit(&mut inner_iter).unwrap()
    //     );
    // }
    // assert_eq!(ARMSTRONG_LIMIT, LittleEndian::read_u64(&inner_iter));
    // assert_eq!(
    //     ARMSTRONG_EXPECTED,
    //     LittleEndian::read_u64(&inner_iter[mem::size_of::<u64>()..])
    // );

    // bencher.iter(|| unsafe {
    //     vm.execute_program_jit(&mut inner_iter).unwrap();
    // });
    // let summary = bencher.bench(|_bencher| {}).unwrap();
    // println!("  {:?} instructions", instructions);
    // println!("  {:?} ns/iter median", summary.median as u64);
    // assert!(0f64 != summary.median);
    // let mips = (instructions * (ns_per_s / summary.median as u64)) / one_million;
    // println!("  {:?} MIPS", mips);
    // println!("{{ \"type\": \"bench\", \"name\": \"bench_program_alu_jit_to_native_mips\", \"median\": {:?}, \"deviation\": 0 }}", mips);
}

#[bench]
fn bench_program_execute_noop(bencher: &mut Bencher) {
    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let mut bank = Bank::new(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin_loader(&name, id, entrypoint);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let invoke_program_id =
        load_bpf_program(&bank_client, &bpf_loader::id(), &mint_keypair, "noop");

    let mint_pubkey = mint_keypair.pubkey();
    let account_metas = vec![AccountMeta::new(mint_pubkey, true)];

    let instruction = Instruction::new(invoke_program_id, &[u8::MAX, 0, 0, 0], account_metas);
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
fn bench_instruction_count_tuner(_bencher: &mut Bencher) {
    const BUDGET: u64 = 200_000;
    let loader_id = bpf_loader::id();
    let mut invoke_context = MockInvokeContext::default();
    invoke_context.compute_meter.borrow_mut().remaining = BUDGET;
    let compute_meter = invoke_context.get_compute_meter();

    let elf = load_elf("tuner").unwrap();
    let executable =
        EbpfVm::<solana_bpf_loader_program::BPFError>::create_executable_from_elf(&elf, None)
            .unwrap();
    let (mut vm, _) = solana_bpf_loader_program::create_vm(
        &loader_id,
        executable.as_ref(),
        &[],
        &mut invoke_context,
    )
    .unwrap();
    let instruction_meter = MockInstructionMeter { compute_meter };

    let mut measure = Measure::start("tune");
    let _ = vm.execute_program_metered(&mut [0], &[], &[], instruction_meter.clone());
    measure.stop();
    assert_eq!(
        0,
        instruction_meter.get_remaining(),
        "Tuner must consume the whole budget"
    );
    println!(
        "{:?} Consumed compute budget took {:?} us ({:?} instructions)",
        BUDGET - instruction_meter.get_remaining(),
        measure.as_us(),
        vm.get_total_instruction_count(),
    );
}

#[derive(Debug, Default)]
pub struct MockInvokeContext {
    key: Pubkey,
    logger: MockLogger,
    compute_meter: Rc<RefCell<MockComputeMeter>>,
}
impl InvokeContext for MockInvokeContext {
    fn push(&mut self, _key: &Pubkey) -> Result<(), InstructionError> {
        Ok(())
    }
    fn pop(&mut self) {}
    fn verify_and_update(
        &mut self,
        _message: &Message,
        _instruction: &CompiledInstruction,
        _accounts: &[Rc<RefCell<Account>>],
    ) -> Result<(), InstructionError> {
        Ok(())
    }
    fn get_caller(&self) -> Result<&Pubkey, InstructionError> {
        Ok(&self.key)
    }
    fn get_programs(&self) -> &[(Pubkey, ProcessInstruction)] {
        &[]
    }
    fn get_logger(&self) -> Rc<RefCell<dyn Logger>> {
        Rc::new(RefCell::new(self.logger.clone()))
    }
    fn is_cross_program_supported(&self) -> bool {
        true
    }
    fn get_compute_budget(&self) -> ComputeBudget {
        ComputeBudget::default()
    }
    fn get_compute_meter(&self) -> Rc<RefCell<dyn ComputeMeter>> {
        self.compute_meter.clone()
    }
    fn add_executor(&mut self, _pubkey: &Pubkey, _executor: Arc<dyn Executor>) {}
    fn get_executor(&mut self, _pubkey: &Pubkey) -> Option<Arc<dyn Executor>> {
        None
    }
    fn record_instruction(&self, _instruction: &Instruction) {}
}
#[derive(Debug, Default, Clone)]
pub struct MockLogger {
    pub log: Rc<RefCell<Vec<String>>>,
}
impl Logger for MockLogger {
    fn log_enabled(&self) -> bool {
        true
    }
    fn log(&mut self, message: &str) {
        self.log.borrow_mut().push(message.to_string());
    }
}
#[derive(Debug, Default, Clone)]
pub struct MockComputeMeter {
    pub remaining: u64,
}
impl ComputeMeter for MockComputeMeter {
    fn consume(&mut self, amount: u64) -> Result<(), InstructionError> {
        let exceeded = self.remaining < amount;
        self.remaining = self.remaining.saturating_sub(amount);
        if exceeded {
            return Err(InstructionError::ComputationalBudgetExceeded);
        }
        Ok(())
    }
    fn get_remaining(&self) -> u64 {
        self.remaining
    }
}

/// Passed to the VM to enforce the compute budget
#[derive(Clone)]
struct MockInstructionMeter {
    compute_meter: Rc<RefCell<dyn ComputeMeter>>,
}
impl InstructionMeter for MockInstructionMeter {
    fn consume(&mut self, amount: u64) {
        // 1 to 1 instruction to compute unit mapping
        // ignore error, Ebpf will bail if exceeded
        let _ = self.compute_meter.borrow_mut().consume(amount);
    }
    fn get_remaining(&self) -> u64 {
        self.compute_meter.borrow().get_remaining()
    }
}
