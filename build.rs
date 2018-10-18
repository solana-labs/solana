use std::env;
use std::fs;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=target/perf-libs");
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
        let out_dir = "target/".to_string() + &env::var("PROFILE").unwrap();

        println!("cargo:rerun-if-changed=programs/bpf/noop_c/build.sh");
        println!("cargo:rerun-if-changed=programs/bpf/noop_c/src/noop.c");
        println!("cargo:warning=(not a warning) Compiling noop_c");
        let status = Command::new("programs/bpf/noop_c/build.sh")
            .arg(&out_dir)
            .status()
            .expect("Failed to call noop_c build script");
        assert!(status.success());

        println!("cargo:rerun-if-changed=programs/bpf/move_funds_c/build.sh");
        println!("cargo:rerun-if-changed=programs/bpf/move_funds_c/src/move_funds.c");
        println!("cargo:warning=(not a warning) Compiling move_funds_c");
        let status = Command::new("programs/bpf/move_funds_c/build.sh")
            .arg(&out_dir)
            .status()
            .expect("Failed to call move_funds_c build script");
        assert!(status.success());

        println!("cargo:rerun-if-changed=programs/bpf/tictactoe_c/build.sh");
        println!("cargo:rerun-if-changed=programs/bpf/tictactoe_c/src/tictactoe.c");
        println!("cargo:warning=(not a warning) Compiling tictactoe_c");
        let status = Command::new("programs/bpf/tictactoe_c/build.sh")
            .arg(&out_dir)
            .status()
            .expect("Failed to call tictactoe_c build script");
        assert!(status.success());

        println!("cargo:rerun-if-changed=programs/bpf/tictactoe_dashboard_c/build.sh");
        println!(
            "cargo:rerun-if-changed=programs/bpf/tictactoe_dashboard_c/src/tictactoe_dashboard.c"
        );
        println!("cargo:warning=(not a warning) Compiling tictactoe_dashboard_c");
        let status = Command::new("programs/bpf/tictactoe_dashboard_c/build.sh")
            .arg(&out_dir)
            .status()
            .expect("Failed to call tictactoe_dashboard_c build script");
        assert!(status.success());
    }
    if chacha || cuda || erasure {
        println!("cargo:rustc-link-search=native=target/perf-libs");
    }
    if cuda {
        println!("cargo:rustc-link-lib=static=cuda_verify_ed25519");
        println!("cargo:rustc-link-search=native=/usr/local/cuda/lib64");
        println!("cargo:rustc-link-lib=dylib=cudart");
        println!("cargo:rustc-link-lib=dylib=cuda");
        println!("cargo:rustc-link-lib=dylib=cudadevrt");
    }
    if erasure {
        println!("cargo:rustc-link-lib=dylib=Jerasure");
        println!("cargo:rustc-link-lib=dylib=gf_complete");
    }
}
