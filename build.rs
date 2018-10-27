use std::env;
use std::fs;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Ensure target/perf-libs/ exists.  It's been observed that
    // a cargo:rerun-if-changed= directive with a non-existent
    // directory triggers a rebuild on every |cargo build| invocation
    fs::create_dir("target/perf-libs").unwrap_or_else(|err| {
        if err.kind() != std::io::ErrorKind::AlreadyExists {
            panic!("Unable to create target/perf-libs: {:?}", err);
        }
    });

    let bpf_c = !env::var("CARGO_FEATURE_BPF_C").is_err();
    let chacha = !env::var("CARGO_FEATURE_CHACHA").is_err();
    let cuda = !env::var("CARGO_FEATURE_CUDA").is_err();
    let erasure = !env::var("CARGO_FEATURE_ERASURE").is_err();

    if bpf_c {
        let out_dir = "OUT_DIR=../../../target/".to_string()
            + &env::var("PROFILE").unwrap()
            + &"/bpf".to_string();

        println!("cargo:rerun-if-changed=programs/bpf/c/makefile");
        println!("cargo:rerun-if-changed=programs/bpf/c/src/move_funds.c");
        println!("cargo:rerun-if-changed=programs/bpf/c/src/noop.c");
        println!("cargo:rerun-if-changed=programs/bpf/c/src/tictactoe.c");
        println!("cargo:rerun-if-changed=programs/bpf/c/src/tictactoe_dashboard.c");
        println!("cargo:warning=(not a warning) Compiling C-based BPF programs");
        let status = Command::new("make")
            .current_dir("programs/bpf/c")
            .arg("all")
            .arg(&out_dir)
            .status()
            .expect("Failed to build C-based BPF programs");
        assert!(status.success());
    }
    if chacha || cuda || erasure {
        println!("cargo:rerun-if-changed=target/perf-libs");
        println!("cargo:rustc-link-search=native=target/perf-libs");
    }
    if chacha {
        println!("cargo:rerun-if-changed=target/perf-libs/libcpu-crypt.a");
    }
    if cuda {
        println!("cargo:rerun-if-changed=target/perf-libs/libcuda-crypt.a");
        println!("cargo:rustc-link-lib=static=cuda-crypt");
        println!("cargo:rustc-link-search=native=/usr/local/cuda/lib64");
        println!("cargo:rustc-link-lib=dylib=cudart");
        println!("cargo:rustc-link-lib=dylib=cuda");
        println!("cargo:rustc-link-lib=dylib=cudadevrt");
    }
    if erasure {
        println!("cargo:rerun-if-changed=target/perf-libs/libgf_complete.so");
        println!("cargo:rerun-if-changed=target/perf-libs/libJerasure.so");
        println!("cargo:rustc-link-lib=dylib=Jerasure");
        println!("cargo:rustc-link-lib=dylib=gf_complete");
    }
}
