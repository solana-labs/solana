use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let perf_libs_dir = {
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let mut path = Path::new(&manifest_dir);
        path = path.parent().unwrap();
        path.join(Path::new("target/perf-libs"))
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

    let chacha = !env::var("CARGO_FEATURE_CHACHA").is_err();
    let cuda = !env::var("CARGO_FEATURE_CUDA").is_err();
    let erasure = !env::var("CARGO_FEATURE_ERASURE").is_err();

    if chacha || cuda || erasure {
        println!("cargo:rerun-if-changed={}", perf_libs_dir);
        println!("cargo:rustc-link-search=native={}", perf_libs_dir);
    }
    if chacha {
        println!("cargo:rerun-if-changed={}/libcpu-crypt.a", perf_libs_dir);
    }
    if cuda {
        let cuda_home = match env::var("CUDA_HOME") {
            Ok(cuda_home) => cuda_home,
            Err(_) => String::from("/usr/local/cuda"),
        };

        println!("cargo:rerun-if-changed={}/libcuda-crypt.a", perf_libs_dir);
        println!("cargo:rustc-link-lib=static=cuda-crypt");
        println!("cargo:rustc-link-search=native={}/lib64", cuda_home);
        println!("cargo:rustc-link-lib=dylib=cudart");
        println!("cargo:rustc-link-lib=dylib=cuda");
        println!("cargo:rustc-link-lib=dylib=cudadevrt");
    }
    if erasure {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            println!(
                "cargo:rerun-if-changed={}/libgf_complete.dylib",
                perf_libs_dir
            );
            println!("cargo:rerun-if-changed={}/libJerasure.dylib", perf_libs_dir);
        }
        #[cfg(all(unix, not(any(target_os = "macos", target_os = "ios"))))]
        {
            println!("cargo:rerun-if-changed={}/libgf_complete.so", perf_libs_dir);
            println!("cargo:rerun-if-changed={}/libJerasure.so", perf_libs_dir);
        }
        #[cfg(windows)]
        {
            println!(
                "cargo:rerun-if-changed={}/libgf_complete.dll",
                perf_libs_dir
            );
            println!("cargo:rerun-if-changed={}/libJerasure.dll", perf_libs_dir);
        }

        println!("cargo:rustc-link-lib=dylib=Jerasure");
        println!("cargo:rustc-link-lib=dylib=gf_complete");
    }
}
