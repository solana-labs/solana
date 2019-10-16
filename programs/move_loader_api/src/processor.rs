use crate::account_state::{pubkey_to_address, LibraAccountState};
use crate::data_store::DataStore;
use crate::error_mappers::*;
use crate::id;
use bytecode_verifier::{VerifiedModule, VerifiedScript};
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{
    account::KeyedAccount, instruction::InstructionError, loader_instruction::LoaderInstruction,
    pubkey::Pubkey, sysvar::rent,
};
use types::{
    account_address::AccountAddress,
    transaction::{Program, TransactionArgument, TransactionOutput},
};
use vm::{
    access::ModuleAccess,
    file_format::{CompiledModule, CompiledScript},
    gas_schedule::{MAXIMUM_NUMBER_OF_GAS_UNITS, MAX_PRICE_PER_GAS_UNIT},
    transaction_metadata::TransactionMetadata,
};
use vm_cache_map::Arena;
use vm_runtime::{
    code_cache::{
        module_adapter::ModuleFetcherImpl,
        module_cache::{BlockModuleCache, ModuleCache, VMModuleCache},
    },
    txn_executor::TransactionExecutor,
    value::Local,
};

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    solana_logger::setup();

    let command = bincode::deserialize::<LoaderInstruction>(data).map_err(|err| {
        info!("Invalid instruction: {:?} {:?}", data, err);
        InstructionError::InvalidInstructionData
    })?;

    trace!("{:?}", command);

    match command {
        LoaderInstruction::Write { offset, bytes } => {
            MoveProcessor::do_write(keyed_accounts, offset, &bytes)
        }
        LoaderInstruction::Finalize => MoveProcessor::do_finalize(keyed_accounts),
        LoaderInstruction::InvokeMain { data } => {
            MoveProcessor::do_invoke_main(keyed_accounts, &data)
        }
    }
}

pub const PROGRAM_INDEX: usize = 0;
pub const GENESIS_INDEX: usize = 1;

/// Command to invoke
#[derive(Debug, Serialize, Deserialize)]
pub enum InvokeCommand {
    /// Create a new genesis account
    CreateGenesis(u64),
    /// Run a Move program
    RunProgram {
        /// Sender of the "transaction", the "sender" who is running this program
        sender_address: AccountAddress,
        /// Name of the program's function to call
        function_name: String,
        /// Arguments to pass to the program being called
        args: Vec<TransactionArgument>,
    },
}

pub struct MoveProcessor {}

impl MoveProcessor {
    #[allow(clippy::needless_pass_by_value)]
    fn missing_account() -> InstructionError {
        debug!("Error: Missing account");
        InstructionError::InvalidAccountData
    }

    fn arguments_to_locals(args: Vec<TransactionArgument>) -> Vec<Local> {
        let mut locals = vec![];
        for arg in args {
            locals.push(match arg {
                TransactionArgument::U64(i) => Local::u64(i),
                TransactionArgument::Address(a) => Local::address(a),
                TransactionArgument::ByteArray(b) => Local::bytearray(b),
                TransactionArgument::String(s) => Local::string(s),
            });
        }
        locals
    }

    fn serialize_and_enforce_length(
        state: &LibraAccountState,
        data: &mut Vec<u8>,
    ) -> Result<(), InstructionError> {
        let original_len = data.len() as u64;
        let mut writer = std::io::Cursor::new(data);
        bincode::config()
            .limit(original_len)
            .serialize_into(&mut writer, &state)
            .map_err(map_data_error)?;
        if writer.position() < original_len {
            writer.set_position(original_len);
        }
        Ok(())
    }

    fn serialize_verified_program(
        script: &VerifiedScript,
        modules: &[VerifiedModule],
    ) -> Result<(Vec<u8>), InstructionError> {
        let mut script_bytes = vec![];
        script
            .as_inner()
            .serialize(&mut script_bytes)
            .map_err(map_failure_error)?;
        let mut modules_bytes = vec![];
        for module in modules.iter() {
            let mut buf = vec![];
            module
                .as_inner()
                .serialize(&mut buf)
                .map_err(map_failure_error)?;
            modules_bytes.push(buf);
        }
        bincode::serialize(&LibraAccountState::VerifiedProgram {
            script_bytes,
            modules_bytes,
        })
        .map_err(map_data_error)
    }

