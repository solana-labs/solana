//! storage program
//!  Receive mining proofs from miners, validate the answers
//!  and give reward for good proofs.

use crate::storage_contract::StorageAccount;
use crate::storage_instruction::StorageInstruction;
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    tick_height: u64,
) -> Result<(), InstructionError> {
    solana_logger::setup();

    let num_keyed_accounts = keyed_accounts.len();
    let (me, rest) = keyed_accounts.split_at_mut(1);

    // accounts_keys[0] must be signed
    let storage_account_pubkey = me[0].signer_key();
    if storage_account_pubkey.is_none() {
        info!("account[0] is unsigned");
        Err(InstructionError::MissingRequiredSignature)?;
    }
    let storage_account_pubkey = *storage_account_pubkey.unwrap();

    let mut storage_account = StorageAccount::new(&mut me[0].account);
    let mut rest: Vec<_> = rest
        .iter_mut()
        .map(|keyed_account| StorageAccount::new(&mut keyed_account.account))
        .collect();

    match bincode::deserialize(data).map_err(|_| InstructionError::InvalidInstructionData)? {
        StorageInstruction::SubmitMiningProof {
            sha_state,
            entry_height,
            signature,
        } => {
            if num_keyed_accounts != 1 {
                Err(InstructionError::InvalidArgument)?;
            }
            storage_account.submit_mining_proof(
                storage_account_pubkey,
                sha_state,
                entry_height,
                signature,
            )
        }
        StorageInstruction::AdvertiseStorageRecentBlockhash { hash, entry_height } => {
            if num_keyed_accounts != 1 {
                // keyed_accounts[0] should be the main storage key
                // to access its data
                Err(InstructionError::InvalidArgument)?;
            }
            storage_account.advertise_storage_recent_blockhash(hash, entry_height)
        }
        StorageInstruction::ClaimStorageReward { entry_height } => {
            if num_keyed_accounts != 1 {
                // keyed_accounts[0] should be the main storage key
                // to access its data
                Err(InstructionError::InvalidArgument)?;
            }
            storage_account.claim_storage_reward(entry_height, tick_height)
        }
        StorageInstruction::ProofValidation {
            entry_height,
            proofs,
        } => {
            if num_keyed_accounts == 1 {
                // have to have at least 1 replicator to do any verification
                Err(InstructionError::InvalidArgument)?;
            }
            storage_account.proof_validation(entry_height, proofs, &mut rest)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id;
    use crate::storage_contract::{CheckedProof, Proof, ProofStatus, StorageContract};
    use crate::storage_instruction;
    use crate::ENTRIES_PER_SEGMENT;
    use bincode::deserialize;
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::account::{create_keyed_accounts, Account};
    use solana_sdk::client::SyncClient;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::{hash, Hash};
    use solana_sdk::instruction::Instruction;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
    use solana_sdk::system_instruction;

    fn test_instruction(
        ix: &Instruction,
        program_accounts: &mut [Account],
    ) -> Result<(), InstructionError> {
        let mut keyed_accounts: Vec<_> = ix
            .accounts
            .iter()
            .zip(program_accounts.iter_mut())
            .map(|(account_meta, account)| {
                KeyedAccount::new(&account_meta.pubkey, account_meta.is_signer, account)
            })
            .collect();

        let ret = process_instruction(&id(), &mut keyed_accounts, &ix.data, 42);
        info!("ret: {:?}", ret);
        ret
    }

    #[test]
    fn test_storage_tx() {
        let pubkey = Pubkey::new_rand();
        let mut accounts = [(pubkey, Account::default())];
        let mut keyed_accounts = create_keyed_accounts(&mut accounts);
        assert!(process_instruction(&id(), &mut keyed_accounts, &[], 42).is_err());
    }

    #[test]
    fn test_serialize_overflow() {
        let pubkey = Pubkey::new_rand();
        let mut keyed_accounts = Vec::new();
        let mut user_account = Account::default();
        keyed_accounts.push(KeyedAccount::new(&pubkey, true, &mut user_account));

        let ix = storage_instruction::advertise_recent_blockhash(
            &pubkey,
            Hash::default(),
            ENTRIES_PER_SEGMENT,
        );

        assert_eq!(
            process_instruction(&id(), &mut keyed_accounts, &ix.data, 42),
            Err(InstructionError::InvalidAccountData)
        );
    }

    #[test]
    fn test_invalid_accounts_len() {
        let pubkey = Pubkey::new_rand();
        let mut accounts = [Account::default()];

        let ix =
            storage_instruction::mining_proof(&pubkey, Hash::default(), 0, Signature::default());
        assert!(test_instruction(&ix, &mut accounts).is_err());

        let mut accounts = [Account::default(), Account::default(), Account::default()];

        assert!(test_instruction(&ix, &mut accounts).is_err());
    }

    #[test]
    fn test_submit_mining_invalid_entry_height() {
        solana_logger::setup();
        let pubkey = Pubkey::new_rand();
        let mut accounts = [Account::default(), Account::default()];
        accounts[1].data.resize(16 * 1024, 0);

        let ix =
            storage_instruction::mining_proof(&pubkey, Hash::default(), 0, Signature::default());

        // Haven't seen a transaction to roll over the epoch, so this should fail
        assert!(test_instruction(&ix, &mut accounts).is_err());
    }

    #[test]
    fn test_submit_mining_ok() {
        solana_logger::setup();
        let pubkey = Pubkey::new_rand();
        let mut accounts = [Account::default(), Account::default()];
        accounts[0].data.resize(16 * 1024, 0);

        let ix =
            storage_instruction::mining_proof(&pubkey, Hash::default(), 0, Signature::default());

        test_instruction(&ix, &mut accounts).unwrap();
    }

    #[test]
    #[ignore]
    fn test_validate_mining() {
        solana_logger::setup();
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let mint_pubkey = mint_keypair.pubkey();
        let replicator_keypair = Keypair::new();
        let replicator = replicator_keypair.pubkey();
        let validator_keypair = Keypair::new();
        let validator = validator_keypair.pubkey();

        let mut bank = Bank::new(&genesis_block);
        bank.add_instruction_processor(id(), process_instruction);
        let entry_height = 0;
        let bank_client = BankClient::new(bank);

        let ix = system_instruction::create_account(&mint_pubkey, &validator, 10, 4 * 1042, &id());
        bank_client.send_instruction(&mint_keypair, ix).unwrap();

        let ix = system_instruction::create_account(&mint_pubkey, &replicator, 10, 4 * 1042, &id());
        bank_client.send_instruction(&mint_keypair, ix).unwrap();

        let ix = storage_instruction::advertise_recent_blockhash(
            &validator,
            Hash::default(),
            ENTRIES_PER_SEGMENT,
        );

        bank_client
            .send_instruction(&validator_keypair, ix)
            .unwrap();

        let ix = storage_instruction::mining_proof(
            &replicator,
            Hash::default(),
            entry_height,
            Signature::default(),
        );
        bank_client
            .send_instruction(&replicator_keypair, ix)
            .unwrap();

        let ix = storage_instruction::advertise_recent_blockhash(
            &validator,
            Hash::default(),
            ENTRIES_PER_SEGMENT * 2,
        );
        bank_client
            .send_instruction(&validator_keypair, ix)
            .unwrap();

        let ix = storage_instruction::proof_validation(
            &validator,
            entry_height,
            vec![CheckedProof {
                proof: Proof {
                    id: replicator,
                    signature: Signature::default(),
                    sha_state: Hash::default(),
                },
                status: ProofStatus::Valid,
            }],
        );
        bank_client
            .send_instruction(&validator_keypair, ix)
            .unwrap();

        let ix = storage_instruction::advertise_recent_blockhash(
            &validator,
            Hash::default(),
            ENTRIES_PER_SEGMENT * 3,
        );
        bank_client
            .send_instruction(&validator_keypair, ix)
            .unwrap();

        let ix = storage_instruction::reward_claim(&validator, entry_height);
        bank_client
            .send_instruction(&validator_keypair, ix)
            .unwrap();

        // TODO enable when rewards are working
        // assert_eq!(bank_client.get_balance(&validator).unwrap(), TOTAL_VALIDATOR_REWARDS);

        // TODO extend BankClient with a method to force a block boundary
        // tick the bank into the next storage epoch so that rewards can be claimed
        //for _ in 0..=ENTRIES_PER_SEGMENT {
        //    bank.register_tick(&bank.last_blockhash());
        //}

        let ix = storage_instruction::reward_claim(&replicator, entry_height);
        bank_client
            .send_instruction(&replicator_keypair, ix)
            .unwrap();

        // TODO enable when rewards are working
        // assert_eq!(bank_client.get_balance(&replicator).unwrap(), TOTAL_REPLICATOR_REWARDS);
    }

    fn get_storage_entry_height<C: SyncClient>(client: &C, account: &Pubkey) -> u64 {
        match client.get_account_data(&account).unwrap() {
            Some(storage_system_account_data) => {
                let contract = deserialize(&storage_system_account_data);
                if let Ok(contract) = contract {
                    match contract {
                        StorageContract::ValidatorStorage { entry_height, .. } => {
                            return entry_height;
                        }
                        _ => info!("error in reading entry_height"),
                    }
                }
            }
            None => {
                info!("error in reading entry_height");
            }
        }
        0
    }

    fn get_storage_blockhash<C: SyncClient>(client: &C, account: &Pubkey) -> Hash {
        if let Some(storage_system_account_data) = client.get_account_data(&account).unwrap() {
            let contract = deserialize(&storage_system_account_data);
            if let Ok(contract) = contract {
                match contract {
                    StorageContract::ValidatorStorage { hash, .. } => {
                        return hash;
                    }
                    _ => (),
                }
            }
        }
        Hash::default()
    }

    #[test]
    fn test_bank_storage() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(1000);
        let mint_pubkey = mint_keypair.pubkey();
        let replicator_keypair = Keypair::new();
        let replicator_pubkey = replicator_keypair.pubkey();
        let validator_keypair = Keypair::new();
        let validator_pubkey = validator_keypair.pubkey();

        let mut bank = Bank::new(&genesis_block);
        bank.add_instruction_processor(id(), process_instruction);
        let bank_client = BankClient::new(bank);

        let x = 42;
        let x2 = x * 2;
        let storage_blockhash = hash(&[x2]);

        bank_client
            .transfer(10, &mint_keypair, &replicator_pubkey)
            .unwrap();

        let ix = system_instruction::create_account(
            &mint_pubkey,
            &replicator_pubkey,
            1,
            4 * 1024,
            &id(),
        );

        bank_client.send_instruction(&mint_keypair, ix).unwrap();

        let ix =
            system_instruction::create_account(&mint_pubkey, &validator_pubkey, 1, 4 * 1024, &id());

        bank_client.send_instruction(&mint_keypair, ix).unwrap();

        let ix = storage_instruction::advertise_recent_blockhash(
            &validator_pubkey,
            storage_blockhash,
            ENTRIES_PER_SEGMENT,
        );

        bank_client
            .send_instruction(&validator_keypair, ix)
            .unwrap();

        let entry_height = 0;
        let ix = storage_instruction::mining_proof(
            &replicator_pubkey,
            Hash::default(),
            entry_height,
            Signature::default(),
        );
        let _result = bank_client
            .send_instruction(&replicator_keypair, ix)
            .unwrap();

        assert_eq!(
            get_storage_entry_height(&bank_client, &validator_pubkey),
            ENTRIES_PER_SEGMENT
        );
        assert_eq!(
            get_storage_blockhash(&bank_client, &validator_pubkey),
            storage_blockhash
        );
    }
}
