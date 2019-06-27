use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let perf_libs_dir = {
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let mut path = Path::new(&manifest_dir);
        path = path.parent().unwrap();
        let path = path.join(Path::new("target/perf-libs"));
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
    println!("cargo:rerun-if-changed={}/libpoh-simd.a", perf_libs_dir);
    println!("cargo:rustc-link-search=native={}", perf_libs_dir);
//    println!("cargo:rustc-link-lib=static=poh-simd");
}
