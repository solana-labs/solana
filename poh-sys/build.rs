use std::env;
use std::process::Command;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();

    println!("cargo:rerun-if-changed=poh-simd/poh-verify.ipsc");
    println!("cargo:rerun-if-changed=poh-simd/sha256.h");

    Command::new("ispc")
        .args(&[
            "-O2",
            "--target=sse2-i32x4",
            "-DNAME_SUFFIX=sse2",
            "poh-simd/poh-verify.ipsc",
            "-o",
            &format!("{}/poh-verify-sse2.o", out_dir),
        ])
        .status()
        .unwrap();

    Command::new("ispc")
        .args(&[
            "-O2",
            "--target=sse4-i32x4",
            "-DNAME_SUFFIX=sse4",
            "poh-simd/poh-verify.ipsc",
            "-o",
            &format!("{}/poh-verify-sse4.o", out_dir),
        ])
        .status()
        .unwrap();

    Command::new("ispc")
        .args(&[
            "-O2",
            "--target=avx1-i32x8",
            "-DNAME_SUFFIX=avx1",
            "poh-simd/poh-verify.ipsc",
            "-o",
            &format!("{}/poh-verify-avx1.o", out_dir),
        ])
        .status()
        .unwrap();

    Command::new("ispc")
        .args(&[
            "-O2",
            "--target=avx2-i32x8",
            "-DNAME_SUFFIX=avx2",
            "poh-simd/poh-verify.ipsc",
            "-o",
            &format!("{}/poh-verify-avx2.o", out_dir),
        ])
        .status()
        .unwrap();

    cc::Build::new()
        .object(&format!("{}/poh-verify-sse2.o", out_dir))
        .object(&format!("{}/poh-verify-sse4.o", out_dir))
        .object(&format!("{}/poh-verify-avx1.o", out_dir))
        .object(&format!("{}/poh-verify-avx2.o", out_dir))
        .compile("poh-simd");

    println!("cargo:rustc-link-search=native={}", out_dir);
    println!("cargo:rustc-link-lib=static=poh-simd");
}
