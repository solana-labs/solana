use {
    bzip2::bufread::BzDecoder,
    clap::{
        crate_description, crate_name, crate_version, value_t, value_t_or_exit, values_t, App, Arg,
    },
    regex::Regex,
    solana_download_utils::download_file,
    solana_sdk::signature::{write_keypair_file, Keypair},
    std::{
        collections::{HashMap, HashSet},
        env,
        ffi::OsStr,
        fs::{self, File},
        io::{prelude::*, BufReader, BufWriter},
        path::{Path, PathBuf},
        process::{exit, Command, Stdio},
        str::FromStr,
    },
    tar::Archive,
};

struct Config<'a> {
    cargo_args: Option<Vec<&'a str>>,
    bpf_out_dir: Option<PathBuf>,
    bpf_sdk: PathBuf,
    dump: bool,
    features: Vec<String>,
    generate_child_script_on_failure: bool,
    no_default_features: bool,
    offline: bool,
    verbose: bool,
    workspace: bool,
}

impl Default for Config<'_> {
    fn default() -> Self {
        Self {
            cargo_args: None,
            bpf_sdk: env::current_exe()
                .expect("Unable to get current executable")
                .parent()
                .expect("Unable to get parent directory")
                .to_path_buf()
                .join("sdk")
                .join("bpf"),
            bpf_out_dir: None,
            dump: false,
            features: vec![],
            generate_child_script_on_failure: false,
            no_default_features: false,
            offline: false,
            verbose: false,
            workspace: false,
        }
    }
}

fn spawn<I, S>(program: &Path, args: I, generate_child_script_on_failure: bool) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    print!("cargo-build-bpf child: {}", program.display());
    for arg in args.iter() {
        print!(" {}", arg.as_ref().to_str().unwrap_or("?"));
    }
    println!();

    let child = Command::new(program)
        .args(&args)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| {
            eprintln!("Failed to execute {}: {}", program.display(), err);
            exit(1);
        });

    let output = child.wait_with_output().expect("failed to wait on child");
    if !output.status.success() {
        if !generate_child_script_on_failure {
            exit(1);
        }
        eprintln!("cargo-build-bpf exited on command execution failure");
        let script_name = format!(
            "cargo-build-bpf-child-script-{}.sh",
            program.file_name().unwrap().to_str().unwrap(),
        );
        let file = File::create(&script_name).unwrap();
        let mut out = BufWriter::new(file);
        for (key, value) in env::vars() {
            writeln!(out, "{}=\"{}\" \\", key, value).unwrap();
        }
        write!(out, "{}", program.display()).unwrap();
        for arg in args.iter() {
            write!(out, " {}", arg.as_ref().to_str().unwrap_or("?")).unwrap();
        }
        writeln!(out).unwrap();
        out.flush().unwrap();
        eprintln!(
            "To rerun the failed command for debugging use {}",
            script_name,
        );
        exit(1);
    }
    output
        .stdout
        .as_slice()
        .iter()
        .map(|&c| c as char)
        .collect::<String>()
}

