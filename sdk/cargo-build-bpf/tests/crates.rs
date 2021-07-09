use std::{
    env, fs,
    io::{self, Write},
    process::{Command, Output},
};

#[macro_use]
extern crate serial_test;

fn run_cargo_build(extra_args: &[&str]) -> Output {
    let cwd = env::current_dir().expect("Unable to get current working directory");
    let root = cwd
        .parent()
        .expect("Unable to get parent directory of current working dir")
        .parent()
        .expect("Unable to get ../.. of current working dir");
    let toml = cwd
        .join("tests")
        .join("crates")
        .join("noop")
        .join("Cargo.toml");
    let toml = format!("{}", toml.display());
    let mut args = vec!["--bpf-sdk", "../bpf", "--manifest-path", &toml];
    for arg in extra_args {
        args.push(arg);
    }
    let cargo_build_bpf = root.join("target").join("debug").join("cargo-build-bpf");
    let output = Command::new(cargo_build_bpf)
        .args(&args)
        .output()
        .expect("Error running cargo-build-bpf");
    if !output.status.success() {
        eprintln!("--- stdout ---");
        io::stderr().write_all(&output.stdout).unwrap();
        eprintln!("--- stderr ---");
        io::stderr().write_all(&output.stderr).unwrap();
        eprintln!("--------------");
    }
    output
}

#[test]
#[serial]
fn test_build() {
    let output = run_cargo_build(&[]);
    assert!(output.status.success());
}

#[test]
#[serial]
fn test_dump() {
    // This test requires rustfilt.
    assert!(Command::new("cargo")
        .args(&["install", "rustfilt"])
        .status()
        .expect("Unable to install rustfilt required for --dump option")
        .success());
    let output = run_cargo_build(&["--dump"]);
    assert!(output.status.success());
    let cwd = env::current_dir().expect("Unable to get current working directory");
    let dump = cwd
        .join("tests")
        .join("crates")
        .join("noop")
        .join("target")
        .join("deploy")
        .join("noop-dump.txt");
    assert!(dump.exists());
}

#[test]
#[serial]
fn test_out_dir() {
    let output = run_cargo_build(&["--bpf-out-dir", "tmp_out"]);
    assert!(output.status.success());
    let cwd = env::current_dir().expect("Unable to get current working directory");
    let dir = cwd.join("tmp_out");
    assert!(dir.exists());
    fs::remove_dir_all("tmp_out").expect("Failed to remove tmp_out dir");
}
