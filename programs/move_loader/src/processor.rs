use crate::account_state::{pubkey_to_address, LibraAccountState};
use crate::data_store::DataStore;
use crate::error_mappers::*;
use bytecode_verifier::verifier::{VerifiedModule, VerifiedScript};
use log::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{
    account::KeyedAccount,
    account_utils::State,
    instruction::InstructionError,
    move_loader::id,
    program_utils::{is_executable, limited_deserialize, next_keyed_account},
    pubkey::Pubkey,
    sysvar::rent,
};
use types::{
    account_address::AccountAddress,
    account_config,
    identifier::Identifier,
    transaction::{Module, Script, TransactionArgument, TransactionOutput},
};
use vm::{
    access::ModuleAccess,
    file_format::{CompiledModule, CompiledScript},
    gas_schedule::{MAXIMUM_NUMBER_OF_GAS_UNITS, MAX_PRICE_PER_GAS_UNIT},
    transaction_metadata::TransactionMetadata,
    vm_string::VMString,
};
use vm_cache_map::Arena;
use vm_runtime::{
    code_cache::{
        module_adapter::ModuleFetcherImpl,
        module_cache::{BlockModuleCache, ModuleCache, VMModuleCache},
    },
    txn_executor::TransactionExecutor,
};
use vm_runtime_types::value::Value;

/// Instruction data passed to perform a loader operation, must be based
/// on solana_sdk::loader_instruction::LoaderInstruction
#[derive(Serialize, Deserialize, Debug)]
pub enum MoveLoaderInstruction {
    /// Write program data into an account
    ///
    /// * key[0] - the account to write into.
    ///
    /// The transaction must be signed by key[0]
    Write {
        offset: u32,
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    },

    /// Finalize an account loaded with program data for execution.
    /// The exact preparation steps is loader specific but on success the loader must set the executable
    /// bit of the Account
    ///
    /// * key[0] - the account to prepare for execution
    /// * key[1] - rent sysvar account
    ///
    /// The transaction must be signed by key[0]
    Finalize,

    /// Create a new genesis account
    ///
    /// * key[0] - the account to write the genesis into
    ///
    /// The transaction must be signed by key[0]
    CreateGenesis(u64),
}

/// Instruction data passed when executing a Move script
#[derive(Serialize, Deserialize, Debug)]
pub enum Executable {
    RunScript {
        /// Sender of the "transaction", the "sender" who is running this script
        sender_address: AccountAddress,
        /// Name of the script's function to call
        function_name: String,
        /// Arguments to pass to the script being called
        args: Vec<TransactionArgument>,
    },
}

pub struct MoveProcessor {}

impl MoveProcessor {
    #[allow(clippy::needless_pass_by_value)]
    fn missing_account() -> InstructionError {
        debug!("Error: Missing libra account");
        InstructionError::InvalidAccountData
    }

