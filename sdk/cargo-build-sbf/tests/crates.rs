use {
    predicates::prelude::*,
    std::{
        env, fs,
        sync::atomic::{AtomicBool, Ordering},
    },
};

#[macro_use]
extern crate serial_test;

static SBF_TOOLS_INSTALL: AtomicBool = AtomicBool::new(true);
fn run_cargo_build(crate_name: &str, extra_args: &[&str], fail: bool) {
    let cwd = env::current_dir().expect("Unable to get current working directory");
    let toml = cwd
        .join("tests")
        .join("crates")
        .join(crate_name)
        .join("Cargo.toml");
    let toml = format!("{}", toml.display());
    let mut args = vec!["-v", "--sbf-sdk", "../sbf", "--manifest-path", &toml];
    if SBF_TOOLS_INSTALL.fetch_and(false, Ordering::SeqCst) {
        args.push("--force-tools-install");
    }
    for arg in extra_args {
        args.push(arg);
    }
    args.push("--");
    args.push("-vv");
    let mut cmd = assert_cmd::Command::cargo_bin("cargo-build-sbf").unwrap();
    let assert = cmd.env("RUST_LOG", "debug").args(&args).assert();
    let output = assert.get_output();
    eprintln!("Test stdout\n{}\n", String::from_utf8_lossy(&output.stdout));
    eprintln!("Test stderr\n{}\n", String::from_utf8_lossy(&output.stderr));
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
    assert_cmd::Command::new("cargo")
        .args(["install", "rustfilt"])
        .assert()
        .success();
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
fn test_generate_child_script_on_failure() {
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

#[test]
#[serial]
fn test_sbfv2() {
    run_cargo_build("noop", &["--arch", "sbfv2"], false);
    let cwd = env::current_dir().expect("Unable to get current working directory");
    let bin = cwd
        .join("tests")
        .join("crates")
        .join("noop")
        .join("target")
        .join("deploy")
        .join("noop.so");
    let bin = bin.to_str().unwrap();
    let root = cwd
        .parent()
        .expect("Unable to get parent directory of current working dir")
        .parent()
        .expect("Unable to get ../.. of current working dir");
    let readelf = root
        .join("sdk")
        .join("sbf")
        .join("dependencies")
        .join("platform-tools")
        .join("llvm")
        .join("bin")
        .join("llvm-readelf");
    assert_cmd::Command::new(readelf)
        .args(["-h", bin])
        .assert()
        .stdout(predicate::str::contains(
            "Flags:                             0x20",
        ))
        .success();
    clean_target("noop");
}
