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
    let mut metadata_command = cargo_metadata::MetadataCommand::new();
    if let Some(manifest_path) = config.manifest_path.as_ref() {
        metadata_command.manifest_path(manifest_path);
    }

    let metadata = metadata_command.exec().unwrap_or_else(|err| {
        eprintln!("Failed to obtain package metadata: {}", err);
        exit(1);
    });

    let bpf_out_dir = config
        .bpf_out_dir
        .unwrap_or_else(|| format!("{}", metadata.target_directory.join("deploy").display()));

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
    build_bpf_args.push("--bpf-out-dir");
    build_bpf_args.push(&bpf_out_dir);

    spawn(&config.cargo_build_bpf, &build_bpf_args);

    // Pass --bpf-out-dir along to the solana-program-test crate
    env::set_var("BPF_OUT_DIR", bpf_out_dir);

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

    let em_dash = "--".to_string();
    let args_contain_dashash = args.contains(&em_dash);

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
                .value_name("extra args for cargo test and the test binary")
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

    // clap.rs swallows "--" in the case when the user provides it as the first `extra_cargo_test_args`
    //
    // For example, this command-line "cargo-test-bpf -- --nocapture" results in `extra_cargo_test_args` only
    // containing "--nocapture".  This is a problem because `cargo test` will never see the `--`.
    //
    // Whereas "cargo-test-bpf testname --  --nocapture" correctly produces a `extra_cargo_test_args`
    // with "testname -- --nocapture".
    //
    // So if the original cargo-test-bpf arguments contain "--" but `extra_cargo_test_args` does
    // not, then prepend "--".
    //
    if args_contain_dashash && !config.extra_cargo_test_args.contains(&em_dash) {
        config.extra_cargo_test_args.insert(0, em_dash);
    }

    test_bpf(config);
}
