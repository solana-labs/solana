use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let bpf_c = !env::var("CARGO_FEATURE_BPF_C").is_err();

    if bpf_c {
        let out_dir = "OUT_DIR=../../../target/".to_string()
            + &env::var("PROFILE").unwrap()
            + &"/bpf".to_string();

        println!("cargo:rerun-if-changed=../../bpf/c/sdk/bpf.mk");
        println!("cargo:rerun-if-changed=../../bpf/c/sdk/inc/solana_sdk.h");
        println!("cargo:rerun-if-changed=../../bpf/c/makefile");
        println!("cargo:rerun-if-changed=../../bpf/c/src/bench_alu.c");
        println!("cargo:rerun-if-changed=../../bpf/c/src/move_funds.c");
        println!("cargo:rerun-if-changed=../../bpf/c/src/noop.c");
        println!("cargo:warning=(not a warning) Compiling C-based BPF programs");
        let status = Command::new("make")
            .current_dir("../../bpf/c")
            .arg("all")
            .arg(&out_dir)
            .status()
            .expect("Failed to build C-based BPF programs");
        assert!(status.success());
    }
}
