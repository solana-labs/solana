use {
    clap::{crate_description, crate_name, crate_version, Arg},
    log::*,
    std::{
        env,
        ffi::OsStr,
        fs::File,
        io::{prelude::*, BufWriter},
        path::{Path, PathBuf},
        process::{exit, Command},
    },
};

struct Config<'a> {
    sbf_sdk: Option<String>,
    sbf_out_dir: Option<String>,
    cargo: PathBuf,
    cargo_build_sbf: PathBuf,
    extra_cargo_test_args: Vec<String>,
    features: Vec<String>,
    packages: Vec<String>,
    generate_child_script_on_failure: bool,
    test_name: Option<String>,
    no_default_features: bool,
    no_run: bool,
    offline: bool,
    verbose: bool,
    workspace: bool,
    jobs: Option<String>,
    arch: &'a str,
}

impl Default for Config<'_> {
    fn default() -> Self {
        Self {
            sbf_sdk: None,
            sbf_out_dir: None,
            cargo: PathBuf::from("cargo"),
            cargo_build_sbf: PathBuf::from("cargo-build-sbf"),
            extra_cargo_test_args: vec![],
            features: vec![],
            packages: vec![],
            generate_child_script_on_failure: false,
            test_name: None,
            no_default_features: false,
            no_run: false,
            offline: false,
            verbose: false,
            workspace: false,
            jobs: None,
            arch: "sbf",
        }
    }
}

fn spawn<I, S>(program: &Path, args: I, generate_child_script_on_failure: bool)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let mut msg = format!("spawn: {}", program.display());
    for arg in args.iter() {
        msg = msg + &format!(" {}", arg.as_ref().to_str().unwrap_or("?")).to_string();
    }
    info!("{}", msg);

    let mut child = Command::new(program)
        .args(&args)
        .spawn()
        .unwrap_or_else(|err| {
            error!("Failed to execute {}: {}", program.display(), err);
            exit(1);
        });

    let exit_status = child.wait().expect("failed to wait on child");
    if !exit_status.success() {
        if !generate_child_script_on_failure {
            exit(1);
        }
        error!("cargo-test-sbf exited on command execution failure");
        let script_name = format!(
            "cargo-test-sbf-child-script-{}.sh",
            program.file_name().unwrap().to_str().unwrap(),
        );
        let file = File::create(&script_name).unwrap();
        let mut out = BufWriter::new(file);
        for (key, value) in env::vars() {
            writeln!(out, "{key}=\"{value}\" \\").unwrap();
        }
        write!(out, "{}", program.display()).unwrap();
        for arg in args.iter() {
            write!(out, " {}", arg.as_ref().to_str().unwrap_or("?")).unwrap();
        }
        writeln!(out).unwrap();
        out.flush().unwrap();
        error!(
            "To rerun the failed command for debugging use {}",
            script_name,
        );
        exit(1);
    }
}

