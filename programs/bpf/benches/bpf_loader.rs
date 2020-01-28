#![feature(test)]
#![cfg(feature = "bpf_c")]
extern crate test;

use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use solana_rbpf::EbpfVm;
use std::{env, fs::File, io::Error, io::Read, mem, path::PathBuf};
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

fn empty_check(_prog: &[u8]) -> Result<(), Error> {
    Ok(())
}

fn load_elf() -> Result<Vec<u8>, std::io::Error> {
    let path = create_bpf_path("bench_alu");
    let mut file = File::open(&path).expect(&format!("Unable to open {:?}", path));
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();
    Ok(elf)
}

const ARMSTRONG_LIMIT: u64 = 500;
const ARMSTRONG_EXPECTED: u64 = 5;

#[bench]
fn bench_program_load_elf(bencher: &mut Bencher) {
    let elf = load_elf().unwrap();
    let mut vm = EbpfVm::new(None).unwrap();
    vm.set_verifier(empty_check).unwrap();

    bencher.iter(|| {
        vm.set_elf(&elf).unwrap();
    });
}

#[bench]
fn bench_program_verify(bencher: &mut Bencher) {
    let elf = load_elf().unwrap();
    let mut vm = EbpfVm::new(None).unwrap();
    vm.set_verifier(empty_check).unwrap();
    vm.set_elf(&elf).unwrap();

    bencher.iter(|| {
        vm.set_verifier(solana_bpf_loader_program::bpf_verifier::check)
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

    let elf = load_elf().unwrap();
    let (mut vm, _) = solana_bpf_loader_program::create_vm(&elf).unwrap();

    println!("Interpreted:");
    assert_eq!(
        0, /*success*/
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
    let instructions = vm.get_last_instruction_count();
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
