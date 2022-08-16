use std::{
    env, fs,
    io::{self, Write},
    process::{Command, Output},
};

#[macro_use]
extern crate serial_test;

fn run_cargo_build(crate_name: &str, extra_args: &[&str], test_name: &str) -> Output {
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
    let mut args = vec!["-v", "--sbf-sdk", "../bpf", "--manifest-path", &toml];
    for arg in extra_args {
        args.push(arg);
    }
    args.push("--");
    args.push("-vv");
    let cargo_build_sbf = root.join("target").join("debug").join("cargo-build-sbf");
    env::set_var("RUST_LOG", "debug");
    let output = Command::new(cargo_build_sbf)
        .args(&args)
        .output()
        .expect("Error running cargo-build-sbf");
    if !output.status.success() {
        eprintln!("--- stdout of {} ---", test_name);
        io::stderr().write_all(&output.stdout).unwrap();
        eprintln!("--- stderr of {} ---", test_name);
        io::stderr().write_all(&output.stderr).unwrap();
        eprintln!("--------------");
    }
    output
}

fn clean_target(crate_name: &str) {
    let cwd = env::current_dir().expect("Unable to get current working directory");
    let target = cwd
        .join("tests")
        .join("crates")
        .join(crate_name)
        .join("target");
    fs::remove_dir_all(target).expect("Failed to remove target dir");
}

#[test]
#[serial]
fn test_build() {
    let output = run_cargo_build("noop", &[], "test_build");
    assert!(output.status.success());
    clean_target("noop");
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
    let output = run_cargo_build("noop", &["--dump"], "test_dump");
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
    clean_target("noop");
}

#[test]
#[serial]
fn test_out_dir() {
    let output = run_cargo_build("noop", &["--sbf-out-dir", "tmp_out"], "test_out_dir");
    assert!(output.status.success());
    let cwd = env::current_dir().expect("Unable to get current working directory");
    let dir = cwd.join("tmp_out");
    assert!(dir.exists());
    fs::remove_dir_all("tmp_out").expect("Failed to remove tmp_out dir");
    clean_target("noop");
}

#[test]
#[serial]
fn test_generate_child_script_on_failre() {
    let output = run_cargo_build(
        "fail",
        &["--generate-child-script-on-failure"],
        "test_generate_child_script_on_failre",
    );
    assert!(!output.status.success());
    let cwd = env::current_dir().expect("Unable to get current working directory");
    let scr = cwd
        .join("tests")
        .join("crates")
        .join("fail")
        .join("cargo-build-sbf-child-script-cargo.sh");
    assert!(scr.exists());
    fs::remove_file(scr).expect("Failed to remove script");
    clean_target("fail");
}
