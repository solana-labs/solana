use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let bpf_c = !env::var("CARGO_FEATURE_BPF_C").is_err();

    if bpf_c {
        let out_dir = "OUT_DIR=../../../target/".to_string()
            + &env::var("PROFILE").unwrap()
            + &"/bpf".to_string();

        let rerun_if_changed_files = vec![
            "../../../sdk/bpf/bpf.mk",
            "../../../sdk/bpf/inc/solana_sdk.h",
            "../../bpf/c/makefile",
            "../../bpf/c/src/bench_alu.c",
            "../../bpf/c/src/move_funds.c",
            "../../bpf/c/src/noop++.cc",
            "../../bpf/c/src/noop.c",
            "../../bpf/c/src/struct_pass.c",
            "../../bpf/c/src/struct_ret.c",
        ];

        for file in rerun_if_changed_files {
            if !Path::new(file).is_file() {
                panic!("{} is not a file", file);
            }
            println!("cargo:rerun-if-changed={}", file);
        }

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
