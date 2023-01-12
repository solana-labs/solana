extern crate rustc_version;
use {
    rustc_version::{version_meta, Channel},
    std::{process::Command},
};

fn main() {
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

    if let Ok(output) = Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .output()
    {
        let maybe_commit = output.stdout.as_slice();
        if !output.stdout.is_empty() {
            if let Ok(commit) = std::str::from_utf8(maybe_commit) {
                println!("cargo:rustc-env=DEV_COMMIT={commit}");
            }
        }
    }
}
