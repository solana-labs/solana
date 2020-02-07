fn main() {
    // Until https://github.com/rust-lang/rust/issues/44839 is resolved
    // `#[target_feature(enable = "avx512f")]` is not available.
    // Use an env flag instead:
    if is_x86_feature_detected!("avx512f") {
        println!("cargo:rustc-env=TARGET_FEATURE_AVX512F=1");
    }
}
