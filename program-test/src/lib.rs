//! The solana-program-test provides a BanksClient-based test framework BPF programs

use chrono_humanize::{Accuracy, HumanTime, Tense};
use log::*;
use solana_banks_client::start_client;
use solana_banks_server::banks_server::start_local_server;
use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, hash::Hash, instruction::Instruction,
    instruction::InstructionError, message::Message, native_token::sol_to_lamports,
    program_error::ProgramError, program_stubs, pubkey::Pubkey, rent::Rent,
};
use solana_runtime::{
    bank::{Bank, Builtin},
    bank_forks::BankForks,
    genesis_utils::create_genesis_config_with_leader,
};
use solana_sdk::{
    account::Account,
    keyed_account::KeyedAccount,
    process_instruction::BpfComputeBudget,
    process_instruction::{InvokeContext, MockInvokeContext, ProcessInstructionWithContext},
    signature::{Keypair, Signer},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    convert::TryFrom,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, RwLock},
};

// Export types so test clients can limit their solana crate dependencies
pub use solana_banks_client::{BanksClient, BanksClientExt};

#[macro_use]
extern crate solana_bpf_loader_program;

pub fn to_instruction_error(error: ProgramError) -> InstructionError {
    match error {
        ProgramError::Custom(err) => InstructionError::Custom(err),
        ProgramError::InvalidArgument => InstructionError::InvalidArgument,
        ProgramError::InvalidInstructionData => InstructionError::InvalidInstructionData,
        ProgramError::InvalidAccountData => InstructionError::InvalidAccountData,
        ProgramError::AccountDataTooSmall => InstructionError::AccountDataTooSmall,
        ProgramError::InsufficientFunds => InstructionError::InsufficientFunds,
        ProgramError::IncorrectProgramId => InstructionError::IncorrectProgramId,
        ProgramError::MissingRequiredSignature => InstructionError::MissingRequiredSignature,
        ProgramError::AccountAlreadyInitialized => InstructionError::AccountAlreadyInitialized,
        ProgramError::UninitializedAccount => InstructionError::UninitializedAccount,
        ProgramError::NotEnoughAccountKeys => InstructionError::NotEnoughAccountKeys,
        ProgramError::AccountBorrowFailed => InstructionError::AccountBorrowFailed,
        ProgramError::MaxSeedLengthExceeded => InstructionError::MaxSeedLengthExceeded,
        ProgramError::InvalidSeeds => InstructionError::InvalidSeeds,
    }
}

thread_local! {
    static INVOKE_CONTEXT:RefCell<Rc<MockInvokeContext>> = RefCell::new(Rc::new(MockInvokeContext::default()));
}