    fn arguments_to_values(args: Vec<TransactionArgument>) -> Vec<Value> {
        let mut locals = vec![];
        for arg in args {
            locals.push(match arg {
                TransactionArgument::U64(i) => Value::u64(i),
                TransactionArgument::Address(a) => Value::address(a),
                TransactionArgument::ByteArray(b) => Value::byte_array(b),
                TransactionArgument::String(s) => Value::string(VMString::new(s)),
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

    fn deserialize_verified_script(data: &[u8]) -> Result<VerifiedScript, InstructionError> {
        match limited_deserialize(data)? {
            LibraAccountState::VerifiedScript { script_bytes } => {
                let script =
                    VerifiedScript::deserialize(&script_bytes).map_err(map_err_vm_status)?;
                Ok(script)
            }
            _ => {
                debug!("Error: Script account does not contain a script");
                Err(InstructionError::InvalidArgument)
            }
        }
    }

    fn execute(
        sender_address: AccountAddress,
        function_name: &str,
        args: Vec<TransactionArgument>,
        script: VerifiedScript,
        data_store: &DataStore,
    ) -> Result<TransactionOutput, InstructionError> {
        let allocator = Arena::new();
        let code_cache = VMModuleCache::new(&allocator);
        let module_cache = BlockModuleCache::new(&code_cache, ModuleFetcherImpl::new(data_store));
        let modules_to_publish = vec![];

        let main_module = script.into_module();
        let module_id = main_module.self_id();
        module_cache.cache_module(main_module);

        let mut txn_metadata = TransactionMetadata::default();
        txn_metadata.sender = sender_address;

        // Caps execution to the Libra prescribed 10 milliseconds
        txn_metadata.max_gas_amount = *MAXIMUM_NUMBER_OF_GAS_UNITS;
        txn_metadata.gas_unit_price = *MAX_PRICE_PER_GAS_UNIT;

        let mut vm = TransactionExecutor::new(&module_cache, data_store, txn_metadata);
        vm.execute_function(
            &module_id,
            &Identifier::new(function_name).unwrap(),
            Self::arguments_to_values(args),
        )
        .map_err(map_err_vm_status)?;

        Ok(vm
            .make_write_set(modules_to_publish, Ok(()))
            .map_err(map_err_vm_status)?)
    }

    fn keyed_accounts_to_data_store(
        keyed_accounts: &[KeyedAccount],
    ) -> Result<DataStore, InstructionError> {
        let mut data_store = DataStore::default();

        let mut keyed_accounts_iter = keyed_accounts.iter();
        let genesis = next_keyed_account(&mut keyed_accounts_iter)?;

        match limited_deserialize(&genesis.try_account_ref()?.data)? {
            LibraAccountState::Genesis(write_set) => data_store.apply_write_set(&write_set),
            _ => {
                debug!("Must include a genesis account");
                return Err(InstructionError::InvalidArgument);
            }
        }

        for keyed_account in keyed_accounts_iter {
            match limited_deserialize(&keyed_account.try_account_ref()?.data)? {
                LibraAccountState::User(owner, write_set) => {
                    if owner != *genesis.unsigned_key() {
                        debug!("User account must be owned by this genesis");
                        return Err(InstructionError::InvalidArgument);
                    }
                    data_store.apply_write_set(&write_set)
                }
                LibraAccountState::PublishedModule(write_set) => {
                    data_store.apply_write_set(&write_set)
                }
                _ => (),
            }
        }
        Ok(data_store)
    }

    fn data_store_to_keyed_accounts(
        data_store: DataStore,
        keyed_accounts: &[KeyedAccount],
    ) -> Result<(), InstructionError> {
        let mut write_sets = data_store
            .into_write_sets()
            .map_err(|_| InstructionError::GenericError)?;

        let mut keyed_accounts_iter = keyed_accounts.iter();

        // Genesis account holds both mint and stdlib
        let genesis = next_keyed_account(&mut keyed_accounts_iter)?;
        let genesis_key = *genesis.unsigned_key();
        let mut write_set = write_sets
            .remove(&account_config::association_address())
            .ok_or_else(Self::missing_account)?
            .into_mut();
        for (access_path, write_op) in write_sets
            .remove(&account_config::core_code_address())
            .ok_or_else(Self::missing_account)?
            .into_iter()
        {
            write_set.push((access_path, write_op));
        }
        let write_set = write_set.freeze().unwrap();
        Self::serialize_and_enforce_length(
            &LibraAccountState::Genesis(write_set),
            &mut genesis.try_account_ref_mut()?.data,
        )?;

        // Now do the rest of the accounts
        for keyed_account in keyed_accounts_iter {
            let write_set = write_sets
                .remove(&pubkey_to_address(keyed_account.unsigned_key()))
                .ok_or_else(Self::missing_account)?;
            if !keyed_account.executable()? {
                // Only write back non-executable accounts
                Self::serialize_and_enforce_length(
                    &LibraAccountState::User(genesis_key, write_set),
                    &mut keyed_account.try_account_ref_mut()?.data,
                )?;
            }
        }
        if !write_sets.is_empty() {
            debug!("Error: Missing keyed accounts");
            return Err(InstructionError::GenericError);
        }
        Ok(())
    }

    pub fn do_write(
        keyed_accounts: &[KeyedAccount],
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), InstructionError> {
        let mut keyed_accounts_iter = keyed_accounts.iter();
        let keyed_account = next_keyed_account(&mut keyed_accounts_iter)?;

        if keyed_account.signer_key().is_none() {
            debug!("Error: key[0] did not sign the transaction");
            return Err(InstructionError::MissingRequiredSignature);
        }
        let offset = offset as usize;
        let len = bytes.len();
        trace!("Write: offset={} length={}", offset, len);
        if keyed_account.data_len()? < offset + len {
            debug!(
                "Error: Write overflow: {} < {}",
                keyed_account.data_len()?,
                offset + len
            );
            return Err(InstructionError::AccountDataTooSmall);
        }
        keyed_account.try_account_ref_mut()?.data[offset..offset + len].copy_from_slice(&bytes);
        Ok(())
    }

    pub fn do_finalize(keyed_accounts: &[KeyedAccount]) -> Result<(), InstructionError> {
        let mut keyed_accounts_iter = keyed_accounts.iter();
        let finalized = next_keyed_account(&mut keyed_accounts_iter)?;
        let rent = next_keyed_account(&mut keyed_accounts_iter)?;

        if finalized.signer_key().is_none() {
            debug!("Error: account to finalize did not sign the transaction");
            return Err(InstructionError::MissingRequiredSignature);
        }

        rent::verify_rent_exemption(&finalized, &rent)?;

        match finalized.state()? {
            LibraAccountState::CompiledScript(string) => {
                let script: Script = serde_json::from_str(&string).map_err(map_json_error)?;
                let compiled_script =
                    CompiledScript::deserialize(&script.code()).map_err(map_err_vm_status)?;
                let verified_script = match VerifiedScript::new(compiled_script) {
                    Ok(script) => script,
                    Err((_, errors)) => {
                        if errors.is_empty() {
                            return Err(InstructionError::GenericError);
                        } else {
                            return Err(map_err_vm_status(errors[0].clone()));
                        }
                    }
                };
                let mut script_bytes = vec![];
                verified_script
                    .as_inner()
                    .serialize(&mut script_bytes)
                    .map_err(map_failure_error)?;
                Self::serialize_and_enforce_length(
                    &LibraAccountState::VerifiedScript { script_bytes },
                    &mut finalized.try_account_ref_mut()?.data,
                )?;
                info!("Finalize script: {:?}", finalized.unsigned_key());
            }
            LibraAccountState::CompiledModule(string) => {
                let module: Module = serde_json::from_str(&string).map_err(map_json_error)?;
                let compiled_module =
                    CompiledModule::deserialize(&module.code()).map_err(map_err_vm_status)?;
                let verified_module =
                    VerifiedModule::new(compiled_module).map_err(map_vm_verification_error)?;

                let mut data_store = DataStore::default();
                data_store
                    .add_module(&verified_module)
                    .map_err(map_failure_error)?;

                let mut write_sets = data_store
                    .into_write_sets()
                    .map_err(|_| InstructionError::GenericError)?;

                let write_set = write_sets
                    .remove(&pubkey_to_address(finalized.unsigned_key()))
                    .ok_or_else(Self::missing_account)?;
                Self::serialize_and_enforce_length(
                    &LibraAccountState::PublishedModule(write_set),
                    &mut finalized.try_account_ref_mut()?.data,
                )?;

                if !write_sets.is_empty() {
                    debug!("Error: Missing keyed accounts");
                    return Err(InstructionError::GenericError);
                }

                info!("Finalize module: {:?}", finalized.unsigned_key());
            }
            _ => {
                debug!("Error: Account to finalize does not contain compiled data");
                return Err(InstructionError::InvalidArgument);
            }
        };

        finalized.try_account_ref_mut()?.executable = true;

        Ok(())
    }

    pub fn do_create_genesis(
        keyed_accounts: &[KeyedAccount],
        amount: u64,
    ) -> Result<(), InstructionError> {
        let mut keyed_accounts_iter = keyed_accounts.iter();
        let genesis = next_keyed_account(&mut keyed_accounts_iter)?;

        if genesis.owner()? != id() {
            debug!("Error: Move genesis account not owned by Move loader");
            return Err(InstructionError::InvalidArgument);
        }

        match genesis.state()? {
            LibraAccountState::Unallocated => Self::serialize_and_enforce_length(
                &LibraAccountState::create_genesis(amount)?,
                &mut genesis.try_account_ref_mut()?.data,
            ),
            _ => {
                debug!("Error: Must provide an unallocated account");
                Err(InstructionError::InvalidArgument)
            }
        }
    }

    pub fn do_invoke_main(
        keyed_accounts: &[KeyedAccount],
        sender_address: AccountAddress,
        function_name: String,
        args: Vec<TransactionArgument>,
    ) -> Result<(), InstructionError> {
        let mut keyed_accounts_iter = keyed_accounts.iter();
        let script = next_keyed_account(&mut keyed_accounts_iter)?;

        trace!(
            "Run script {:?} with entrypoint {:?}",
            script.unsigned_key(),
            function_name
        );

        if script.owner()? != id() {
            debug!("Error: Move script account not owned by Move loader");
            return Err(InstructionError::InvalidArgument);
        }
        if !script.executable()? {
            debug!("Error: Move script account not executable");
            return Err(InstructionError::AccountNotExecutable);
        }

        let data_accounts = keyed_accounts_iter.as_slice();

        let mut data_store = Self::keyed_accounts_to_data_store(&data_accounts)?;
        let verified_script = Self::deserialize_verified_script(&script.try_account_ref()?.data)?;

        let output = Self::execute(
            sender_address,
            &function_name,
            args,
            verified_script,
            &data_store,
        )?;
        for event in output.events() {
            trace!("Event: {:?}", event);
        }

        data_store.apply_write_set(&output.write_set());
        Self::data_store_to_keyed_accounts(data_store, data_accounts)
    }

    pub fn process_instruction(
        _program_id: &Pubkey,
        keyed_accounts: &[KeyedAccount],
        instruction_data: &[u8],
    ) -> Result<(), InstructionError> {
        solana_logger::setup();

        if is_executable(keyed_accounts)? {
            match limited_deserialize(&instruction_data)? {
                Executable::RunScript {
                    sender_address,
                    function_name,
                    args,
                } => Self::do_invoke_main(keyed_accounts, sender_address, function_name, args),
            }
        } else {
            match limited_deserialize(instruction_data)? {
                MoveLoaderInstruction::Write { offset, bytes } => {
                    Self::do_write(keyed_accounts, offset, &bytes)
                }
                MoveLoaderInstruction::Finalize => Self::do_finalize(keyed_accounts),
                MoveLoaderInstruction::CreateGenesis(amount) => {
                    Self::do_create_genesis(keyed_accounts, amount)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::account::Account;
    use solana_sdk::rent::Rent;
    use solana_sdk::sysvar::rent;
    use std::cell::RefCell;

    const BIG_ENOUGH: usize = 10_000;

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
        let script = LibraAccount::create_script(&sender_address, code, vec![]);
        let rent_id = rent::id();
        let rent_account = RefCell::new(rent::create_account(1, &Rent::free()));
        let keyed_accounts = vec![
            KeyedAccount::new(&script.key, true, &script.account),
            KeyedAccount::new(&rent_id, false, &rent_account),
        ];
        MoveProcessor::do_finalize(&keyed_accounts).unwrap();
        let _ = MoveProcessor::deserialize_verified_script(&script.account.borrow().data).unwrap();
    }

    #[test]
    fn test_create_genesis_account() {
        solana_logger::setup();

        let amount = 10_000_000;
        let unallocated = LibraAccount::create_unallocated(BIG_ENOUGH);

        let keyed_accounts = vec![KeyedAccount::new(
            &unallocated.key,
            false,
            &unallocated.account,
        )];
        MoveProcessor::do_create_genesis(&keyed_accounts, amount).unwrap();

        assert_eq!(
            bincode::deserialize::<LibraAccountState>(
                &LibraAccount::create_genesis(amount).account.borrow().data
            )
            .unwrap(),
            bincode::deserialize::<LibraAccountState>(&keyed_accounts[0].account.borrow().data)
                .unwrap()
        );
    }

    #[test]
    fn test_invoke_script() {
        solana_logger::setup();

        let code = "main() { return; }";
        let sender_address = AccountAddress::default();
        let script = LibraAccount::create_script(&sender_address, code, vec![]);
        let genesis = LibraAccount::create_genesis(1_000_000_000);

        let rent_id = rent::id();
        let rent_account = RefCell::new(rent::create_account(1, &Rent::free()));
        let keyed_accounts = vec![
            KeyedAccount::new(&script.key, true, &script.account),
            KeyedAccount::new(&rent_id, false, &rent_account),
        ];

        MoveProcessor::do_finalize(&keyed_accounts).unwrap();

        let keyed_accounts = vec![
            KeyedAccount::new(&script.key, true, &script.account),
            KeyedAccount::new(&genesis.key, false, &genesis.account),
        ];

        MoveProcessor::do_invoke_main(&keyed_accounts, sender_address, "main".to_string(), vec![])
            .unwrap();
    }

    #[test]
    fn test_publish_module() {
        solana_logger::setup();

        let code = "
            module M {
                universal_truth(): u64 {
                    return 42;
                }
            }
        ";
        let module = LibraAccount::create_module(code, vec![]);
        let rent_id = rent::id();
        let rent_account = RefCell::new(rent::create_account(1, &Rent::free()));
        let keyed_accounts = vec![
            KeyedAccount::new(&module.key, true, &module.account),
            KeyedAccount::new(&rent_id, false, &rent_account),
        ];
        keyed_accounts[0]
            .account
            .borrow_mut()
            .data
            .resize(BIG_ENOUGH, 0);

        MoveProcessor::do_finalize(&keyed_accounts).unwrap();
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
        let script = LibraAccount::create_script(&sender_address, code, vec![]);
        let genesis = LibraAccount::create_genesis(1_000_000_000);

        let rent_id = rent::id();
        let rent_account = RefCell::new(rent::create_account(1, &Rent::free()));
        let keyed_accounts = vec![
            KeyedAccount::new(&script.key, true, &script.account),
            KeyedAccount::new(&rent_id, false, &rent_account),
        ];

        MoveProcessor::do_finalize(&keyed_accounts).unwrap();

        let keyed_accounts = vec![
            KeyedAccount::new(&script.key, true, &script.account),
            KeyedAccount::new(&genesis.key, false, &genesis.account),
        ];

        assert_eq!(
            MoveProcessor::do_invoke_main(
                &keyed_accounts,
                sender_address,
                "main".to_string(),
                vec![],
            ),
            Err(InstructionError::CustomError(4002))
        );
    }

    #[test]
    fn test_invoke_mint_to_address() {
        solana_logger::setup();

        let mut data_store = DataStore::default();

        let amount = 42;
        let accounts = mint_coins(amount).unwrap();
        let mut accounts_iter = accounts.iter();

        let _script = next_libra_account(&mut accounts_iter).unwrap();
        let genesis = next_libra_account(&mut accounts_iter).unwrap();
        let payee = next_libra_account(&mut accounts_iter).unwrap();
        match bincode::deserialize(&payee.account.borrow().data).unwrap() {
            LibraAccountState::User(owner, write_set) => {
                if owner != genesis.key {
                    panic!();
                }
                data_store.apply_write_set(&write_set)
            }
            _ => panic!("Invalid account state"),
        }

        let payee_resource = data_store.read_account_resource(&payee.address).unwrap();
        assert_eq!(amount, payee_resource.balance());
        assert_eq!(0, payee_resource.sequence_number());
    }

    #[test]
    fn test_invoke_pay_from_sender() {
        solana_logger::setup();
        let amount_to_mint = 42;
        let accounts = mint_coins(amount_to_mint).unwrap();
        let mut accounts_iter = accounts.iter();

        let _script = next_libra_account(&mut accounts_iter).unwrap();
        let genesis = next_libra_account(&mut accounts_iter).unwrap();
        let sender = next_libra_account(&mut accounts_iter).unwrap();

        let code = "
            import 0x0.LibraAccount;
            import 0x0.LibraCoin;
            main(payee: address, amount: u64) {
                LibraAccount.pay_from_sender(move(payee), move(amount));
                return;
            }
        ";
        let script = LibraAccount::create_script(&genesis.address, code, vec![]);
        let payee = LibraAccount::create_unallocated(BIG_ENOUGH);

        let rent_id = rent::id();
        let rent_account = RefCell::new(rent::create_account(1, &Rent::free()));
        let keyed_accounts = vec![
            KeyedAccount::new(&script.key, true, &script.account),
            KeyedAccount::new(&rent_id, false, &rent_account),
        ];

        MoveProcessor::do_finalize(&keyed_accounts).unwrap();

        let keyed_accounts = vec![
            KeyedAccount::new(&script.key, true, &script.account),
            KeyedAccount::new(&genesis.key, false, &genesis.account),
            KeyedAccount::new(&sender.key, false, &sender.account),
            KeyedAccount::new(&payee.key, false, &payee.account),
        ];

        let amount = 2;
        MoveProcessor::do_invoke_main(
            &keyed_accounts,
            sender.address.clone(),
            "main".to_string(),
            vec![
                TransactionArgument::Address(payee.address.clone()),
                TransactionArgument::U64(amount),
            ],
        )
        .unwrap();

        let data_store = MoveProcessor::keyed_accounts_to_data_store(&keyed_accounts[1..]).unwrap();
        let sender_resource = data_store.read_account_resource(&sender.address).unwrap();
        let payee_resource = data_store.read_account_resource(&payee.address).unwrap();

        assert_eq!(amount_to_mint - amount, sender_resource.balance());
        assert_eq!(0, sender_resource.sequence_number());
        assert_eq!(amount, payee_resource.balance());
        assert_eq!(0, payee_resource.sequence_number());
    }

    #[test]
    fn test_invoke_published_module() {
        solana_logger::setup();

        let universal_truth = 42;

        // First publish the module

        let code = format!(
            "
            module M {{
                public universal_truth(): u64 {{
                    return {};
                }}
            }}
        ",
            universal_truth
        );
        let module = LibraAccount::create_module(&code, vec![]);

        let rent_id = rent::id();
        let rent_account = RefCell::new(rent::create_account(1, &Rent::free()));
        let keyed_accounts = vec![
            KeyedAccount::new(&module.key, true, &module.account),
            KeyedAccount::new(&rent_id, false, &rent_account),
        ];
        keyed_accounts[0]
            .account
            .borrow_mut()
            .data
            .resize(BIG_ENOUGH, 0);

        MoveProcessor::do_finalize(&keyed_accounts).unwrap();

        // Next invoke the published module

        let amount_to_mint = 84;
        let accounts = mint_coins(amount_to_mint).unwrap();
        let mut accounts_iter = accounts.iter();

        let _script = next_libra_account(&mut accounts_iter).unwrap();
        let genesis = next_libra_account(&mut accounts_iter).unwrap();
        let sender = next_libra_account(&mut accounts_iter).unwrap();

        let code = format!(
            "
                import 0x0.LibraAccount;
                import 0x0.LibraCoin;
                import 0x{}.M;

                main(payee: address) {{
                    let amount: u64;
                    amount = M.universal_truth();
                    LibraAccount.pay_from_sender(move(payee), move(amount));
                    return;
                }}
            ",
            module.address
        );
        let script = LibraAccount::create_script(
            &genesis.address,
            &code,
            vec![&module.account.borrow().data],
        );
        let payee = LibraAccount::create_unallocated(BIG_ENOUGH);

        let rent_id = rent::id();
        let rent_account = RefCell::new(rent::create_account(1, &Rent::free()));
        let keyed_accounts = vec![
            KeyedAccount::new(&script.key, true, &script.account),
            KeyedAccount::new(&rent_id, false, &rent_account),
        ];

        MoveProcessor::do_finalize(&keyed_accounts).unwrap();

        let keyed_accounts = vec![
            KeyedAccount::new(&script.key, true, &script.account),
            KeyedAccount::new(&genesis.key, false, &genesis.account),
            KeyedAccount::new(&sender.key, false, &sender.account),
            KeyedAccount::new(&module.key, false, &module.account),
            KeyedAccount::new(&payee.key, false, &payee.account),
        ];

        MoveProcessor::do_invoke_main(
            &keyed_accounts,
            sender.address.clone(),
            "main".to_string(),
            vec![TransactionArgument::Address(payee.address.clone())],
        )
        .unwrap();

        let data_store = MoveProcessor::keyed_accounts_to_data_store(&keyed_accounts[1..]).unwrap();
        let sender_resource = data_store.read_account_resource(&sender.address).unwrap();
        let payee_resource = data_store.read_account_resource(&payee.address).unwrap();

        assert_eq!(amount_to_mint - universal_truth, sender_resource.balance());
        assert_eq!(0, sender_resource.sequence_number());
        assert_eq!(universal_truth, payee_resource.balance());
        assert_eq!(0, payee_resource.sequence_number());
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
        let genesis = LibraAccount::create_genesis(1_000_000_000);
        let script = LibraAccount::create_script(&genesis.address.clone(), code, vec![]);
        let payee = LibraAccount::create_unallocated(BIG_ENOUGH);

        let rent_id = rent::id();
        let rent_account = RefCell::new(rent::create_account(1, &Rent::free()));
        let keyed_accounts = vec![
            KeyedAccount::new(&script.key, true, &script.account),
            KeyedAccount::new(&rent_id, false, &rent_account),
        ];

        MoveProcessor::do_finalize(&keyed_accounts).unwrap();

        let keyed_accounts = vec![
            KeyedAccount::new(&script.key, true, &script.account),
            KeyedAccount::new(&genesis.key, false, &genesis.account),
            KeyedAccount::new(&payee.key, false, &payee.account),
        ];

        MoveProcessor::do_invoke_main(
            &keyed_accounts,
            account_config::association_address(),
            "main".to_string(),
            vec![
                TransactionArgument::Address(pubkey_to_address(&payee.key)),
                TransactionArgument::U64(amount),
            ],
        )
        .unwrap();

        Ok(vec![
            LibraAccount::new(script.key, script.account.into_inner()),
            LibraAccount::new(genesis.key, genesis.account.into_inner()),
            LibraAccount::new(payee.key, payee.account.into_inner()),
        ])
    }

    #[derive(Eq, PartialEq, Debug, Default)]
    struct LibraAccount {
        pub key: Pubkey,
        pub address: AccountAddress,
        pub account: RefCell<Account>,
    }

    pub fn next_libra_account<I: Iterator>(iter: &mut I) -> Result<I::Item, InstructionError> {
        iter.next().ok_or(InstructionError::NotEnoughAccountKeys)
    }

    impl LibraAccount {
        pub fn new(key: Pubkey, account: Account) -> Self {
            let address = pubkey_to_address(&key);
            Self {
                key,
                address,
                account: RefCell::new(account),
            }
        }

        pub fn create_unallocated(size: usize) -> Self {
            let key = Pubkey::new_rand();
            let mut account = Account {
                lamports: 1,
                data: bincode::serialize(&LibraAccountState::create_unallocated()).unwrap(),
                owner: id(),
                ..Account::default()
            };
            if account.data.len() < size {
                account.data.resize(size, 0);
            }

            Self::new(key, account)
        }

        pub fn create_genesis(amount: u64) -> Self {
            let account = Account {
                lamports: 1,
                owner: id(),
                ..Account::default()
            };
            let genesis = Self::new(Pubkey::new_rand(), account);
            let pre_data = LibraAccountState::create_genesis(amount).unwrap();
            let _hi = "hello";
            genesis.account.borrow_mut().data = bincode::serialize(&pre_data).unwrap();
            genesis
        }

        pub fn create_script(
            sender_address: &AccountAddress,
            code: &str,
            deps: Vec<&Vec<u8>>,
        ) -> Self {
            let script = Self::create_unallocated(0);
            script.account.borrow_mut().data = bincode::serialize(
                &LibraAccountState::create_script(sender_address, code, deps),
            )
            .unwrap();

            script
        }

        pub fn create_module(code: &str, deps: Vec<&Vec<u8>>) -> Self {
            let module = Self::create_unallocated(0);
            module.account.borrow_mut().data = bincode::serialize(
                &LibraAccountState::create_module(&module.address, code, deps),
            )
            .unwrap();

            module
        }
    }
}