fn test_sbf_package(config: &Config, target_directory: &Path, package: &cargo_metadata::Package) {
    let sbf_out_dir = config
        .sbf_out_dir
        .as_ref()
        .cloned()
        .unwrap_or_else(|| format!("{}", target_directory.join("deploy").display()));

    let manifest_path = format!("{}", package.manifest_path);
    let mut cargo_args = vec!["--manifest-path", &manifest_path];
    if config.no_default_features {
        cargo_args.push("--no-default-features");
    }
    for feature in &config.features {
        cargo_args.push("--features");
        cargo_args.push(feature);
    }
    if config.verbose {
        cargo_args.push("--verbose");
    }
    if let Some(jobs) = &config.jobs {
        cargo_args.push("--jobs");
        cargo_args.push(jobs);
    }

    let mut build_sbf_args = cargo_args.clone();
    if let Some(sbf_sdk) = config.sbf_sdk.as_ref() {
        build_sbf_args.push("--sbf-sdk");
        build_sbf_args.push(sbf_sdk);
    }
    build_sbf_args.push("--sbf-out-dir");
    build_sbf_args.push(&sbf_out_dir);

    build_sbf_args.push("--arch");
    build_sbf_args.push(config.arch);

    if !config.packages.is_empty() {
        build_sbf_args.push("--");
        for package in &config.packages {
            build_sbf_args.push("-p");
            build_sbf_args.push(package);
        }
    }

    spawn(
        &config.cargo_build_sbf,
        &build_sbf_args,
        config.generate_child_script_on_failure,
    );

    // Pass --sbf-out-dir along to the solana-program-test crate
    env::set_var("SBF_OUT_DIR", sbf_out_dir);

    cargo_args.insert(0, "test");

    if !config.packages.is_empty() {
        for package in &config.packages {
            cargo_args.push("-p");
            cargo_args.push(package);
        }
    }
    if let Some(test_name) = &config.test_name {
        cargo_args.push("--test");
        cargo_args.push(test_name);
    }

    if config.no_run {
        cargo_args.push("--no-run");
    }

    // If the program crate declares the "test-sbf" feature, pass it along to the tests so they can
    // distinguish between `cargo test` and `cargo test-sbf`
    if package.features.contains_key("test-sbf") {
        cargo_args.push("--features");
        cargo_args.push("test-sbf");
    }
    if package.features.contains_key("test-bpf") {
        cargo_args.push("--features");
        cargo_args.push("test-bpf");
    }
    for extra_cargo_test_arg in &config.extra_cargo_test_args {
        cargo_args.push(extra_cargo_test_arg);
    }
    spawn(
        &config.cargo,
        &cargo_args,
        config.generate_child_script_on_failure,
    );
}

fn test_sbf(config: Config, manifest_path: Option<PathBuf>) {
    let mut metadata_command = cargo_metadata::MetadataCommand::new();
    if let Some(manifest_path) = manifest_path.as_ref() {
        metadata_command.manifest_path(manifest_path);
    }
    if config.offline {
        metadata_command.other_options(vec!["--offline".to_string()]);
    }

    let metadata = metadata_command.exec().unwrap_or_else(|err| {
        error!("Failed to obtain package metadata: {}", err);
        exit(1);
    });

    if let Some(root_package) = metadata.root_package() {
        if !config.workspace
            && (config.packages.is_empty()
                || config
                    .packages
                    .iter()
                    .any(|p| root_package.id.repr.contains(p)))
        {
            debug!("test root package {:?}", root_package.id);
            test_sbf_package(&config, metadata.target_directory.as_ref(), root_package);
            return;
        }
    }

    let all_sbf_packages = metadata
        .packages
        .iter()
        .filter(|package| {
            if metadata.workspace_members.contains(&package.id) {
                for target in package.targets.iter() {
                    if target.kind.contains(&"cdylib".to_string()) {
                        return true;
                    }
                }
            }
            false
        })
        .collect::<Vec<_>>();

    for package in all_sbf_packages {
        if config.packages.is_empty() || config.packages.iter().any(|p| package.id.repr.contains(p))
        {
            debug!("test package {:?}", package.id);
            test_sbf_package(&config, metadata.target_directory.as_ref(), package);
        }
    }
}