    fn deserialize_compiled_program(
        data: &[u8],
    ) -> Result<(CompiledScript, Vec<CompiledModule>), InstructionError> {
        match bincode::deserialize(data).map_err(map_data_error)? {
            LibraAccountState::CompiledProgram(string) => {
                let program: Program = serde_json::from_str(&string).map_err(map_json_error)?;

                let script =
                    CompiledScript::deserialize(&program.code()).map_err(map_vm_binary_error)?;
                let modules = program
                    .modules()
                    .iter()
                    .map(|bytes| CompiledModule::deserialize(&bytes))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(map_vm_binary_error)?;

                Ok((script, modules))
            }
            _ => {
                debug!("Error: Program account does not contain a program");
                Err(InstructionError::InvalidArgument)
            }
        }
    }

    fn deserialize_verified_program(
        data: &[u8],
    ) -> Result<(VerifiedScript, Vec<VerifiedModule>), InstructionError> {
        match bincode::deserialize(data).map_err(map_data_error)? {
            LibraAccountState::VerifiedProgram {
                script_bytes,
                modules_bytes,
            } => {
                let script =
                    VerifiedScript::deserialize(&script_bytes).map_err(map_vm_binary_error)?;
                let modules = modules_bytes
                    .iter()
                    .map(|bytes| VerifiedModule::deserialize(&bytes))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(map_vm_binary_error)?;

                Ok((script, modules))
            }
            _ => {
                debug!("Error: Program account does not contain a program");
                Err(InstructionError::InvalidArgument)
            }
        }
    }

    fn execute(
        sender_address: AccountAddress,
        function_name: &str,
        args: Vec<TransactionArgument>,
        script: VerifiedScript,
        modules: Vec<VerifiedModule>,
        data_store: &DataStore,
    ) -> Result<TransactionOutput, InstructionError> {
        let allocator = Arena::new();
        let code_cache = VMModuleCache::new(&allocator);
        let module_cache = BlockModuleCache::new(&code_cache, ModuleFetcherImpl::new(data_store));
        let mut modules_to_publish = vec![];

        let main_module = script.into_module();
        let module_id = main_module.self_id();
        module_cache.cache_module(main_module);
        for verified_module in modules {
            let mut raw_bytes = vec![];
            verified_module
                .as_inner()
                .serialize(&mut raw_bytes)
                .map_err(map_failure_error)?;
            modules_to_publish.push((verified_module.self_id(), raw_bytes));
            module_cache.cache_module(verified_module);
        }

        let mut txn_metadata = TransactionMetadata::default();
        txn_metadata.sender = sender_address;

        // Caps execution to the Libra prescribed 10 milliseconds
        txn_metadata.max_gas_amount = *MAXIMUM_NUMBER_OF_GAS_UNITS;
        txn_metadata.gas_unit_price = *MAX_PRICE_PER_GAS_UNIT;

        let mut vm = TransactionExecutor::new(&module_cache, data_store, txn_metadata);
        vm.execute_function(&module_id, &function_name, Self::arguments_to_locals(args))
            .map_err(map_vm_invariant_violation_error)?
            .map_err(map_vm_runtime_error)?;

        Ok(vm
            .make_write_set(modules_to_publish, Ok(Ok(())))
            .map_err(map_vm_runtime_error)?)
    }

    fn keyed_accounts_to_data_store(
        genesis_key: &Pubkey,
        keyed_accounts: &[KeyedAccount],
    ) -> Result<DataStore, InstructionError> {
        let mut data_store = DataStore::default();
        for keyed_account in keyed_accounts {
            match bincode::deserialize(&keyed_account.account.data).map_err(map_data_error)? {
                LibraAccountState::Genesis(write_set) => data_store.apply_write_set(&write_set),
                LibraAccountState::User(owner, write_set) => {
                    if owner != *genesis_key {
                        debug!("All user accounts must be owned by the genesis");
                        return Err(InstructionError::InvalidArgument);
                    }
                    data_store.apply_write_set(&write_set)
                }
                _ => (), // ignore unallocated accounts
            }
        }
        Ok(data_store)
    }

