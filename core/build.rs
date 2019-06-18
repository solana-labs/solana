use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if env::var("CARGO_FEATURE_CUDA").is_ok() {
        println!("cargo:rustc-cfg=cuda");

        let perf_libs_dir = {
            let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
            let mut path = Path::new(&manifest_dir);
            path = path.parent().unwrap();
            let mut path = path.join(Path::new("target/perf-libs"));
            path.push(
                env::var("SOLANA_PERF_LIBS_CUDA")
                    .unwrap_or_else(|err| panic!("SOLANA_PERF_LIBS_CUDA not defined: {}", err)),
            );
            path
        };
        let perf_libs_dir = perf_libs_dir.to_str().unwrap();

        // Ensure `perf_libs_dir` exists.  It's been observed that
        // a cargo:rerun-if-changed= directive with a non-existent
        // directory triggers a rebuild on every |cargo build| invocation
        fs::create_dir_all(&perf_libs_dir).unwrap_or_else(|err| {
            if err.kind() != std::io::ErrorKind::AlreadyExists {
                panic!("Unable to create {}: {:?}", perf_libs_dir, err);
            }
        });
        println!("cargo:rerun-if-changed={}", perf_libs_dir);
        println!("cargo:rustc-link-search=native={}", perf_libs_dir);
        if cfg!(windows) {
            println!("cargo:rerun-if-changed={}/libcuda-crypt.dll", perf_libs_dir);
        } else if cfg!(target_os = "macos") {
            println!(
                "cargo:rerun-if-changed={}/libcuda-crypt.dylib",
                perf_libs_dir
            );
        } else {
            println!("cargo:rerun-if-changed={}/libcuda-crypt.so", perf_libs_dir);
        }

        let cuda_home = match env::var("CUDA_HOME") {
            Ok(cuda_home) => cuda_home,
            Err(_) => String::from("/usr/local/cuda"),
        };
        println!("cargo:rustc-link-search=native={}/lib64", cuda_home);
        println!("cargo:rustc-link-lib=dylib=cudart");
        println!("cargo:rustc-link-lib=dylib=cuda");
        println!("cargo:rustc-link-lib=dylib=cudadevrt");
    }
}
