fn main() {
    println!("cargo:rustc-link-search=native=.");
    println!("cargo:rustc-link-lib=static=cuda_verify_ed25519");
    println!("cargo:rustc-link-search=native=/usr/local/cuda/lib64");
    println!("cargo:rustc-link-lib=dylib=cudart");
    println!("cargo:rustc-link-lib=dylib=cuda");
    println!("cargo:rustc-link-lib=dylib=cudadevrt");
}