    fn data_store_to_keyed_accounts(
        data_store: DataStore,
        keyed_accounts: &mut [KeyedAccount],
    ) -> Result<(), InstructionError> {
        let mut write_sets = data_store
            .into_write_sets()
            .map_err(|_| InstructionError::GenericError)?;

        // Genesis account holds both mint and stdlib under address 0x0
        let genesis_key = *keyed_accounts[GENESIS_INDEX].unsigned_key();
        let write_set = write_sets
            .remove(&AccountAddress::default())
            .ok_or_else(Self::missing_account)?;
        Self::serialize_and_enforce_length(
            &LibraAccountState::Genesis(write_set),
            &mut keyed_accounts[GENESIS_INDEX].account.data,
        )?;

        // Now do the rest of the accounts
        for keyed_account in keyed_accounts[GENESIS_INDEX + 1..].iter_mut() {
            let write_set = write_sets
                .remove(&pubkey_to_address(keyed_account.unsigned_key()))
                .ok_or_else(Self::missing_account)?;
            Self::serialize_and_enforce_length(
                &LibraAccountState::User(genesis_key, write_set),
                &mut keyed_account.account.data,
            )?;
        }
        if !write_sets.is_empty() {
            debug!("Error: Missing keyed accounts");
            return Err(InstructionError::GenericError);
        }
        Ok(())
    }

    pub fn do_write(
        keyed_accounts: &mut [KeyedAccount],
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), InstructionError> {
        if keyed_accounts[PROGRAM_INDEX].signer_key().is_none() {
            debug!("Error: key[0] did not sign the transaction");
            return Err(InstructionError::GenericError);
        }
        let offset = offset as usize;
        let len = bytes.len();
        trace!("Write: offset={} length={}", offset, len);
        if keyed_accounts[PROGRAM_INDEX].account.data.len() < offset + len {
            debug!(
                "Error: Write overflow: {} < {}",
                keyed_accounts[PROGRAM_INDEX].account.data.len(),
                offset + len
            );
            return Err(InstructionError::GenericError);
        }
        keyed_accounts[PROGRAM_INDEX].account.data[offset..offset + len].copy_from_slice(&bytes);
        Ok(())
    }

    pub fn do_finalize(keyed_accounts: &mut [KeyedAccount]) -> Result<(), InstructionError> {
        if keyed_accounts.len() < 2 {
            return Err(InstructionError::InvalidInstructionData);
        }
        if keyed_accounts[PROGRAM_INDEX].signer_key().is_none() {
            debug!("Error: key[0] did not sign the transaction");
            return Err(InstructionError::GenericError);
        }

        rent::verify_rent_exemption(&keyed_accounts[0], &keyed_accounts[1])?;

        let (compiled_script, compiled_modules) =
            Self::deserialize_compiled_program(&keyed_accounts[PROGRAM_INDEX].account.data)?;

        let verified_script = VerifiedScript::new(compiled_script).unwrap();
        let verified_modules = compiled_modules
            .into_iter()
            .map(VerifiedModule::new)
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_vm_verification_error)?;

        keyed_accounts[PROGRAM_INDEX].account.data =
            Self::serialize_verified_program(&verified_script, &verified_modules)?;
        keyed_accounts[PROGRAM_INDEX].account.executable = true;

