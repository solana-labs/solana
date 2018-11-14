use std::env;
use std::fs;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Ensure target/perf-libs/ exists.  It's been observed that
    // a cargo:rerun-if-changed= directive with a non-existent
    // directory triggers a rebuild on every |cargo build| invocation
    fs::create_dir_all("target/perf-libs").unwrap_or_else(|err| {
        if err.kind() != std::io::ErrorKind::AlreadyExists {
            panic!("Unable to create target/perf-libs: {:?}", err);
        }
    });

    let chacha = !env::var("CARGO_FEATURE_CHACHA").is_err();
    let cuda = !env::var("CARGO_FEATURE_CUDA").is_err();
    let erasure = !env::var("CARGO_FEATURE_ERASURE").is_err();

    if chacha || cuda || erasure {
        println!("cargo:rerun-if-changed=target/perf-libs");
        println!("cargo:rustc-link-search=native=target/perf-libs");
    }
    if chacha {
        println!("cargo:rerun-if-changed=target/perf-libs/libcpu-crypt.a");
    }
    if cuda {
        let cuda_home = match env::var("CUDA_HOME") {
            Ok(cuda_home) => cuda_home,
            Err(_) => String::from("/usr/local/cuda"),
        };

        println!("cargo:rerun-if-changed=target/perf-libs/libcuda-crypt.a");
        println!("cargo:rustc-link-lib=static=cuda-crypt");
        println!("cargo:rustc-link-search=native={}/lib64", cuda_home);
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
