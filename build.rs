use std::env;

fn main() {
    println!("cargo:rustc-link-search=native=.");
    if !env::var("CARGO_FEATURE_CUDA").is_err() {
        println!("cargo:rustc-link-lib=static=cuda_verify_ed25519");
        println!("cargo:rustc-link-search=native=/usr/local/cuda/lib64");
        println!("cargo:rustc-link-lib=dylib=cudart");
        println!("cargo:rustc-link-lib=dylib=cuda");
        println!("cargo:rustc-link-lib=dylib=cudadevrt");
    }
    if !env::var("CARGO_FEATURE_ERASURE").is_err() {
        println!("cargo:rustc-link-lib=dylib=Jerasure");
        println!("cargo:rustc-link-lib=dylib=gf_complete");
    }
}