pub fn builtin_process_instruction(
    process_instruction: solana_program::entrypoint::ProcessInstruction,
    program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    input: &[u8],
    invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    let mut mock_invoke_context = MockInvokeContext::default();
    mock_invoke_context.programs = invoke_context.get_programs().to_vec();
    mock_invoke_context.key = *program_id;
    // TODO: Populate MockInvokeContext more, or rework to avoid MockInvokeContext entirely.
    //       The context being passed into the program is incomplete...
    let local_invoke_context = RefCell::new(Rc::new(mock_invoke_context));
    swap_invoke_context(&local_invoke_context);

    // Copy all the accounts into a HashMap to ensure there are no duplicates
    let mut accounts: HashMap<Pubkey, Account> = keyed_accounts
        .iter()
        .map(|ka| (*ka.unsigned_key(), ka.account.borrow().clone()))
        .collect();

    // Create shared references to each account's lamports/data/owner
    let account_refs: HashMap<_, _> = accounts
        .iter_mut()
        .map(|(key, account)| {
            (
                *key,
                (
                    Rc::new(RefCell::new(&mut account.lamports)),
                    Rc::new(RefCell::new(&mut account.data[..])),
                    &account.owner,
                ),
            )
        })
        .collect();

    // Create AccountInfos
    let account_infos: Vec<AccountInfo> = keyed_accounts
        .iter()
        .map(|keyed_account| {
            let key = keyed_account.unsigned_key();
            let (lamports, data, owner) = &account_refs[key];
            AccountInfo {
                key,
                is_signer: keyed_account.signer_key().is_some(),
                is_writable: keyed_account.is_writable(),
                lamports: lamports.clone(),
                data: data.clone(),
                owner,
                executable: keyed_account.executable().unwrap(),
                rent_epoch: keyed_account.rent_epoch().unwrap(),
            }
        })
        .collect();

    // Execute the BPF entrypoint
    let result =
        process_instruction(program_id, &account_infos, input).map_err(to_instruction_error);

    if result.is_ok() {
        // Commit changes to the KeyedAccounts
        for keyed_account in keyed_accounts {
            let mut account = keyed_account.account.borrow_mut();
            let key = keyed_account.unsigned_key();
            let (lamports, data, _owner) = &account_refs[key];
            account.lamports = **lamports.borrow();
            account.data = data.borrow().to_vec();
        }
    }

    swap_invoke_context(&local_invoke_context);

    // Propagate logs back to caller's invoke context
    // (TODO: This goes away if MockInvokeContext usage can be removed)
    let logger = invoke_context.get_logger();
    let logger = logger.borrow_mut();
    for message in local_invoke_context.borrow().logger.log.borrow_mut().iter() {
        if logger.log_enabled() {
            logger.log(message);
        }
    }

    result
}

/// Converts a `solana-program`-style entrypoint into the runtime's entrypoint style, for
/// use with `ProgramTest::add_program`
#[macro_export]
macro_rules! processor {
    ($process_instruction:expr) => {
        Some(
            |program_id: &Pubkey,
             keyed_accounts: &[solana_sdk::keyed_account::KeyedAccount],
             input: &[u8],
             invoke_context: &mut dyn solana_sdk::process_instruction::InvokeContext| {
                $crate::builtin_process_instruction(
                    $process_instruction,
                    program_id,
                    keyed_accounts,
                    input,
                    invoke_context,
                )
            },
        )
    };
}

pub fn swap_invoke_context(other_invoke_context: &RefCell<Rc<MockInvokeContext>>) {
    INVOKE_CONTEXT.with(|invoke_context| {
        invoke_context.swap(&other_invoke_context);
    });
}

struct SyscallStubs {}
impl program_stubs::SyscallStubs for SyscallStubs {
    fn sol_log(&self, message: &str) {
        INVOKE_CONTEXT.with(|invoke_context| {
            let invoke_context = invoke_context.borrow_mut();
            let logger = invoke_context.get_logger();
            let logger = logger.borrow_mut();

            if logger.log_enabled() {
                logger.log(&format!("Program log: {}", message));
            }
        });
    }

