#![feature(test)]
extern crate byteorder;
extern crate solana_bpf_loader;
extern crate solana_rbpf;
extern crate test;

use byteorder::{LittleEndian, WriteBytesExt};
use solana_rbpf::EbpfVmRaw;
use std::env;
use std::fs::File;
use std::io::Error;
use std::io::Read;
use std::path::PathBuf;
use test::Bencher;

/// BPF program file extension
const PLATFORM_FILE_EXTENSION_BPF: &str = "o";
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

fn empty_check(_prog: &[u8]) -> Result<(), Error> {
    Ok(())
}

const ARMSTRONG_LIMIT: u64 = 500;
const ARMSTRONG_EXPECTED: u64 = 5;

#[bench]
fn bench_program_load_elf(bencher: &mut Bencher) {
    let mut file = File::open(create_bpf_path("bench_alu")).expect("file open failed");
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();

    let mut vm = EbpfVmRaw::new(None).unwrap();
    vm.set_verifier(empty_check).unwrap();

    bencher.iter(|| {
        vm.set_elf(&elf).unwrap();
    });
}

#[bench]
fn bench_program_verify(bencher: &mut Bencher) {
    let mut file = File::open(create_bpf_path("bench_alu")).expect("file open failed");
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();

    let mut vm = EbpfVmRaw::new(None).unwrap();
    vm.set_verifier(empty_check).unwrap();
    vm.set_elf(&elf).unwrap();

    bencher.iter(|| {
        vm.set_verifier(solana_bpf_loader::bpf_verifier::check)
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

    let mut file = File::open(create_bpf_path("bench_alu")).expect("file open failed");
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();
    let mut vm = solana_bpf_loader::create_vm(&elf).unwrap();

    println!("Interpreted:");
    assert_eq!(
        ARMSTRONG_EXPECTED,
        vm.execute_program(&mut inner_iter).unwrap()
    );
    bencher.iter(|| {
        vm.execute_program(&mut inner_iter).unwrap();
    });
    let instructions = vm.get_last_instruction_count();
    let summary = bencher.bench(|_bencher| {}).unwrap();
    println!("  {:?} instructions", instructions);
    println!("  {:?} ns/iter median", summary.median as u64);
    assert!(0f64 != summary.median);
    let mips = (instructions * (ns_per_s / summary.median as u64)) / one_million;
    println!("  {:?} MIPS", mips);

    println!("JIT to native:");
    vm.jit_compile().unwrap();
    unsafe {
        assert_eq!(
            ARMSTRONG_EXPECTED,
            vm.execute_program_jit(&mut inner_iter).unwrap()
        );
    }
    bencher.iter(|| unsafe {
        vm.execute_program_jit(&mut inner_iter).unwrap();
    });
    let summary = bencher.bench(|_bencher| {}).unwrap();
    println!("  {:?} instructions", instructions);
    println!("  {:?} ns/iter median", summary.median as u64);
    assert!(0f64 != summary.median);
    let mips = (instructions * (ns_per_s / summary.median as u64)) / one_million;
    println!("  {:?} MIPS", mips);
}
