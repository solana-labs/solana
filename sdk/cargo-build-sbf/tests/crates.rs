use std::{
    env, fs,
    io::{self, Write},
    process::{Command, Output},
};

#[macro_use]
extern crate serial_test;

fn run_cargo_build(crate_name: &str, extra_args: &[&str]) -> Output {
    let cwd = env::current_dir().expect("Unable to get current working directory");
    let root = cwd
        .parent()
        .expect("Unable to get parent directory of current working dir")
        .parent()
        .expect("Unable to get ../.. of current working dir");
    let toml = cwd
        .join("tests")
        .join("crates")
        .join(crate_name)
        .join("Cargo.toml");
    let toml = format!("{}", toml.display());
    let mut args = vec!["--sbf-sdk", "../bpf", "--manifest-path", &toml];
    for arg in extra_args {
        args.push(arg);
    }
    let cargo_build_sbf = root.join("target").join("debug").join("cargo-build-sbf");
    let output = Command::new(cargo_build_sbf)
        .args(&args)
        .output()
        .expect("Error running cargo-build-sbf");
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
    let output = run_cargo_build("noop", &[]);
    assert!(output.status.success());
}

#[test]
#[serial]
fn test_dump() {
    // This test requires rustfilt.
    assert!(Command::new("cargo")
        .args(&["install", "-f", "rustfilt"])
        .status()
        .expect("Unable to install rustfilt required for --dump option")
        .success());
    let output = run_cargo_build("noop", &["--dump"]);
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
    let output = run_cargo_build("noop", &["--sbf-out-dir", "tmp_out"]);
    assert!(output.status.success());
    let cwd = env::current_dir().expect("Unable to get current working directory");
    let dir = cwd.join("tmp_out");
    assert!(dir.exists());
    fs::remove_dir_all("tmp_out").expect("Failed to remove tmp_out dir");
}

#[test]
#[serial]
fn test_generate_child_script_on_failre() {
    let output = run_cargo_build("fail", &["--generate-child-script-on-failure"]);
    assert!(!output.status.success());
    let cwd = env::current_dir().expect("Unable to get current working directory");
    let scr = cwd
        .join("tests")
        .join("crates")
        .join("fail")
        .join("cargo-build-sbf-child-script-cargo.sh");
    assert!(scr.exists());
    fs::remove_file(scr).expect("Failed to remove script");
}
