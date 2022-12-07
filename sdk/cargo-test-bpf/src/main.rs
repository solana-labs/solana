use std::{
    env,
    path::PathBuf,
    process::{exit, Command, Stdio},
};

fn main() {
    println!("Warning: cargo-test-bpf is deprecated. Please, use cargo-test-sbf");
    let mut args = env::args()
        .map(|x| {
            let s = x;
            s.replace("--bpf", "--sbf")
        })
        .collect::<Vec<_>>();
    if let Ok(cargo_build_bpf) = env::var("CARGO_BUILD_BPF") {
        let cargo_build_sbf = cargo_build_bpf.replace("build-bpf", "build-sbf");
        env::set_var("CARGO_BUILD_SBF", cargo_build_sbf);
    }
    let program = if let Some(arg0) = args.get(0) {
        let cargo_test_sbf = arg0.replace("test-bpf", "test-sbf");
        let cargo_build_sbf = cargo_test_sbf.replace("test-sbf", "build-sbf");
        env::set_var("CARGO_BUILD_SBF", cargo_build_sbf);
        args.remove(0);
        PathBuf::from(cargo_test_sbf)
    } else {
        PathBuf::from("cargo-test-sbf")
    };
    // When run as a cargo subcommand, the first program argument is the subcommand name.
    // Remove it
    if let Some(arg0) = args.get(0) {
        if arg0 == "test-bpf" {
            args.remove(0);
        }
    }
    let index = args.iter().position(|x| x == "--").unwrap_or(args.len());
    args.insert(index, "bpf".to_string());
    args.insert(index, "--arch".to_string());
    print!("cargo-test-bpf child: {}", program.display());
    for a in &args {
        print!(" {a}");
    }
    println!();
    let child = Command::new(&program)
        .args(&args)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| {
            eprintln!("Failed to execute {}: {}", program.display(), err);
            exit(1);
        });

    let output = child.wait_with_output().expect("failed to wait on child");
    println!(
        "{}",
        output
            .stdout
            .as_slice()
            .iter()
            .map(|&c| c as char)
            .collect::<String>()
    );
    let code = output.status.code().unwrap_or(1);
    exit(code);
}
