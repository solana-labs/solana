use std::{
    env,
    path::PathBuf,
    process::{exit, Command, Stdio},
};

fn main() {
    println!("Warning: cargo-build-bpf is deprecated. Please, use cargo-build-sbf");
    let mut args = env::args()
        .map(|x| {
            let s = x;
            s.replace("--bpf", "--sbf")
        })
        .collect::<Vec<_>>();
    let program = if let Some(arg0) = args.get(0) {
        let arg0 = arg0.replace("build-bpf", "build-sbf");
        args.remove(0);
        PathBuf::from(arg0)
    } else {
        PathBuf::from("cargo-build-sbf")
    };
    // When run as a cargo subcommand, the first program argument is the subcommand name.
    // Remove it
    if let Some(arg0) = args.get(0) {
        if arg0 == "build-bpf" {
            args.remove(0);
        }
    }
    args.push("--arch".to_string());
    args.push("bpf".to_string());
    print!("cargo-build-bpf child: {}", program.display());
    for a in &args {
        print!(" {}", a);
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
