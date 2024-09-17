fn main() {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if is_x86_feature_detected!("avx") {
            println!("cargo:rustc-cfg=build_target_feature_avx");
        }
        if is_x86_feature_detected!("avx2") {
            println!("cargo:rustc-cfg=build_target_feature_avx2");
        }
    }
}
