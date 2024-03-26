extern crate walkdir;

use {
    std::{env, path::Path, process::Command},
    walkdir::WalkDir,
};

fn rerun_if_changed(files: &[&str], directories: &[&str], excludes: &[&str]) {
    let mut all_files: Vec<_> = files.iter().map(|f| f.to_string()).collect();

    for directory in directories {
        let files_in_directory: Vec<_> = WalkDir::new(directory)
            .into_iter()
            .map(|entry| entry.unwrap())
            .filter(|entry| {
                if !entry.file_type().is_file() {
                    return false;
                }
                for exclude in excludes.iter() {
                    if entry.path().to_str().unwrap().contains(exclude) {
                        return false;
                    }
                }
                true
            })
            .map(|f| f.path().to_str().unwrap().to_owned())
            .collect();
        all_files.extend_from_slice(&files_in_directory[..]);
    }

    for file in all_files {
        if !Path::new(&file).is_file() {
            panic!("{file} is not a file");
        }
        println!("cargo:rerun-if-changed={file}");
    }
}

fn main() {
    if env::var("CARGO_FEATURE_DUMMY_FOR_CI_CHECK").is_ok() {
        println!("cargo:warning=(not a warning) Compiling with host toolchain for CI...");
        return;
    }

    let build_profile = env::var("PROFILE").expect("`PROFILE` envvar to be set");
    let install_dir = format!("target/{build_profile}/sbf");
    let sbf_c = env::var("CARGO_FEATURE_SBF_C").is_ok();
    if sbf_c {
        let install_dir = format!("OUT_DIR=../{install_dir}");
        println!("cargo:warning=(not a warning) Building C-based on-chain programs");
        assert!(Command::new("make")
            .current_dir("c")
            .arg("programs")
            .arg(&install_dir)
            .status()
            .expect("Failed to build C-based SBF programs")
            .success());

        rerun_if_changed(&["c/makefile"], &["c/src", "../../sdk"], &["/target/"]);
    }

    let sbf_rust = env::var("CARGO_FEATURE_SBF_RUST").is_ok();
    if sbf_rust {
        let rust_programs = [
            "128bit",
            "alloc",
            "alt_bn128",
            "alt_bn128_compression",
            "big_mod_exp",
            "call_depth",
            "caller_access",
            "curve25519",
            "custom_heap",
            "dep_crate",
            "deprecated_loader",
            "dup_accounts",
            "error_handling",
            "log_data",
            "external_spend",
            "finalize",
            "get_minimum_delegation",
            "inner_instruction_alignment_check",
            "instruction_introspection",
            "invoke",
            "invoke_and_error",
            "invoke_and_ok",
            "invoke_and_return",
            "invoked",
            "iter",
            "many_args",
            "mem",
            "membuiltins",
            "noop",
            "panic",
            "param_passing",
            "poseidon",
            "rand",
            "realloc",
            "realloc_invoke",
            "remaining_compute_units",
            "ro_modify",
            "ro_account_modify",
            "sanity",
            "secp256k1_recover",
            "sha",
            "sibling_inner_instruction",
            "sibling_instruction",
            "simulation",
            "spoof1",
            "spoof1_system",
            "upgradeable",
            "upgraded",
        ];
        for program in rust_programs.iter() {
            println!("cargo:warning=(not a warning) Building Rust-based on-chain programs: solana_sbf_rust_{program}");
            assert!(Command::new("../../cargo-build-sbf")
                .args([
                    "--manifest-path",
                    &format!("rust/{program}/Cargo.toml"),
                    "--sbf-out-dir",
                    &install_dir
                ])
                .status()
                .expect("Error calling cargo-build-sbf from build.rs")
                .success());
        }

        rerun_if_changed(&[], &["rust", "../../sdk", &install_dir], &["/target/"]);
    }
}