    fn sol_invoke_signed(
        &self,
        instruction: &Instruction,
        account_infos: &[AccountInfo],
        signers_seeds: &[&[&[u8]]],
    ) -> ProgramResult {
        //
        // TODO: Merge the business logic between here and the BPF invoke path in
        //       programs/bpf_loader/src/syscalls.rs
        //
        info!("SyscallStubs::sol_invoke_signed()");

        let mut caller = Pubkey::default();
        let mut mock_invoke_context = MockInvokeContext::default();

        INVOKE_CONTEXT.with(|invoke_context| {
            let invoke_context = invoke_context.borrow_mut();
            caller = *invoke_context.get_caller().expect("get_caller");
            invoke_context.record_instruction(&instruction);

            mock_invoke_context.programs = invoke_context.get_programs().to_vec();
            // TODO: Populate MockInvokeContext more, or rework to avoid MockInvokeContext entirely.
            //       The context being passed into the program is incomplete...
        });

        if instruction.accounts.len() + 1 != account_infos.len() {
            panic!(
                "Instruction accounts mismatch.  Instruction contains {} accounts, with {}
                       AccountInfos provided",
                instruction.accounts.len(),
                account_infos.len()
            );
        }
        let message = Message::new(&[instruction.clone()], None);

        let program_id_index = message.instructions[0].program_id_index as usize;
        let program_id = message.account_keys[program_id_index];

        let program_account_info = &account_infos[program_id_index];
        if !program_account_info.executable {
            panic!("Program account is not executable");
        }
        if program_account_info.is_writable {
            panic!("Program account is writable");
        }

        fn ai_to_a(ai: &AccountInfo) -> Account {
            Account {
                lamports: ai.lamports(),
                data: ai.try_borrow_data().unwrap().to_vec(),
                owner: *ai.owner,
                executable: ai.executable,
                rent_epoch: ai.rent_epoch,
            }
        }
        let executable_accounts = vec![(program_id, RefCell::new(ai_to_a(program_account_info)))];

        let mut accounts = vec![];
        for instruction_account in &instruction.accounts {
            for account_info in account_infos {
                if *account_info.unsigned_key() == instruction_account.pubkey {
                    if instruction_account.is_writable && !account_info.is_writable {
                        panic!("Writeable mismatch for {}", instruction_account.pubkey);
                    }
                    if instruction_account.is_signer && !account_info.is_signer {
                        let mut program_signer = false;
                        for seeds in signers_seeds.iter() {
                            let signer = Pubkey::create_program_address(&seeds, &caller).unwrap();
                            if instruction_account.pubkey == signer {
                                program_signer = true;
                                break;
                            }
                        }
                        if !program_signer {
                            panic!("Signer mismatch for {}", instruction_account.pubkey);
                        }
                    }
                    accounts.push(Rc::new(RefCell::new(ai_to_a(account_info))));
                    break;
                }
            }
        }
        assert_eq!(accounts.len(), instruction.accounts.len());

        solana_runtime::message_processor::MessageProcessor::process_cross_program_instruction(
            &message,
            &executable_accounts,
            &accounts,
            &mut mock_invoke_context,
        )
        .map_err(|err| ProgramError::try_from(err).unwrap_or_else(|err| panic!("{}", err)))?;

        // Propagate logs back to caller's invoke context
        // (TODO: This goes away if MockInvokeContext usage can be removed)
        INVOKE_CONTEXT.with(|invoke_context| {
            let logger = invoke_context.borrow().get_logger();
            let logger = logger.borrow_mut();
            for message in mock_invoke_context.logger.log.borrow_mut().iter() {
                if logger.log_enabled() {
                    logger.log(message);
                }
            }
        });

        // Copy writeable account modifications back into the caller's AccountInfos
        for (i, instruction_account) in instruction.accounts.iter().enumerate() {
            if !instruction_account.is_writable {
                continue;
            }

            for account_info in account_infos {
                if *account_info.unsigned_key() == instruction_account.pubkey {
                    let account = &accounts[i];
                    **account_info.try_borrow_mut_lamports().unwrap() = account.borrow().lamports;

                    let mut data = account_info.try_borrow_mut_data()?;
                    let new_data = &account.borrow().data;
                    if data.len() != new_data.len() {
                        // TODO: Figure out how to change the callers account data size
                        panic!(
                            "Account resizing ({} -> {}) not supported yet",
                            data.len(),
                            new_data.len()
                        );
                    }
                    data.clone_from_slice(new_data);
                }
            }
        }

        Ok(())
    }
}

