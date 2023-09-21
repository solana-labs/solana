extern crate rustc_version;

use {
    rustc_version::{version_meta, Channel},
    std::io::Result,
};

fn main() -> Result<()> {
    const PROTOC_ENVAR: &str = "PROTOC";
    if std::env::var(PROTOC_ENVAR).is_err() {
        #[cfg(not(windows))]
        std::env::set_var(PROTOC_ENVAR, protobuf_src::protoc());
    }

    // Copied and adapted from
    // https://github.com/Kimundi/rustc-version-rs/blob/1d692a965f4e48a8cb72e82cda953107c0d22f47/README.md#example
    // Licensed under Apache-2.0 + MIT
    match version_meta().unwrap().channel {
        Channel::Stable => {
            println!("cargo:rustc-cfg=RUSTC_WITHOUT_SPECIALIZATION");
        }
        Channel::Beta => {
            println!("cargo:rustc-cfg=RUSTC_WITHOUT_SPECIALIZATION");
        }
        Channel::Nightly => {
            println!("cargo:rustc-cfg=RUSTC_WITH_SPECIALIZATION");
        }
        Channel::Dev => {
            println!("cargo:rustc-cfg=RUSTC_WITH_SPECIALIZATION");
            // See https://github.com/solana-labs/solana/issues/11055
            // We may be running the custom `rust-bpf-builder` toolchain,
            // which currently needs `#![feature(proc_macro_hygiene)]` to
            // be applied.
            println!("cargo:rustc-cfg=RUSTC_NEEDS_PROC_MACRO_HYGIENE");
        }
    }

    // Generate rust files from protos.
    prost_build::compile_protos(&["proto/wen_restart.proto"], &["proto/"])?;
    Ok(())
}
