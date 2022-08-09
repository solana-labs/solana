use std::{env, fs, process};

#[macro_use]
extern crate serial_test;

fn run_cargo_build(crate_name: &str, extra_args: &[&str], fail: bool) {
    let cwd = env::current_dir().expect("Unable to get current working directory");
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
    let mut cmd = assert_cmd::Command::cargo_bin("cargo-build-sbf").unwrap();
    let assert = cmd.env("RUST_LOG", "debug").args(&args).assert();
    if fail {
        assert.failure();
    } else {
        assert.success();
    }
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
    run_cargo_build("noop", &[], false);
    clean_target("noop");
}

#[test]
#[serial]
fn test_dump() {
    // This test requires rustfilt.
    assert!(process::Command::new("cargo")
        .args(&["install", "-f", "rustfilt"])
        .status()
        .expect("Unable to install rustfilt required for --dump option")
        .success());
    run_cargo_build("noop", &["--dump"], false);
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
    run_cargo_build("noop", &["--sbf-out-dir", "tmp_out"], false);
    let cwd = env::current_dir().expect("Unable to get current working directory");
    let dir = cwd.join("tmp_out");
    assert!(dir.exists());
    fs::remove_dir_all("tmp_out").expect("Failed to remove tmp_out dir");
    clean_target("noop");
}

#[test]
#[serial]
fn test_generate_child_script_on_failre() {
    run_cargo_build("fail", &["--generate-child-script-on-failure"], true);
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