fn find_file(filename: &str, search_path: &[PathBuf]) -> Option<PathBuf> {
    for path in search_path {
        let candidate = path.join(&filename);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn read_file<P: AsRef<Path>>(path: P) -> Vec<u8> {
    let path = path.as_ref();
    let mut file = File::open(path)
        .unwrap_or_else(|err| panic!("Failed to open \"{}\": {}", path.display(), err));

    let mut file_data = Vec::new();
    file.read_to_end(&mut file_data)
        .unwrap_or_else(|err| panic!("Failed to read \"{}\": {}", path.display(), err));
    file_data
}

pub struct ProgramTest {
    accounts: Vec<(Pubkey, Account)>,
    builtins: Vec<Builtin>,
    bpf_compute_max_units: Option<u64>,
    prefer_bpf: bool,
    search_path: Vec<PathBuf>,
}

impl Default for ProgramTest {
    /// Initialize a new ProgramTest
    ///
    /// If the `BPF_OUT_DIR` environment variable is defined, BPF programs will be preferred over
    /// over a native instruction processor.  The `ProgramTest::prefer_bpf()` method may be
    /// used to override this preference at runtime.  `cargo test-bpf` will set `BPF_OUT_DIR`
    /// automatically.
    ///
    /// BPF program shared objects and account data files are searched for in
    /// * the value of the `BPF_OUT_DIR` environment variable
    /// * the `tests/fixtures` sub-directory
    /// * the current working directory
    ///
    fn default() -> Self {
        solana_logger::setup_with_default(
            "solana_bpf_loader=debug,\
             solana_rbpf::vm=debug,\
             solana_runtime::message_processor=info,\
             solana_runtime::system_instruction_processor=trace,\
             solana_program_test=info",
        );
        let mut prefer_bpf = false;

        let mut search_path = vec![];
        if let Ok(bpf_out_dir) = std::env::var("BPF_OUT_DIR") {
            prefer_bpf = true;
            search_path.push(PathBuf::from(bpf_out_dir));
        }
        search_path.push(PathBuf::from("tests/fixtures"));
        if let Ok(dir) = std::env::current_dir() {
            search_path.push(dir);
        }
        debug!("search path: {:?}", search_path);

        Self {
            accounts: vec![],
            builtins: vec![],
            bpf_compute_max_units: None,
            prefer_bpf,
            search_path,
        }
    }
}

impl ProgramTest {
    pub fn new(
        program_name: &str,
        program_id: Pubkey,
        process_instruction: Option<ProcessInstructionWithContext>,
    ) -> Self {
        let mut me = Self::default();
        me.add_program(program_name, program_id, process_instruction);
        me
    }

    /// Override default BPF program selection
    pub fn prefer_bpf(&mut self, prefer_bpf: bool) {
        self.prefer_bpf = prefer_bpf;
    }

    /// Override the BPF compute budget
    pub fn set_bpf_compute_max_units(&mut self, bpf_compute_max_units: u64) {
        self.bpf_compute_max_units = Some(bpf_compute_max_units);
    }

    /// Add an account to the test environment
    pub fn add_account(&mut self, address: Pubkey, account: Account) {
        self.accounts.push((address, account));
    }

    /// Add an account to the test environment with the account data in the provided `filename`
    pub fn add_account_with_file_data(
        &mut self,
        address: Pubkey,
        lamports: u64,
        owner: Pubkey,
        filename: &str,
    ) {
        self.add_account(
            address,
            Account {
                lamports,
                data: read_file(find_file(filename, &self.search_path).unwrap_or_else(|| {
                    panic!("Unable to locate {}", filename);
                })),
                owner,
                executable: false,
                rent_epoch: 0,
            },
        );
    }

    /// Add an account to the test environment with the account data in the provided as a base 64
    /// string
    pub fn add_account_with_base64_data(
        &mut self,
        address: Pubkey,
        lamports: u64,
        owner: Pubkey,
        data_base64: &str,
    ) {
        self.add_account(
            address,
            Account {
                lamports,
                data: base64::decode(data_base64)
                    .unwrap_or_else(|err| panic!("Failed to base64 decode: {}", err)),
                owner,
                executable: false,
                rent_epoch: 0,
            },
        );
    }

    /// Add a BPF program to the test environment.
    ///
    /// `program_name` will also used to locate the BPF shared object in the current or fixtures
    /// directory.
    ///
    /// If `process_instruction` is provided, the natively built-program may be used instead of the
    /// BPF shared object depending on the `bpf` environment variable.
    pub fn add_program(
        &mut self,
        program_name: &str,
        program_id: Pubkey,
        process_instruction: Option<ProcessInstructionWithContext>,
    ) {
        let loader = solana_program::bpf_loader::id();
        let program_file = find_file(&format!("{}.so", program_name), &self.search_path);

        if process_instruction.is_none() && program_file.is_none() {
            panic!("Unable to add program {} ({})", program_name, program_id);
        }

        if (program_file.is_some() && self.prefer_bpf) || process_instruction.is_none() {
            let program_file = program_file.unwrap_or_else(|| {
                panic!(
                    "Program file data not available for {} ({})",
                    program_name, program_id
                );
            });
            let data = read_file(&program_file);
            info!(
                "\"{}\" BPF program from {}{}",
                program_name,
                program_file.display(),
                std::fs::metadata(&program_file)
                    .map(|metadata| {
                        metadata
                            .modified()
                            .map(|time| {
                                format!(
                                    ", modified {}",
                                    HumanTime::from(time)
                                        .to_text_en(Accuracy::Precise, Tense::Past)
                                )
                            })
                            .ok()
                    })
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "".to_string())
            );

            self.add_account(
                program_id,
                Account {
                    lamports: Rent::default().minimum_balance(data.len()).min(1),
                    data,
                    owner: loader,
                    executable: true,
                    rent_epoch: 0,
                },
            );
        } else {
            info!("\"{}\" program loaded as native code", program_name);
            self.builtins.push(Builtin::new(
                program_name,
                program_id,
                process_instruction.unwrap_or_else(|| {
                    panic!(
                        "Program processor not available for {} ({})",
                        program_name, program_id
                    );
                }),
            ));
        }
    }

    /// Start the test client
    ///
    /// Returns a `BanksClient` interface into the test environment as well as a payer `Keypair`
    /// with SOL for sending transactions
    pub async fn start(self) -> (BanksClient, Keypair, Hash) {
        {
            use std::sync::Once;
            static ONCE: Once = Once::new();

            ONCE.call_once(|| {
                program_stubs::set_syscall_stubs(Box::new(SyscallStubs {}));
            });
        }

        let bootstrap_validator_pubkey = Pubkey::new_unique();
        let bootstrap_validator_stake_lamports = 42;

        let gci = create_genesis_config_with_leader(
            sol_to_lamports(1_000_000.0),
            &bootstrap_validator_pubkey,
            bootstrap_validator_stake_lamports,
        );
        let mut genesis_config = gci.genesis_config;
        genesis_config.rent = Rent::default();
        genesis_config.fee_rate_governor =
            solana_program::fee_calculator::FeeRateGovernor::default();
        let payer = gci.mint_keypair;
        debug!("Payer address: {}", payer.pubkey());
        debug!("Genesis config: {}", genesis_config);

        let mut bank = Bank::new(&genesis_config);

        for loader in &[
            solana_bpf_loader_deprecated_program!(),
            solana_bpf_loader_program!(),
        ] {
            bank.add_builtin(&loader.0, loader.1, loader.2);
        }

        // User-supplied additional builtins
        for builtin in self.builtins {
            bank.add_builtin(
                &builtin.name,
                builtin.id,
                builtin.process_instruction_with_context,
            );
        }

        for (address, account) in self.accounts {
            if bank.get_account(&address).is_some() {
                panic!("An account at {} already exists", address);
            }
            bank.store_account(&address, &account);
        }
        bank.set_capitalization();
        if let Some(max_units) = self.bpf_compute_max_units {
            bank.set_bpf_compute_budget(Some(BpfComputeBudget {
                max_units,
                ..BpfComputeBudget::default()
            }));
        }

        // Advance beyond slot 0 for a slightly more realistic test environment
        let bank = Arc::new(bank);
        let bank = Bank::new_from_parent(&bank, bank.collector_id(), bank.slot() + 1);
        debug!("Bank slot: {}", bank.slot());

        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));

        let transport = start_local_server(&bank_forks).await;
        let mut banks_client = start_client(transport)
            .await
            .unwrap_or_else(|err| panic!("Failed to start banks client: {}", err));

        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        (banks_client, payer, recent_blockhash)
    }
}
