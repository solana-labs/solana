use clap::{
    crate_description, crate_name, crate_version, value_t, values_t, App, AppSettings, Arg,
};
use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::exit,
    process::Command,
};

struct Config {
    bpf_sdk: Option<String>,
    bpf_out_dir: Option<String>,
    cargo: PathBuf,
    cargo_build_bpf: PathBuf,
    extra_cargo_test_args: Vec<String>,
    features: Vec<String>,
    manifest_path: Option<String>,
    no_default_features: bool,
    verbose: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bpf_sdk: None,
            bpf_out_dir: None,
            cargo: PathBuf::from("cargo"),
            cargo_build_bpf: PathBuf::from("cargo-build-bpf"),
            extra_cargo_test_args: vec![],
            features: vec![],
            manifest_path: None,
            no_default_features: false,
            verbose: false,
        }
    }
}

fn spawn<I, S>(program: &Path, args: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    print!("Running: {}", program.display());
    for arg in args.iter() {
        print!(" {}", arg.as_ref().to_str().unwrap_or("?"));
    }
    println!();

    let mut child = Command::new(program)
        .args(&args)
        .spawn()
        .unwrap_or_else(|err| {
            eprintln!("Failed to execute {}: {}", program.display(), err);
            exit(1);
        });

    let exit_status = child.wait().expect("failed to wait on child");
    if !exit_status.success() {
        exit(1);
    }
}

fn test_bpf(config: Config) {
    let mut cargo_args = vec![];
    if config.no_default_features {
        cargo_args.push("--no-default-features");
    }
    for feature in &config.features {
        cargo_args.push("--features");
        cargo_args.push(feature);
    }
    if let Some(manifest_path) = config.manifest_path.as_ref() {
        cargo_args.push("--manifest-path");
        cargo_args.push(&manifest_path);
    }
    if config.verbose {
        cargo_args.push("--verbose");
    }

    let mut build_bpf_args = cargo_args.clone();
    if let Some(bpf_sdk) = config.bpf_sdk.as_ref() {
        build_bpf_args.push("--bpf-sdk");
        build_bpf_args.push(bpf_sdk);
    }
    if let Some(bpf_out_dir) = config.bpf_out_dir.as_ref() {
        build_bpf_args.push("--bpf-out-dir");
        build_bpf_args.push(bpf_out_dir);
    }
    spawn(&config.cargo_build_bpf, &build_bpf_args);

    env::set_var("bpf", "1"); // Hint to solana-program-test that it should load BPF programs
    cargo_args.insert(0, "test");
    for extra_cargo_test_arg in &config.extra_cargo_test_args {
        cargo_args.push(&extra_cargo_test_arg);
    }
    spawn(&config.cargo, &cargo_args);
}

fn main() {
    let mut args = env::args().collect::<Vec<_>>();
    // When run as a cargo subcommand, the first program argument is the subcommand name.
    // Remove it
    if let Some(arg1) = args.get(1) {
        if arg1 == "test-bpf" {
            args.remove(1);
        }
    }

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .setting(AppSettings::TrailingVarArg)
        .arg(
            Arg::with_name("bpf_sdk")
                .long("bpf-sdk")
                .value_name("PATH")
                .takes_value(true)
                .help("Path to the Solana BPF SDK"),
        )
        .arg(
            Arg::with_name("features")
                .long("features")
                .value_name("FEATURES")
                .takes_value(true)
                .multiple(true)
                .help("Space-separated list of features to activate"),
        )
        .arg(
            Arg::with_name("no_default_features")
                .long("no-default-features")
                .takes_value(false)
                .help("Do not activate the `default` feature"),
        )
        .arg(
            Arg::with_name("manifest_path")
                .long("manifest-path")
                .value_name("PATH")
                .takes_value(true)
                .help("Path to Cargo.toml"),
        )
        .arg(
            Arg::with_name("bpf_out_dir")
                .long("bpf-out-dir")
                .value_name("DIRECTORY")
                .takes_value(true)
                .help("Place final BPF build artifacts in this directory"),
        )
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .takes_value(false)
                .help("Use verbose output"),
        )
        .arg(
            Arg::with_name("extra_cargo_test_args")
                .value_name("extra args for cargo test")
                .index(1)
                .multiple(true)
                .help("All extra arguments are passed through to cargo test"),
        )
        .get_matches_from(args);

    let mut config = Config {
        bpf_sdk: value_t!(matches, "bpf_sdk", String).ok(),
        bpf_out_dir: value_t!(matches, "bpf_out_dir", String).ok(),
        extra_cargo_test_args: values_t!(matches, "extra_cargo_test_args", String)
            .ok()
            .unwrap_or_else(Vec::new),
        features: values_t!(matches, "features", String)
            .ok()
            .unwrap_or_else(Vec::new),
        manifest_path: value_t!(matches, "manifest_path", String).ok(),
        no_default_features: matches.is_present("no_default_features"),
        verbose: matches.is_present("verbose"),
        ..Config::default()
    };

    if let Ok(cargo_build_bpf) = env::var("CARGO_BUILD_BPF") {
        config.cargo_build_bpf = PathBuf::from(cargo_build_bpf);
    }
    if let Ok(cargo_build_bpf) = env::var("CARGO") {
        config.cargo = PathBuf::from(cargo_build_bpf);
    }

    test_bpf(config);

    // TODO: args after -- go to ct
}