// Check whether a package is installed and install it if missing.
fn install_if_missing(
    config: &Config,
    package: &str,
    version: &str,
    url: &str,
    download_file_name: &str,
    target_path: &Path,
) -> Result<(), String> {
    // Check whether the target path is an empty directory. This can
    // happen if package download failed on previous run of
    // cargo-build-bpf.  Remove the target_path directory in this
    // case.
    if target_path.is_dir()
        && target_path
            .read_dir()
            .map_err(|err| err.to_string())?
            .next()
            .is_none()
    {
        fs::remove_dir(&target_path).map_err(|err| err.to_string())?;
    }

    // Check whether the package is already in ~/.cache/solana.
    // Download it and place in the proper location if not found.
    if !target_path.is_dir()
        && !target_path
            .symlink_metadata()
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
    {
        if target_path.exists() {
            fs::remove_file(&target_path).map_err(|err| err.to_string())?;
        }
        fs::create_dir_all(&target_path).map_err(|err| err.to_string())?;
        let mut url = String::from(url);
        url.push('/');
        url.push_str(version);
        url.push('/');
        url.push_str(download_file_name);
        let download_file_path = target_path.join(download_file_name);
        if download_file_path.exists() {
            fs::remove_file(&download_file_path).map_err(|err| err.to_string())?;
        }
        download_file(url.as_str(), &download_file_path, true, &mut None)?;
        let zip = File::open(&download_file_path).map_err(|err| err.to_string())?;
        let tar = BzDecoder::new(BufReader::new(zip));
        let mut archive = Archive::new(tar);
        archive
            .unpack(&target_path)
            .map_err(|err| err.to_string())?;
        fs::remove_file(download_file_path).map_err(|err| err.to_string())?;
    }
    // Make a symbolic link source_path -> target_path in the
    // sdk/bpf/dependencies directory if no valid link found.
    let source_base = config.bpf_sdk.join("dependencies");
    if !source_base.exists() {
        fs::create_dir_all(&source_base).map_err(|err| err.to_string())?;
    }
    let source_path = source_base.join(package);
    // Check whether the correct symbolic link exists.
    let invalid_link = if let Ok(link_target) = source_path.read_link() {
        if link_target.ne(target_path) {
            fs::remove_file(&source_path).map_err(|err| err.to_string())?;
            true
        } else {
            false
        }
    } else {
        true
    };
    if invalid_link {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target_path, source_path).map_err(|err| err.to_string())?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(target_path, source_path)
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

// Process dump file attributing call instructions with callee function names
fn postprocess_dump(program_dump: &Path) {
    if !program_dump.exists() {
        return;
    }
    let postprocessed_dump = program_dump.with_extension("postprocessed");
    let head_re = Regex::new(r"^([0-9a-f]{16}) (.+)").unwrap();
    let insn_re = Regex::new(r"^ +([0-9]+)((\s[0-9a-f]{2})+)\s.+").unwrap();
    let call_re = Regex::new(r"^ +([0-9]+)(\s[0-9a-f]{2})+\scall (-?)0x([0-9a-f]+)").unwrap();
    let relo_re = Regex::new(r"^([0-9a-f]{16})  [0-9a-f]{16} R_BPF_64_32 +0{16} (.+)").unwrap();
    let mut a2n: HashMap<i64, String> = HashMap::new();
    let mut rel: HashMap<u64, String> = HashMap::new();
    let mut name = String::from("");
    let mut state = 0;
    let file = match File::open(program_dump) {
        Ok(x) => x,
        _ => return,
    };
    for line_result in BufReader::new(file).lines() {
        let line = line_result.unwrap();
        let line = line.trim_end();
        if line == "Disassembly of section .text" {
            state = 1;
        }
        if state == 0 {
            if relo_re.is_match(line) {
                let captures = relo_re.captures(line).unwrap();
                let address = u64::from_str_radix(&captures[1], 16).unwrap();
                let symbol = captures[2].to_string();
                rel.insert(address, symbol);
            }
        } else if state == 1 {
            if head_re.is_match(line) {
                state = 2;
                let captures = head_re.captures(line).unwrap();
                name = captures[2].to_string();
            }
        } else if state == 2 {
            state = 1;
            if insn_re.is_match(line) {
                let captures = insn_re.captures(line).unwrap();
                let address = i64::from_str(&captures[1]).unwrap();
                a2n.insert(address, name.clone());
            }
        }
    }
    let file = match File::create(&postprocessed_dump) {
        Ok(x) => x,
        _ => return,
    };
    let mut out = BufWriter::new(file);
    let file = match File::open(program_dump) {
        Ok(x) => x,
        _ => return,
    };
    let mut pc = 0u64;
    let mut step = 0u64;
    for line_result in BufReader::new(file).lines() {
        let line = line_result.unwrap();
        let line = line.trim_end();
        if head_re.is_match(line) {
            let captures = head_re.captures(line).unwrap();
            pc = u64::from_str_radix(&captures[1], 16).unwrap();
            writeln!(out, "{}", line).unwrap();
            continue;
        }
        if insn_re.is_match(line) {
            let captures = insn_re.captures(line).unwrap();
            step = if captures[2].len() > 24 { 16 } else { 8 };
        }
        if call_re.is_match(line) {
            if rel.contains_key(&pc) {
                writeln!(out, "{} ; {}", line, rel[&pc]).unwrap();
            } else {
                let captures = call_re.captures(line).unwrap();
                let pc = i64::from_str(&captures[1]).unwrap().checked_add(1).unwrap();
                let offset = i64::from_str_radix(&captures[4], 16).unwrap();
                let offset = if &captures[3] == "-" {
                    offset.checked_neg().unwrap()
                } else {
                    offset
                };
                let address = pc.checked_add(offset).unwrap();
                if a2n.contains_key(&address) {
                    writeln!(out, "{} ; {}", line, a2n[&address]).unwrap();
                } else {
                    writeln!(out, "{}", line).unwrap();
                }
            }
        } else {
            writeln!(out, "{}", line).unwrap();
        }
        pc = pc.checked_add(step).unwrap();
    }
    fs::rename(postprocessed_dump, program_dump).unwrap();
}

// Check whether the built .so file contains undefined symbols that are
// not known to the runtime and warn about them if any.
fn check_undefined_symbols(config: &Config, program: &Path) {
    let syscalls_txt = config.bpf_sdk.join("syscalls.txt");
    let file = match File::open(syscalls_txt) {
        Ok(x) => x,
        _ => return,
    };
    let mut syscalls = HashSet::new();
    for line_result in BufReader::new(file).lines() {
        let line = line_result.unwrap();
        let line = line.trim_end();
        syscalls.insert(line.to_string());
    }
    let entry =
        Regex::new(r"^ *[0-9]+: [0-9a-f]{16} +[0-9a-f]+ +NOTYPE +GLOBAL +DEFAULT +UND +(.+)")
            .unwrap();
    let readelf = config
        .bpf_sdk
        .join("dependencies")
        .join("bpf-tools")
        .join("llvm")
        .join("bin")
        .join("llvm-readelf");
    let mut readelf_args = vec!["--dyn-symbols"];
    readelf_args.push(program.to_str().unwrap());
    let output = spawn(
        &readelf,
        &readelf_args,
        config.generate_child_script_on_failure,
    );
    if config.verbose {
        println!("{}", output);
    }
    let mut unresolved_symbols: Vec<String> = Vec::new();
    for line in output.lines() {
        let line = line.trim_end();
        if entry.is_match(line) {
            let captures = entry.captures(line).unwrap();
            let symbol = captures[1].to_string();
            if !syscalls.contains(&symbol) {
                unresolved_symbols.push(symbol);
            }
        }
    }
    if !unresolved_symbols.is_empty() {
        println!(
            "Warning: the following functions are undefined and not known syscalls {:?}.",
            unresolved_symbols
        );
        println!("         Calling them will trigger a run-time error.");
    }
}

// check whether custom BPF toolchain is linked, and link it if it is not.
fn link_bpf_toolchain(config: &Config) {
    let toolchain_path = config
        .bpf_sdk
        .join("dependencies")
        .join("bpf-tools")
        .join("rust");
    let rustup = PathBuf::from("rustup");
    let rustup_args = vec!["toolchain", "list", "-v"];
    let rustup_output = spawn(
        &rustup,
        &rustup_args,
        config.generate_child_script_on_failure,
    );
    if config.verbose {
        println!("{}", rustup_output);
    }
    let mut do_link = true;
    for line in rustup_output.lines() {
        if line.starts_with("bpf") {
            let mut it = line.split_whitespace();
            let _ = it.next();
            let path = it.next();
            if path.unwrap() != toolchain_path.to_str().unwrap() {
                let rustup_args = vec!["toolchain", "uninstall", "bpf"];
                let output = spawn(
                    &rustup,
                    &rustup_args,
                    config.generate_child_script_on_failure,
                );
                if config.verbose {
                    println!("{}", output);
                }
            } else {
                do_link = false;
            }
            break;
        }
    }
    if do_link {
        let rustup_args = vec!["toolchain", "link", "bpf", toolchain_path.to_str().unwrap()];
        let output = spawn(
            &rustup,
            &rustup_args,
            config.generate_child_script_on_failure,
        );
        if config.verbose {
            println!("{}", output);
        }
    }
}

fn build_bpf_package(config: &Config, target_directory: &Path, package: &cargo_metadata::Package) {
    let program_name = {
        let cdylib_targets = package
            .targets
            .iter()
            .filter_map(|target| {
                if target.crate_types.contains(&"cdylib".to_string()) {
                    Some(&target.name)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        match cdylib_targets.len() {
            0 => {
                println!(
                    "Note: {} crate does not contain a cdylib target",
                    package.name
                );
                None
            }
            1 => Some(cdylib_targets[0].replace("-", "_")),
            _ => {
                eprintln!(
                    "{} crate contains multiple cdylib targets: {:?}",
                    package.name, cdylib_targets
                );
                exit(1);
            }
        }
    };

    let legacy_program_feature_present = package.name == "solana-sdk";
    let root_package_dir = &package.manifest_path.parent().unwrap_or_else(|| {
        eprintln!("Unable to get directory of {}", package.manifest_path);
        exit(1);
    });

    let bpf_out_dir = config
        .bpf_out_dir
        .as_ref()
        .cloned()
        .unwrap_or_else(|| target_directory.join("deploy"));

    let target_build_directory = target_directory
        .join("bpfel-unknown-unknown")
        .join("release");

    env::set_current_dir(&root_package_dir).unwrap_or_else(|err| {
        eprintln!(
            "Unable to set current directory to {}: {}",
            root_package_dir, err
        );
        exit(1);
    });

    println!("BPF SDK: {}", config.bpf_sdk.display());
    if config.no_default_features {
        println!("No default features");
    }
    if !config.features.is_empty() {
        println!("Features: {}", config.features.join(" "));
    }
    if legacy_program_feature_present {
        println!("Legacy program feature detected");
    }
    let bpf_tools_download_file_name = if cfg!(target_os = "windows") {
        "solana-bpf-tools-windows.tar.bz2"
    } else if cfg!(target_os = "macos") {
        "solana-bpf-tools-osx.tar.bz2"
    } else {
        "solana-bpf-tools-linux.tar.bz2"
    };

    let home_dir = PathBuf::from(env::var("HOME").unwrap_or_else(|err| {
        eprintln!("Can't get home directory path: {}", err);
        exit(1);
    }));

    // The following line is scanned by CI configuration script to
    // separate cargo caches according to the version of sbf-tools.
    let bpf_tools_version = "v1.25";
    let package = "bpf-tools";
    let target_path = home_dir
        .join(".cache")
        .join("solana")
        .join(bpf_tools_version)
        .join(package);
    install_if_missing(
        config,
        package,
        bpf_tools_version,
        "https://github.com/solana-labs/bpf-tools/releases/download",
        bpf_tools_download_file_name,
        &target_path,
    )
    .unwrap_or_else(|err| {
        // The package version directory doesn't contain a valid
        // installation, and it should be removed.
        let target_path_parent = target_path.parent().expect("Invalid package path");
        fs::remove_dir_all(&target_path_parent).unwrap_or_else(|err| {
            eprintln!(
                "Failed to remove {} while recovering from installation failure: {}",
                target_path_parent.to_string_lossy(),
                err,
            );
            exit(1);
        });
        eprintln!("Failed to install bpf-tools: {}", err);
        exit(1);
    });
    link_bpf_toolchain(config);

    let llvm_bin = config
        .bpf_sdk
        .join("dependencies")
        .join("bpf-tools")
        .join("llvm")
        .join("bin");
    env::set_var("CC", llvm_bin.join("clang"));
    env::set_var("AR", llvm_bin.join("llvm-ar"));
    env::set_var("OBJDUMP", llvm_bin.join("llvm-objdump"));
    env::set_var("OBJCOPY", llvm_bin.join("llvm-objcopy"));
    const RF_LTO: &str = "-C lto=no";
    let mut rustflags = match env::var("RUSTFLAGS") {
        Ok(rf) => {
            if rf.contains(&RF_LTO) {
                rf
            } else {
                format!("{} {}", rf, RF_LTO)
            }
        }
        _ => RF_LTO.to_string(),
    };
    if cfg!(windows) && !rustflags.contains("-C linker=") {
        let ld_path = llvm_bin.join("ld.lld");
        rustflags = format!("{} -C linker={}", rustflags, ld_path.display());
    }
    if config.verbose {
        println!("RUSTFLAGS={}", rustflags);
    }
    env::set_var("RUSTFLAGS", rustflags);
    let cargo_build = PathBuf::from("cargo");
    let mut cargo_build_args = vec![
        "+bpf",
        "build",
        "--target",
        "bpfel-unknown-unknown",
        "--release",
    ];
    if config.no_default_features {
        cargo_build_args.push("--no-default-features");
    }
    for feature in &config.features {
        cargo_build_args.push("--features");
        cargo_build_args.push(feature);
    }
    if legacy_program_feature_present {
        if !config.no_default_features {
            cargo_build_args.push("--no-default-features");
        }
        cargo_build_args.push("--features=program");
    }
    if config.verbose {
        cargo_build_args.push("--verbose");
    }
    if let Some(args) = &config.cargo_args {
        for arg in args {
            cargo_build_args.push(arg);
        }
    }
    let output = spawn(
        &cargo_build,
        &cargo_build_args,
        config.generate_child_script_on_failure,
    );
    if config.verbose {
        println!("{}", output);
    }

    if let Some(program_name) = program_name {
        let program_unstripped_so = target_build_directory.join(&format!("{}.so", program_name));
        let program_dump = bpf_out_dir.join(&format!("{}-dump.txt", program_name));
        let program_so = bpf_out_dir.join(&format!("{}.so", program_name));
        let program_keypair = bpf_out_dir.join(&format!("{}-keypair.json", program_name));

        fn file_older_or_missing(prerequisite_file: &Path, target_file: &Path) -> bool {
            let prerequisite_metadata = fs::metadata(prerequisite_file).unwrap_or_else(|err| {
                eprintln!(
                    "Unable to get file metadata for {}: {}",
                    prerequisite_file.display(),
                    err
                );
                exit(1);
            });

            if let Ok(target_metadata) = fs::metadata(target_file) {
                use std::time::UNIX_EPOCH;
                prerequisite_metadata.modified().unwrap_or(UNIX_EPOCH)
                    > target_metadata.modified().unwrap_or(UNIX_EPOCH)
            } else {
                true
            }
        }

        if !program_keypair.exists() {
            write_keypair_file(&Keypair::new(), &program_keypair).unwrap_or_else(|err| {
                eprintln!(
                    "Unable to get create {}: {}",
                    program_keypair.display(),
                    err
                );
                exit(1);
            });
        }

        if file_older_or_missing(&program_unstripped_so, &program_so) {
            #[cfg(windows)]
            let output = spawn(
                &llvm_bin.join("llvm-objcopy"),
                &[
                    "--strip-all".as_ref(),
                    program_unstripped_so.as_os_str(),
                    program_so.as_os_str(),
                ],
                config.generate_child_script_on_failure,
            );
            #[cfg(not(windows))]
            let output = spawn(
                &config.bpf_sdk.join("scripts").join("strip.sh"),
                &[&program_unstripped_so, &program_so],
                config.generate_child_script_on_failure,
            );
            if config.verbose {
                println!("{}", output);
            }
        }

        if config.dump && file_older_or_missing(&program_unstripped_so, &program_dump) {
            let dump_script = config.bpf_sdk.join("scripts").join("dump.sh");
            #[cfg(windows)]
            {
                eprintln!("Using Bash scripts from within a program is not supported on Windows, skipping `--dump`.");
                eprintln!(
                    "Please run \"{} {} {}\" from a Bash-supporting shell, then re-run this command to see the processed program dump.",
                    &dump_script.display(),
                    &program_unstripped_so.display(),
                    &program_dump.display());
            }
            #[cfg(not(windows))]
            {
                let output = spawn(
                    &dump_script,
                    &[&program_unstripped_so, &program_dump],
                    config.generate_child_script_on_failure,
                );
                if config.verbose {
                    println!("{}", output);
                }
            }
            postprocess_dump(&program_dump);
        }

        check_undefined_symbols(config, &program_so);

        println!();
        println!("To deploy this program:");
        println!("  $ solana program deploy {}", program_so.display());
        println!("The program address will default to this keypair (override with --program-id):");
        println!("  {}", program_keypair.display());
    } else if config.dump {
        println!("Note: --dump is only available for crates with a cdylib target");
    }
}

fn build_bpf(config: Config, manifest_path: Option<PathBuf>) {
    let mut metadata_command = cargo_metadata::MetadataCommand::new();
    if let Some(manifest_path) = manifest_path {
        metadata_command.manifest_path(manifest_path);
    }
    if config.offline {
        metadata_command.other_options(vec!["--offline".to_string()]);
    }

    let metadata = metadata_command.exec().unwrap_or_else(|err| {
        eprintln!("Failed to obtain package metadata: {}", err);
        exit(1);
    });

    if let Some(root_package) = metadata.root_package() {
        if !config.workspace {
            build_bpf_package(&config, metadata.target_directory.as_ref(), root_package);
            return;
        }
    }

    let all_bpf_packages = metadata
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

    for package in all_bpf_packages {
        build_bpf_package(&config, metadata.target_directory.as_ref(), package);
    }
}

fn main() {
    let default_config = Config::default();
    let default_bpf_sdk = format!("{}", default_config.bpf_sdk.display());

    let mut args = env::args().collect::<Vec<_>>();
    // When run as a cargo subcommand, the first program argument is the subcommand name.
    // Remove it
    if let Some(arg1) = args.get(1) {
        if arg1 == "build-bpf" {
            args.remove(1);
        }
    }

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .arg(
            Arg::with_name("bpf_out_dir")
                .env("BPF_OUT_PATH")
                .long("bpf-out-dir")
                .value_name("DIRECTORY")
                .takes_value(true)
                .help("Place final BPF build artifacts in this directory"),
        )
        .arg(
            Arg::with_name("bpf_sdk")
                .env("BPF_SDK_PATH")
                .long("bpf-sdk")
                .value_name("PATH")
                .takes_value(true)
                .default_value(&default_bpf_sdk)
                .help("Path to the Solana BPF SDK"),
        )
        .arg(
            Arg::with_name("cargo_args")
                .help("Arguments passed directly to `cargo build`")
                .multiple(true)
                .last(true),
        )
        .arg(
            Arg::with_name("dump")
                .long("dump")
                .takes_value(false)
                .help("Dump ELF information to a text file on success"),
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
            Arg::with_name("generate_child_script_on_failure")
                .long("generate-child-script-on-failure")
                .takes_value(false)
                .help("Generate a shell script to rerun a failed subcommand"),
        )
        .arg(
            Arg::with_name("manifest_path")
                .long("manifest-path")
                .value_name("PATH")
                .takes_value(true)
                .help("Path to Cargo.toml"),
        )
        .arg(
            Arg::with_name("no_default_features")
                .long("no-default-features")
                .takes_value(false)
                .help("Do not activate the `default` feature"),
        )
        .arg(
            Arg::with_name("offline")
                .long("offline")
                .takes_value(false)
                .help("Run without accessing the network"),
        )
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .takes_value(false)
                .help("Use verbose output"),
        )
        .arg(
            Arg::with_name("workspace")
                .long("workspace")
                .takes_value(false)
                .alias("all")
                .help("Build all BPF packages in the workspace"),
        )
        .get_matches_from(args);

    let bpf_sdk = value_t_or_exit!(matches, "bpf_sdk", PathBuf);
    let bpf_out_dir = value_t!(matches, "bpf_out_dir", PathBuf).ok();

    let config = Config {
        cargo_args: matches
            .values_of("cargo_args")
            .map(|vals| vals.collect::<Vec<_>>()),
        bpf_sdk: fs::canonicalize(&bpf_sdk).unwrap_or_else(|err| {
            eprintln!(
                "BPF SDK path does not exist: {}: {}",
                bpf_sdk.display(),
                err
            );
            exit(1);
        }),
        bpf_out_dir: bpf_out_dir.map(|bpf_out_dir| {
            if bpf_out_dir.is_absolute() {
                bpf_out_dir
            } else {
                env::current_dir()
                    .expect("Unable to get current working directory")
                    .join(bpf_out_dir)
            }
        }),
        dump: matches.is_present("dump"),
        features: values_t!(matches, "features", String)
            .ok()
            .unwrap_or_else(Vec::new),
        generate_child_script_on_failure: matches.is_present("generate_child_script_on_failure"),
        no_default_features: matches.is_present("no_default_features"),
        offline: matches.is_present("offline"),
        verbose: matches.is_present("verbose"),
        workspace: matches.is_present("workspace"),
    };
    let manifest_path = value_t!(matches, "manifest_path", PathBuf).ok();
    build_bpf(config, manifest_path);
}