fn main() {
    solana_logger::setup();
    let mut args = env::args().collect::<Vec<_>>();
    // When run as a cargo subcommand, the first program argument is the subcommand name.
    // Remove it
    if let Some(arg1) = args.get(1) {
        if arg1 == "test-sbf" {
            args.remove(1);
        }
    }

    let em_dash = "--".to_string();
    let args_contain_dashash = args.contains(&em_dash);

    let matches = clap::Command::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .trailing_var_arg(true)
        .arg(
            Arg::new("sbf_sdk")
                .long("sbf-sdk")
                .value_name("PATH")
                .takes_value(true)
                .help("Path to the Solana SBF SDK"),
        )
        .arg(
            Arg::new("features")
                .long("features")
                .value_name("FEATURES")
                .takes_value(true)
                .multiple_occurrences(true)
                .multiple_values(true)
                .help("Space-separated list of features to activate"),
        )
        .arg(
            Arg::new("no_default_features")
                .long("no-default-features")
                .takes_value(false)
                .help("Do not activate the `default` feature"),
        )
        .arg(
            Arg::new("test")
                .long("test")
                .value_name("NAME")
                .takes_value(true)
                .help("Test only the specified test target"),
        )
        .arg(
            Arg::new("manifest_path")
                .long("manifest-path")
                .value_name("PATH")
                .takes_value(true)
                .help("Path to Cargo.toml"),
        )
        .arg(
            Arg::new("packages")
                .long("package")
                .short('p')
                .value_name("SPEC")
                .takes_value(true)
                .multiple_occurrences(true)
                .multiple_values(true)
                .help("Package to run tests for"),
        )
        .arg(
            Arg::new("sbf_out_dir")
                .long("sbf-out-dir")
                .value_name("DIRECTORY")
                .takes_value(true)
                .help("Place final SBF build artifacts in this directory"),
        )
        .arg(
            Arg::new("no_run")
                .long("no-run")
                .takes_value(false)
                .help("Compile, but don't run tests"),
        )
        .arg(
            Arg::new("offline")
                .long("offline")
                .takes_value(false)
                .help("Run without accessing the network"),
        )
        .arg(
            Arg::new("generate_child_script_on_failure")
                .long("generate-child-script-on-failure")
                .takes_value(false)
                .help("Generate a shell script to rerun a failed subcommand"),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .takes_value(false)
                .help("Use verbose output"),
        )
        .arg(
            Arg::new("workspace")
                .long("workspace")
                .takes_value(false)
                .alias("all")
                .help("Test all SBF packages in the workspace"),
        )
        .arg(
            Arg::new("jobs")
                .short('j')
                .long("jobs")
                .takes_value(true)
                .value_name("N")
                .validator(|val| val.parse::<usize>().map_err(|e| e.to_string()))
                .help("Number of parallel jobs, defaults to # of CPUs"),
        )
        .arg(
            Arg::new("arch")
                .long("arch")
                .possible_values(["bpf", "sbf", "sbfv2"])
                .default_value("sbf")
                .help("Build for the given SBF version"),
        )
        .arg(
            Arg::new("extra_cargo_test_args")
                .value_name("extra args for cargo test and the test binary")
                .index(1)
                .multiple_occurrences(true)
                .multiple_values(true)
                .help("All extra arguments are passed through to cargo test"),
        )
        .get_matches_from(args);

    let mut config = Config {
        sbf_sdk: matches.value_of_t("sbf_sdk").ok(),
        sbf_out_dir: matches.value_of_t("sbf_out_dir").ok(),
        extra_cargo_test_args: matches
            .values_of_t("extra_cargo_test_args")
            .ok()
            .unwrap_or_default(),
        features: matches.values_of_t("features").ok().unwrap_or_default(),
        packages: matches.values_of_t("packages").ok().unwrap_or_default(),
        generate_child_script_on_failure: matches.is_present("generate_child_script_on_failure"),
        test_name: matches.value_of_t("test").ok(),
        no_default_features: matches.is_present("no_default_features"),
        no_run: matches.is_present("no_run"),
        offline: matches.is_present("offline"),
        verbose: matches.is_present("verbose"),
        workspace: matches.is_present("workspace"),
        jobs: matches.value_of_t("jobs").ok(),
        arch: matches.value_of("arch").unwrap(),
        ..Config::default()
    };

    if let Ok(cargo_build_sbf) = env::var("CARGO_BUILD_SBF") {
        config.cargo_build_sbf = PathBuf::from(cargo_build_sbf);
    }
    if let Ok(cargo_build_sbf) = env::var("CARGO") {
        config.cargo = PathBuf::from(cargo_build_sbf);
    }

    // clap.rs swallows "--" in the case when the user provides it as the first `extra_cargo_test_args`
    //
    // For example, this command-line "cargo-test-sbf -- --nocapture" results in `extra_cargo_test_args` only
    // containing "--nocapture".  This is a problem because `cargo test` will never see the `--`.
    //
    // Whereas "cargo-test-sbf testname --  --nocapture" correctly produces a `extra_cargo_test_args`
    // with "testname -- --nocapture".
    //
    // So if the original cargo-test-sbf arguments contain "--" but `extra_cargo_test_args` does
    // not, then prepend "--".
    //
    if args_contain_dashash && !config.extra_cargo_test_args.contains(&em_dash) {
        config.extra_cargo_test_args.insert(0, em_dash);
    }

    let manifest_path: Option<PathBuf> = matches.value_of_t("manifest_path").ok();
    test_sbf(config, manifest_path);
}
