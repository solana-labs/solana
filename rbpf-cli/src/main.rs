use {
    clap::{crate_version, Arg, Command},
    serde::{Deserialize, Serialize},
    serde_json::Result,
    solana_bpf_loader_program::{
        create_vm, serialization::serialize_parameters, syscalls::register_syscalls,
        ThisInstructionMeter,
    },
    solana_program_runtime::invoke_context::{prepare_mock_invoke_context, InvokeContext},
    solana_rbpf::{
        assembler::assemble,
        elf::Executable,
        static_analysis::Analysis,
        verifier::RequisiteVerifier,
        vm::{Config, DynamicAnalysis, VerifiedExecutable},
    },
    solana_sdk::{
        account::AccountSharedData, bpf_loader, instruction::AccountMeta, pubkey::Pubkey,
        sysvar::rent::Rent, transaction_context::TransactionContext,
    },
    std::{
        fmt::{Debug, Formatter},
        fs::File,
        io::{Read, Seek, SeekFrom},
        path::Path,
        time::{Duration, Instant},
    },
};

#[derive(Serialize, Deserialize, Debug)]
struct Account {
    key: Pubkey,
    owner: Pubkey,
    is_signer: bool,
    is_writable: bool,
    lamports: u64,
    data: Vec<u8>,
}
#[derive(Serialize, Deserialize)]
struct Input {
    accounts: Vec<Account>,
    instruction_data: Vec<u8>,
}
fn load_accounts(path: &Path) -> Result<Input> {
    let file = File::open(path).unwrap();
    let input: Input = serde_json::from_reader(file)?;
    eprintln!("Program input:");
    eprintln!("accounts {:?}", &input.accounts);
    eprintln!("instruction_data {:?}", &input.instruction_data);
    eprintln!("----------------------------------------");
    Ok(input)
}