        info!(
            "Finalize: {:?}",
            keyed_accounts[PROGRAM_INDEX]
                .signer_key()
                .unwrap_or(&Pubkey::default())
        );
        Ok(())
    }

    pub fn do_invoke_main(
        keyed_accounts: &mut [KeyedAccount],
        data: &[u8],
    ) -> Result<(), InstructionError> {
        match bincode::deserialize(&data).map_err(map_data_error)? {
            InvokeCommand::CreateGenesis(amount) => {
                if keyed_accounts.is_empty() {
                    debug!("Error: Requires an unallocated account");
                    return Err(InstructionError::InvalidArgument);
                }
                if keyed_accounts[0].account.owner != id() {
                    debug!("Error: Move program account not owned by Move loader");
                    return Err(InstructionError::InvalidArgument);
                }

                match bincode::deserialize(&keyed_accounts[0].account.data)
                    .map_err(map_data_error)?
                {
                    LibraAccountState::Unallocated => Self::serialize_and_enforce_length(
                        &LibraAccountState::create_genesis(amount)?,
                        &mut keyed_accounts[0].account.data,
                    ),
                    _ => {
                        debug!("Error: Must provide an unallocated account");
                        Err(InstructionError::InvalidArgument)
                    }
                }
            }
            InvokeCommand::RunProgram {
                sender_address,
                function_name,
                args,
            } => {
                if keyed_accounts.len() < 2 {
                    debug!("Error: Requires at least a program and a genesis accounts");
                    return Err(InstructionError::InvalidArgument);
                }
                if keyed_accounts[PROGRAM_INDEX].account.owner != id() {
                    debug!("Error: Move program account not owned by Move loader");
                    return Err(InstructionError::InvalidArgument);
                }
                if !keyed_accounts[PROGRAM_INDEX].account.executable {
                    debug!("Error: Move program account not executable");
                    return Err(InstructionError::InvalidArgument);
                }

                let mut data_store = Self::keyed_accounts_to_data_store(
                    keyed_accounts[GENESIS_INDEX].unsigned_key(),
                    &keyed_accounts[GENESIS_INDEX..],
                )?;
                let (verified_script, verified_modules) = Self::deserialize_verified_program(
                    &keyed_accounts[PROGRAM_INDEX].account.data,
                )?;

                let output = Self::execute(
                    sender_address,
                    &function_name,
                    args,
                    verified_script,
                    verified_modules,
                    &data_store,
                )?;
                for event in output.events() {
                    trace!("Event: {:?}", event);
                }

                data_store.apply_write_set(&output.write_set());
                Self::data_store_to_keyed_accounts(data_store, keyed_accounts)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use language_e2e_tests::account::AccountResource;
    use solana_sdk::account::Account;
    use solana_sdk::rent_calculator::RentCalculator;
    use solana_sdk::sysvar::rent;

    const BIG_ENOUGH: usize = 6_000;

    #[test]
    fn test_account_size() {
        let mut data =
            vec![0_u8; bincode::serialized_size(&LibraAccountState::Unallocated).unwrap() as usize];
        let len = data.len();
        assert_eq!(
            MoveProcessor::serialize_and_enforce_length(&LibraAccountState::Unallocated, &mut data),
            Ok(())
        );
        assert_eq!(len, data.len());

        data.resize(6000, 0);
        let len = data.len();
        assert_eq!(
            MoveProcessor::serialize_and_enforce_length(&LibraAccountState::Unallocated, &mut data),
            Ok(())
        );
        assert_eq!(len, data.len());

        data.resize(1, 0);
        assert_eq!(
            MoveProcessor::serialize_and_enforce_length(&LibraAccountState::Unallocated, &mut data),
            Err(InstructionError::AccountDataTooSmall)
        );
    }

    #[test]
    fn test_finalize() {
        solana_logger::setup();

        let code = "main() { return; }";
        let sender_address = AccountAddress::default();
        let mut program = LibraAccount::create_program(&sender_address, code, vec![]);
        let rent_id = rent::id();
        let mut rent_account = rent::create_account(1, &RentCalculator::default());
        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, true, &mut program.account),
            KeyedAccount::new(&rent_id, false, &mut rent_account),
        ];
        MoveProcessor::do_finalize(&mut keyed_accounts).unwrap();
        let (_, _) = MoveProcessor::deserialize_verified_program(&program.account.data).unwrap();
    }

    #[test]
    fn test_create_genesis_account() {
        solana_logger::setup();

        let amount = 10_000_000;
        let mut unallocated = LibraAccount::create_unallocated();

        let mut keyed_accounts = vec![KeyedAccount::new(
            &unallocated.key,
            false,
            &mut unallocated.account,
        )];
        keyed_accounts[0].account.data.resize(BIG_ENOUGH, 0);
        MoveProcessor::do_invoke_main(
            &mut keyed_accounts,
            &bincode::serialize(&InvokeCommand::CreateGenesis(amount)).unwrap(),
        )
        .unwrap();

        assert_eq!(
            bincode::deserialize::<LibraAccountState>(
                &LibraAccount::create_genesis(amount).account.data
            )
            .unwrap(),
            bincode::deserialize::<LibraAccountState>(&keyed_accounts[0].account.data).unwrap()
        );
    }

    #[test]
    fn test_invoke_main() {
        solana_logger::setup();

        let code = "main() { return; }";
        let sender_address = AccountAddress::default();
        let mut program = LibraAccount::create_program(&sender_address, code, vec![]);
        let mut genesis = LibraAccount::create_genesis(1_000_000_000);

        let rent_id = rent::id();
        let mut rent_account = rent::create_account(1, &RentCalculator::default());
        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, true, &mut program.account),
            KeyedAccount::new(&rent_id, false, &mut rent_account),
        ];

        MoveProcessor::do_finalize(&mut keyed_accounts).unwrap();

        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, true, &mut program.account),
            KeyedAccount::new(&genesis.key, false, &mut genesis.account),
        ];

        MoveProcessor::do_invoke_main(
            &mut keyed_accounts,
            &bincode::serialize(&InvokeCommand::RunProgram {
                sender_address,
                function_name: "main".to_string(),
                args: vec![],
            })
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn test_invoke_endless_loop() {
        solana_logger::setup();

        let code = "
            main() {
                loop {}
                return;
            }
        ";
        let sender_address = AccountAddress::default();
        let mut program = LibraAccount::create_program(&sender_address, code, vec![]);
        let mut genesis = LibraAccount::create_genesis(1_000_000_000);

        let rent_id = rent::id();
        let mut rent_account = rent::create_account(1, &RentCalculator::default());
        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, true, &mut program.account),
            KeyedAccount::new(&rent_id, false, &mut rent_account),
        ];

        MoveProcessor::do_finalize(&mut keyed_accounts).unwrap();

        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, true, &mut program.account),
            KeyedAccount::new(&genesis.key, false, &mut genesis.account),
        ];

        assert_eq!(
            MoveProcessor::do_invoke_main(
                &mut keyed_accounts,
                &bincode::serialize(&InvokeCommand::RunProgram {
                    sender_address,
                    function_name: "main".to_string(),
                    args: vec![],
                })
                .unwrap(),
            ),
            Err(InstructionError::InsufficientFunds)
        );
    }

    #[test]
    fn test_invoke_mint_to_address() {
        solana_logger::setup();

        let amount = 42;
        let accounts = mint_coins(amount).unwrap();

        let mut data_store = DataStore::default();
        match bincode::deserialize(&accounts[GENESIS_INDEX + 1].account.data).unwrap() {
            LibraAccountState::User(owner, write_set) => {
                if owner != accounts[GENESIS_INDEX].key {
                    panic!();
                }
                data_store.apply_write_set(&write_set)
            }
            _ => panic!("Invalid account state"),
        }
        let payee_resource = data_store
            .read_account_resource(&accounts[GENESIS_INDEX + 1].address)
            .unwrap();

        assert_eq!(amount, AccountResource::read_balance(&payee_resource));
        assert_eq!(0, AccountResource::read_sequence_number(&payee_resource));
    }

    #[test]
    fn test_invoke_pay_from_sender() {
        solana_logger::setup();
        let amount_to_mint = 42;
        let mut accounts = mint_coins(amount_to_mint).unwrap();

        let code = "
            import 0x0.LibraAccount;
            import 0x0.LibraCoin;
            main(payee: address, amount: u64) {
                LibraAccount.pay_from_sender(move(payee), move(amount));
                return;
            }
        ";
        let mut program =
            LibraAccount::create_program(&accounts[GENESIS_INDEX + 1].address, code, vec![]);
        let mut payee = LibraAccount::create_unallocated();

        let (genesis, sender) = accounts.split_at_mut(GENESIS_INDEX + 1);
        let genesis = &mut genesis[1];
        let sender = &mut sender[0];

        let rent_id = rent::id();
        let mut rent_account = rent::create_account(1, &RentCalculator::default());
        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, true, &mut program.account),
            KeyedAccount::new(&rent_id, false, &mut rent_account),
        ];
        keyed_accounts[0].account.data.resize(BIG_ENOUGH, 0);

        MoveProcessor::do_finalize(&mut keyed_accounts).unwrap();

        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, true, &mut program.account),
            KeyedAccount::new(&genesis.key, false, &mut genesis.account),
            KeyedAccount::new(&sender.key, false, &mut sender.account),
            KeyedAccount::new(&payee.key, false, &mut payee.account),
        ];
        keyed_accounts[2].account.data.resize(BIG_ENOUGH, 0);
        keyed_accounts[3].account.data.resize(BIG_ENOUGH, 0);

        let amount = 2;
        MoveProcessor::do_invoke_main(
            &mut keyed_accounts,
            &bincode::serialize(&InvokeCommand::RunProgram {
                sender_address: sender.address.clone(),
                function_name: "main".to_string(),
                args: vec![
                    TransactionArgument::Address(payee.address.clone()),
                    TransactionArgument::U64(amount),
                ],
            })
            .unwrap(),
        )
        .unwrap();

        let data_store =
            MoveProcessor::keyed_accounts_to_data_store(&genesis.key, &keyed_accounts[1..])
                .unwrap();
        let sender_resource = data_store.read_account_resource(&sender.address).unwrap();
        let payee_resource = data_store.read_account_resource(&payee.address).unwrap();

        assert_eq!(
            amount_to_mint - amount,
            AccountResource::read_balance(&sender_resource)
        );
        assert_eq!(0, AccountResource::read_sequence_number(&sender_resource));
        assert_eq!(amount, AccountResource::read_balance(&payee_resource));
        assert_eq!(0, AccountResource::read_sequence_number(&payee_resource));
    }

    #[test]
    fn test_invoke_local_module() {
        solana_logger::setup();

        let code = "
            modules:

            module M {
                public universal_truth(): u64 {
                    return 42;
                }
            }

            script:

            import Transaction.M;
            main() {
                let x: u64;
                x = M.universal_truth();
                return;
            }
        ";
        let mut genesis = LibraAccount::create_genesis(1_000_000_000);
        let mut payee = LibraAccount::create_unallocated();
        let mut program = LibraAccount::create_program(&payee.address, code, vec![]);

        let rent_id = rent::id();
        let mut rent_account = rent::create_account(1, &RentCalculator::default());
        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, true, &mut program.account),
            KeyedAccount::new(&rent_id, false, &mut rent_account),
        ];
        keyed_accounts[0].account.data.resize(BIG_ENOUGH, 0);

        MoveProcessor::do_finalize(&mut keyed_accounts).unwrap();

        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, true, &mut program.account),
            KeyedAccount::new(&genesis.key, false, &mut genesis.account),
            KeyedAccount::new(&payee.key, false, &mut payee.account),
        ];
        keyed_accounts[2].account.data.resize(BIG_ENOUGH, 0);

        MoveProcessor::do_invoke_main(
            &mut keyed_accounts,
            &bincode::serialize(&InvokeCommand::RunProgram {
                sender_address: payee.address,
                function_name: "main".to_string(),
                args: vec![],
            })
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn test_invoke_published_module() {
        solana_logger::setup();

        // First publish the module

        let code = "
            module M {
                public universal_truth(): u64 {
                    return 42;
                }
            }
        ";
        let mut module = LibraAccount::create_unallocated();
        let mut program = LibraAccount::create_program(&module.address, code, vec![]);
        let mut genesis = LibraAccount::create_genesis(1_000_000_000);

        let rent_id = rent::id();
        let mut rent_account = rent::create_account(1, &RentCalculator::default());
        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, true, &mut program.account),
            KeyedAccount::new(&rent_id, false, &mut rent_account),
        ];
        keyed_accounts[0].account.data.resize(BIG_ENOUGH, 0);

        MoveProcessor::do_finalize(&mut keyed_accounts).unwrap();

        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, true, &mut program.account),
            KeyedAccount::new(&genesis.key, false, &mut genesis.account),
            KeyedAccount::new(&module.key, false, &mut module.account),
        ];
        keyed_accounts[2].account.data.resize(BIG_ENOUGH, 0);

        MoveProcessor::do_invoke_main(
            &mut keyed_accounts,
            &bincode::serialize(&InvokeCommand::RunProgram {
                sender_address: module.address,
                function_name: "main".to_string(),
                args: vec![],
            })
            .unwrap(),
        )
        .unwrap();

        // Next invoke the published module

        let code = format!(
            "
            import 0x{}.M;
            main() {{
                let x: u64;
                x = M.universal_truth();
                return;
            }}
            ",
            module.address
        );
        let mut program =
            LibraAccount::create_program(&module.address, &code, vec![&module.account.data]);

        let rent_id = rent::id();
        let mut rent_account = rent::create_account(1, &RentCalculator::default());
        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, true, &mut program.account),
            KeyedAccount::new(&rent_id, false, &mut rent_account),
        ];

        MoveProcessor::do_finalize(&mut keyed_accounts).unwrap();

        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, true, &mut program.account),
            KeyedAccount::new(&genesis.key, false, &mut genesis.account),
            KeyedAccount::new(&module.key, false, &mut module.account),
        ];

        MoveProcessor::do_invoke_main(
            &mut keyed_accounts,
            &bincode::serialize(&InvokeCommand::RunProgram {
                sender_address: program.address,
                function_name: "main".to_string(),
                args: vec![],
            })
            .unwrap(),
        )
        .unwrap();
    }

    // Helpers

    fn mint_coins(amount: u64) -> Result<Vec<LibraAccount>, InstructionError> {
        let code = "
            import 0x0.LibraAccount;
            import 0x0.LibraCoin;
            main(payee: address, amount: u64) {
                LibraAccount.mint_to_address(move(payee), move(amount));
                return;
            }
        ";
        let mut genesis = LibraAccount::create_genesis(1_000_000_000);
        let mut program = LibraAccount::create_program(&genesis.address, code, vec![]);
        let mut payee = LibraAccount::create_unallocated();

        let rent_id = rent::id();
        let mut rent_account = rent::create_account(1, &RentCalculator::default());
        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, true, &mut program.account),
            KeyedAccount::new(&rent_id, false, &mut rent_account),
        ];
        keyed_accounts[0].account.data.resize(BIG_ENOUGH, 0);

        MoveProcessor::do_finalize(&mut keyed_accounts).unwrap();

        let mut keyed_accounts = vec![
            KeyedAccount::new(&program.key, true, &mut program.account),
            KeyedAccount::new(&genesis.key, false, &mut genesis.account),
            KeyedAccount::new(&payee.key, false, &mut payee.account),
        ];
        keyed_accounts[2].account.data.resize(BIG_ENOUGH, 0);

        MoveProcessor::do_invoke_main(
            &mut keyed_accounts,
            &bincode::serialize(&InvokeCommand::RunProgram {
                sender_address: genesis.address.clone(),
                function_name: "main".to_string(),
                args: vec![
                    TransactionArgument::Address(pubkey_to_address(&payee.key)),
                    TransactionArgument::U64(amount),
                ],
            })
            .unwrap(),
        )
        .unwrap();

        Ok(vec![
            LibraAccount::new(program.key, program.account),
            LibraAccount::new(genesis.key, genesis.account),
            LibraAccount::new(payee.key, payee.account),
        ])
    }

    #[derive(Eq, PartialEq, Debug, Default)]
    struct LibraAccount {
        pub key: Pubkey,
        pub address: AccountAddress,
        pub account: Account,
    }
    impl LibraAccount {
        pub fn new(key: Pubkey, account: Account) -> Self {
            let address = pubkey_to_address(&key);
            Self {
                key,
                address,
                account,
            }
        }

        pub fn create_unallocated() -> Self {
            let key = Pubkey::new_rand();
            let account = Account {
                lamports: 1,
                data: bincode::serialize(&LibraAccountState::create_unallocated()).unwrap(),
                owner: id(),
                ..Account::default()
            };
            Self::new(key, account)
        }

        pub fn create_genesis(amount: u64) -> Self {
            let account = Account {
                lamports: 1,
                owner: id(),
                ..Account::default()
            };
            let mut genesis = Self::new(Pubkey::default(), account);
            genesis.account.data =
                bincode::serialize(&LibraAccountState::create_genesis(amount).unwrap()).unwrap();
            genesis
        }

        pub fn create_program(
            sender_address: &AccountAddress,
            code: &str,
            deps: Vec<&Vec<u8>>,
        ) -> Self {
            let mut program = Self::create_unallocated();
            program.account.data = bincode::serialize(&LibraAccountState::create_program(
                sender_address,
                code,
                deps,
            ))
            .unwrap();
            program.account.executable = true;
            program
        }
    }
}
