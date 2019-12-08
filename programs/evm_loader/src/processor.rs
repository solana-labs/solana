use log::*;
use std::collections::BTreeMap;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::{
    instruction_processor_utils::{limited_deserialize, next_keyed_account},
    loader_instruction::LoaderInstruction,
};
use solana_sdk::pubkey::Pubkey;
use primitive_types::{H160, H256, U256};
use evm::backend::{MemoryVicinity, MemoryAccount, MemoryBackend, Apply};
use evm::executor::StackExecutor;

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    solana_logger::setup();

    match limited_deserialize(data)? {
        LoaderInstruction::Write { offset, bytes } =>
            EVMProcessor::do_write(keyed_accounts, offset, &bytes),
        LoaderInstruction::Finalize =>
            EVMProcessor::do_finalize(keyed_accounts),
        LoaderInstruction::InvokeMain { data } =>
            EVMProcessor::do_invoke_main(keyed_accounts, &data),
    }
}

pub struct EVMProcessor;

impl EVMProcessor {
    fn read_account(bytes: &[u8]) -> Result<(H160, MemoryAccount), InstructionError> {
        limited_deserialize(bytes)
    }

    fn write_account(address: H160, account: MemoryAccount) -> Result<Vec<u8>, InstructionError> {
        bincode::serialize(&(address, account)).map_err(|_| InstructionError::InvalidAccountData)
    }

    fn apply_accounts(
        applies: impl IntoIterator<Item=Apply<impl IntoIterator<Item=(H256, H256)>>>,
        keyed_accounts: &mut [KeyedAccount],
    ) -> Result<(), InstructionError> {
        for apply in applies {
            match apply {
                Apply::Modify {
                    address,
                    basic,
                    code,
                    storage,
                    reset_storage,
                } => {
                    let mut processed = false;
                    'inner_modify: for state_account in keyed_accounts.iter_mut() {
                        let (keyed_address, mut keyed_account) =
                            Self::read_account(&state_account.account.data)?;

                        if keyed_address == address {
                            keyed_account.nonce = basic.nonce;
                            keyed_account.balance = basic.balance;

                            if reset_storage {
                                keyed_account.storage = Default::default();
                            }

                            for (key, value) in storage {
                                keyed_account.storage.insert(key, value);
                            }

                            if let Some(code) = code {
                                keyed_account.code = code;
                            }

                            state_account.account.data =
                                Self::write_account(keyed_address, keyed_account)?;
                            processed = true;
                            break 'inner_modify
                        }
                    }

                    if !processed {
                        return Err(InstructionError::NotEnoughAccountKeys)
                    }
                },
                Apply::Delete {
                    address
                } => {
                    let mut processed = false;
                    'inner_delete: for state_account in keyed_accounts.iter_mut() {
                        let (keyed_address, _) =
                            Self::read_account(&state_account.account.data)?;

                        if keyed_address == address {
                            let keyed_account = Default::default();

                            state_account.account.data =
                                Self::write_account(keyed_address, keyed_account)?;
                            processed = true;
                            break 'inner_delete
                        }
                    }

                    if !processed {
                        return Err(InstructionError::NotEnoughAccountKeys)
                    }
                },
            }
        }

        Ok(())
    }

    fn pubkey_to_address(key: &Pubkey) -> H160 {
        H256::from_slice(key.as_ref()).into()
    }

    pub fn do_write(
        keyed_accounts: &mut [KeyedAccount],
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), InstructionError> {
        let mut keyed_accounts_iter = keyed_accounts.iter_mut();
        let keyed_account = next_keyed_account(&mut keyed_accounts_iter)?;

        if keyed_account.signer_key().is_none() {
            debug!("Error: key[0] did not sign the transaction");
            return Err(InstructionError::MissingRequiredSignature);
        }
        let offset = offset as usize;
        let len = bytes.len();
        trace!("Write: offset={} length={}", offset, len);
        if keyed_account.account.data.len() < offset + len {
            debug!(
                "Error: Write overflow: {} < {}",
                keyed_account.account.data.len(),
                offset + len
            );
            return Err(InstructionError::AccountDataTooSmall);
        }
        keyed_account.account.data[offset..offset + len].copy_from_slice(&bytes);
        Ok(())
    }

    pub fn do_finalize(keyed_accounts: &mut [KeyedAccount]) -> Result<(), InstructionError> {
        let mut keyed_accounts_iter = keyed_accounts.iter_mut();
        let keyed_account = next_keyed_account(&mut keyed_accounts_iter)?;
        let keyed_address = keyed_account.signer_key().map(|k| Self::pubkey_to_address(k))
            .ok_or(InstructionError::MissingRequiredSignature)?;

        let vicinity = MemoryVicinity {
            gas_price: U256::zero(),
            origin: H160::default(),
            chain_id: U256::zero(),
            block_hashes: Vec::new(),
            block_number: U256::zero(),
            block_coinbase: H160::default(),
            block_timestamp: U256::zero(),
            block_difficulty: U256::zero(),
            block_gas_limit: U256::zero(),
        };
        let code = keyed_account.account.data.clone();
        let mut state = BTreeMap::new();

        for state_account in keyed_accounts_iter {
            let (address, account) = Self::read_account(&state_account.account.data)?;

            state.insert(address, account);
        }
        let backend = MemoryBackend::new(&vicinity, state);
        let config = evm::Config::istanbul();
        let mut executor = StackExecutor::new(&backend, usize::max_value(), &config);
        let exit_reason =
            executor.transact_create(keyed_address, U256::zero(), code, usize::max_value());

        let (applies, _logs) = executor.deconstruct();

        if exit_reason.is_succeed() {
            keyed_account.account.executable = true;
        }

        Self::apply_accounts(applies, keyed_accounts)?;

        Ok(())
    }

    pub fn do_invoke_main(
        keyed_accounts: &mut [KeyedAccount],
        data: &[u8],
    ) -> Result<(), InstructionError> {
        let keyed_address = next_keyed_account(&mut keyed_accounts.iter_mut())?
            .signer_key().map(|k| Self::pubkey_to_address(k))
            .ok_or(InstructionError::MissingRequiredSignature)?;

        let vicinity = MemoryVicinity {
            gas_price: U256::zero(),
            origin: H160::default(),
            chain_id: U256::zero(),
            block_hashes: Vec::new(),
            block_number: U256::zero(),
            block_coinbase: H160::default(),
            block_timestamp: U256::zero(),
            block_difficulty: U256::zero(),
            block_gas_limit: U256::zero(),
        };
        let mut state = BTreeMap::new();

        for state_account in keyed_accounts.iter_mut() {
            let (address, account) = Self::read_account(&state_account.account.data)?;

            state.insert(address, account);
        }
        let backend = MemoryBackend::new(&vicinity, state);
        let config = evm::Config::istanbul();
        let mut executor = StackExecutor::new(&backend, usize::max_value(), &config);
        let _ = executor.transact_call(
            H160::default(),
            keyed_address,
            U256::zero(),
            data.to_vec(),
            usize::max_value(),
        );

        let (applies, _logs) = executor.deconstruct();

        Self::apply_accounts(applies, keyed_accounts)?;

        Ok(())
    }
}