fn main() {
    solana_logger::setup();
    let matches = Command::new("Solana SBF CLI")
        .version(crate_version!())
        .author("Solana Maintainers <maintainers@solana.foundation>")
        .about(
            r##"CLI to test and analyze SBF programs.

The tool executes SBF programs in a mocked environment.
Some features, such as sysvars syscall and CPI, are not
available for the programs executed by the CLI tool.

The input data for a program execution have to be in JSON format
and the following fields are required
{
    "accounts": [
        {
            "key": [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
            ],
            "owner": [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
            ],
            "is_signer": false,
            "is_writable": true,
            "lamports": 1000,
            "data": [0, 0, 0, 3]
        }
    ],
    "instruction_data": []
}
"##,
        )
        .arg(
            Arg::new("PROGRAM")
                .help(
                    "Program file to use. This is either an ELF shared-object file to be executed, \
                     or an assembly file to be assembled and executed.",
                )
                .required(true)
                .index(1)
        )
        .arg(
            Arg::new("input")
                .help(
                    "Input for the program to run on, where FILE is a name of a JSON file \
with input data, or BYTES is the number of 0-valued bytes to allocate for program parameters",
                )
                .short('i')
                .long("input")
                .value_name("FILE / BYTES")
                .takes_value(true)
                .default_value("0"),
        )
        .arg(
            Arg::new("memory")
                .help("Heap memory for the program to run on")
                .short('m')
                .long("memory")
                .value_name("BYTES")
                .takes_value(true)
                .default_value("0"),
        )
        .arg(
            Arg::new("use")
                .help(
                    "Method of execution to use, where 'cfg' generates Control Flow Graph \
of the program, 'disassembler' dumps disassembled code of the program, 'interpreter' runs \
the program in the virtual machine's interpreter, and 'jit' precompiles the program to \
native machine code before execting it in the virtual machine.",
                )
                .short('u')
                .long("use")
                .takes_value(true)
                .value_name("VALUE")
                .possible_values(["cfg", "disassembler", "interpreter", "jit"])
                .default_value("jit"),
        )
        .arg(
            Arg::new("instruction limit")
                .help("Limit the number of instructions to execute")
                .short('l')
                .long("limit")
                .takes_value(true)
                .value_name("COUNT")
                .default_value(&std::i64::MAX.to_string()),
        )
        .arg(
            Arg::new("trace")
                .help("Output trace to 'trace.out' file using tracing instrumentation")
                .short('t')
                .long("trace"),
        )
        .arg(
            Arg::new("profile")
                .help("Output profile to 'profile.dot' file using tracing instrumentation")
                .short('p')
                .long("profile"),
        )
        .arg(
            Arg::new("output_format")
                .help("Return information in specified output format")
                .long("output")
                .value_name("FORMAT")
                .global(true)
                .takes_value(true)
                .possible_values(["json", "json-compact"]),
        )
        .get_matches();

    let config = Config {
        enable_instruction_tracing: matches.is_present("trace") || matches.is_present("profile"),
        enable_symbol_and_section_labels: true,
        ..Config::default()
    };
    let loader_id = bpf_loader::id();
    let mut transaction_accounts = vec![
        (
            loader_id,
            AccountSharedData::new(0, 0, &solana_sdk::native_loader::id()),
        ),
        (
            Pubkey::new_unique(),
            AccountSharedData::new(0, 0, &loader_id),
        ),
    ];
    let mut instruction_accounts = Vec::new();
    let instruction_data = match matches.value_of("input").unwrap().parse::<usize>() {
        Ok(allocation_size) => {
            let pubkey = Pubkey::new_unique();
            transaction_accounts.push((
                pubkey,
                AccountSharedData::new(0, allocation_size, &Pubkey::new_unique()),
            ));
            instruction_accounts.push(AccountMeta {
                pubkey,
                is_signer: false,
                is_writable: true,
            });
            vec![]
        }
        Err(_) => {
            let input = load_accounts(Path::new(matches.value_of("input").unwrap())).unwrap();
            for account_info in input.accounts {
                let mut account = AccountSharedData::new(
                    account_info.lamports,
                    account_info.data.len(),
                    &account_info.owner,
                );
                account.set_data(account_info.data);
                instruction_accounts.push(AccountMeta {
                    pubkey: account_info.key,
                    is_signer: account_info.is_signer,
                    is_writable: account_info.is_writable,
                });
                transaction_accounts.push((account_info.key, account));
            }
            input.instruction_data
        }
    };
    let program_indices = [0, 1];
    let preparation =
        prepare_mock_invoke_context(transaction_accounts, instruction_accounts, &program_indices);
    let mut transaction_context = TransactionContext::new(
        preparation.transaction_accounts,
        Some(Rent::default()),
        1,
        1,
    );
    let mut invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
    invoke_context
        .transaction_context
        .get_next_instruction_context()
        .unwrap()
        .configure(
            &program_indices,
            &preparation.instruction_accounts,
            &instruction_data,
        );
    invoke_context.push().unwrap();
    let (_parameter_bytes, regions, account_lengths) = serialize_parameters(
        invoke_context.transaction_context,
        invoke_context
            .transaction_context
            .get_current_instruction_context()
            .unwrap(),
        true, // should_cap_ix_accounts
    )
    .unwrap();
    let compute_meter = invoke_context.get_compute_meter();
    let mut instruction_meter = ThisInstructionMeter { compute_meter };

    let program = matches.value_of("PROGRAM").unwrap();
    let mut file = File::open(Path::new(program)).unwrap();
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).unwrap();
    let syscall_registry = register_syscalls(&invoke_context.feature_set, true).unwrap();
    let executable = if magic == [0x7f, 0x45, 0x4c, 0x46] {
        Executable::<ThisInstructionMeter>::from_elf(&contents, config, syscall_registry)
            .map_err(|err| format!("Executable constructor failed: {:?}", err))
    } else {
        assemble::<ThisInstructionMeter>(
            std::str::from_utf8(contents.as_slice()).unwrap(),
            config,
            syscall_registry,
        )
    }
    .unwrap();

    let mut verified_executable =
        VerifiedExecutable::<RequisiteVerifier, ThisInstructionMeter>::from_executable(executable)
            .map_err(|err| format!("Executable verifier failed: {:?}", err))
            .unwrap();

    verified_executable.jit_compile().unwrap();
    let mut analysis = LazyAnalysis::new(verified_executable.get_executable());

    match matches.value_of("use") {
        Some("cfg") => {
            let mut file = File::create("cfg.dot").unwrap();
            analysis
                .analyze()
                .visualize_graphically(&mut file, None)
                .unwrap();
            return;
        }
        Some("disassembler") => {
            let stdout = std::io::stdout();
            analysis.analyze().disassemble(&mut stdout.lock()).unwrap();
            return;
        }
        _ => {}
    }

    let mut vm = create_vm(
        &verified_executable,
        regions,
        account_lengths,
        &mut invoke_context,
    )
    .unwrap();
    let start_time = Instant::now();
    let result = if matches.value_of("use").unwrap() == "interpreter" {
        vm.execute_program_interpreted(&mut instruction_meter)
    } else {
        vm.execute_program_jit(&mut instruction_meter)
    };
    let duration = Instant::now() - start_time;

    if matches.is_present("trace") {
        eprintln!("Trace is saved in trace.out");
        let mut file = File::create("trace.out").unwrap();
        vm.get_program_environment()
            .tracer
            .write(&mut file, analysis.analyze())
            .unwrap();
    }
    if matches.is_present("profile") {
        eprintln!("Profile is saved in profile.dot");
        let tracer = &vm.get_program_environment().tracer;
        let analysis = analysis.analyze();
        let dynamic_analysis = DynamicAnalysis::new(tracer, analysis);
        let mut file = File::create("profile.dot").unwrap();
        analysis
            .visualize_graphically(&mut file, Some(&dynamic_analysis))
            .unwrap();
    }

    let instruction_count = vm.get_total_instruction_count();
    drop(vm);

    let output = Output {
        result: format!("{:?}", result),
        instruction_count,
        execution_time: duration,
        log: invoke_context
            .get_log_collector()
            .unwrap()
            .borrow()
            .get_recorded_content()
            .to_vec(),
    };
    match matches.value_of("output_format") {
        Some("json") => {
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        Some("json-compact") => {
            println!("{}", serde_json::to_string(&output).unwrap());
        }
        _ => {
            println!("Program output:");
            println!("{:?}", output);
        }
    }
}

#[derive(Serialize)]
struct Output {
    result: String,
    instruction_count: u64,
    execution_time: Duration,
    log: Vec<String>,
}

impl Debug for Output {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Result: {}", self.result)?;
        writeln!(f, "Instruction Count: {}", self.instruction_count)?;
        writeln!(f, "Execution time: {} us", self.execution_time.as_micros())?;
        for line in &self.log {
            writeln!(f, "{}", line)?;
        }
        Ok(())
    }
}

// Replace with std::lazy::Lazy when stabilized.
// https://github.com/rust-lang/rust/issues/74465
struct LazyAnalysis<'a> {
    analysis: Option<Analysis<'a, ThisInstructionMeter>>,
    executable: &'a Executable<ThisInstructionMeter>,
}

impl<'a> LazyAnalysis<'a> {
    fn new(executable: &'a Executable<ThisInstructionMeter>) -> Self {
        Self {
            analysis: None,
            executable,
        }
    }

    fn analyze(&mut self) -> &Analysis<ThisInstructionMeter> {
        if let Some(ref analysis) = self.analysis {
            return analysis;
        }
        self.analysis
            .insert(Analysis::from_executable(self.executable).unwrap())
    }
}
