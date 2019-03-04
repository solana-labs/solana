extern crate walkdir;

use std::env;
use std::path::Path;
use std::process::Command;
use walkdir::WalkDir;

fn rerun_if_changed(files: &[&str], directories: &[&str]) {
    let mut all_files: Vec<_> = files.iter().map(|f| f.to_string()).collect();

    for directory in directories {
        let files_in_directory: Vec<_> = WalkDir::new(directory)
            .into_iter()
            .map(|entry| entry.unwrap())
            .filter(|entry| entry.file_type().is_file())
            .map(|f| f.path().to_str().unwrap().to_owned())
            .collect();
        all_files.extend_from_slice(&files_in_directory[..]);
    }

    for file in all_files {
        if !Path::new(&file).is_file() {
            panic!("{} is not a file", file);
        }
        println!("cargo:rerun-if-changed={}", file);
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let bpf_c = !env::var("CARGO_FEATURE_BPF_C").is_err();
    if bpf_c {
        let out_dir = "OUT_DIR=../../../target/".to_string()
            + &env::var("PROFILE").unwrap()
            + &"/bpf".to_string();

        rerun_if_changed(
            &["../../sdk/bpf/bpf.ld", "../../sdk/bpf/bpf.mk", "c/makefile"],
            &["../../sdk/bpf/inc", "../../sdk/bpf/scripts", "c/src"],
        );

        println!("cargo:warning=(not a warning) Compiling C-based BPF programs");
        let status = Command::new("make")
            .current_dir("c")
            .arg("programs")
            .arg(&out_dir)
            .status()
            .expect("Failed to build C-based BPF programs");
        assert!(status.success());
    }

    let bpf_rust = !env::var("CARGO_FEATURE_BPF_RUST").is_err();
    if bpf_rust {
        let install_dir =
            "../../../../target/".to_string() + &env::var("PROFILE").unwrap() + &"/bpf".to_string();

        if !Path::new("rust/noop/target/bpfel-unknown-unknown/release/solana_bpf_rust_noop.so")
            .is_file()
        {
            // Cannot build Rust BPF programs as part of main build because
            // to build it requires calling Cargo with different parameters which
            // would deadlock due to recursive cargo calls
            panic!(
                "solana_bpf_rust_noop.so not found, you must manually run \
                 `programs/bpf/rust/noop/build.sh` to build it"
            );
        }

        rerun_if_changed(
            &[
                "rust/noop/bpf.ld",
                "rust/noop/build.sh",
                "rust/noop/Cargo.toml",
                "rust/noop/target/bpfel-unknown-unknown/release/solana_bpf_rust_noop.so",
            ],
            &["rust/noop/src"],
        );

        println!(
            "cargo:warning=(not a warning) Installing Rust-based BPF program: solana_bpf_rust_noop"
        );
        let status = Command::new("mkdir")
            .current_dir("rust/noop")
            .arg("-p")
            .arg(&install_dir)
            .status()
            .expect("Unable to create BPF install directory");
        assert!(status.success());

        let status = Command::new("cp")
            .current_dir("rust/noop")
            .arg("target/bpfel-unknown-unknown/release/solana_bpf_rust_noop.so")
            .arg(&install_dir)
            .status()
            .expect("Failed to copy solana_rust_bpf_noop.so to install directory");
        assert!(status.success());
    }
}
