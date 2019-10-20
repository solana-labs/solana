use assert_matches::assert_matches;
use bincode::deserialize;
use log::*;
use solana_runtime::{
    bank::Bank,
    bank_client::BankClient,
    genesis_utils::{create_genesis_block, GenesisBlockInfo},
};
use solana_sdk::{
    account::{create_keyed_accounts, Account, KeyedAccount},
    account_utils::State,
    client::SyncClient,
    clock::{get_segment_from_slot, DEFAULT_SLOTS_PER_SEGMENT, DEFAULT_TICKS_PER_SLOT},
    hash::{hash, Hash},
    instruction::{Instruction, InstructionError},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil, Signature},
    system_instruction,
    sysvar::clock::{self, Clock},
    sysvar::rewards::{self, Rewards},
};
use solana_storage_api::{
    id,
    storage_contract::StorageAccount,
    storage_contract::{ProofStatus, StorageContract, STORAGE_ACCOUNT_SPACE},
    storage_instruction::{self, StorageAccountType},
    storage_processor::process_instruction,
};
use std::collections::HashMap;
use std::sync::Arc;

const TICKS_IN_SEGMENT: u64 = DEFAULT_SLOTS_PER_SEGMENT * DEFAULT_TICKS_PER_SLOT;

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

    let ret = process_instruction(&id(), &mut keyed_accounts, &ix.data);
    info!("ret: {:?}", ret);
    ret
}

#[test]
fn test_account_owner() {
    let account_owner = Pubkey::new_rand();
    let validator_storage_pubkey = Pubkey::new_rand();
    let archiver_storage_pubkey = Pubkey::new_rand();

    let GenesisBlockInfo {
        genesis_block,
        mint_keypair,
        ..
    } = create_genesis_block(1000);
    let mut bank = Bank::new(&genesis_block);
    let mint_pubkey = mint_keypair.pubkey();
    bank.add_instruction_processor(id(), process_instruction);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let message = Message::new(storage_instruction::create_storage_account(
        &mint_pubkey,
        &account_owner,
        &validator_storage_pubkey,
        1,
        StorageAccountType::Validator,
    ));
    bank_client
        .send_message(&[&mint_keypair], message)
        .expect("failed to create account");
    let account = bank
        .get_account(&validator_storage_pubkey)
        .expect("account not found");
    let storage_contract = account.state().expect("couldn't unpack account data");
    if let StorageContract::ValidatorStorage { owner, .. } = storage_contract {
        assert_eq!(owner, account_owner);
    } else {
        assert!(false, "wrong account type found")
    }

    let message = Message::new(storage_instruction::create_storage_account(
        &mint_pubkey,
        &account_owner,
        &archiver_storage_pubkey,
        1,
        StorageAccountType::Archiver,
    ));
    bank_client
        .send_message(&[&mint_keypair], message)
        .expect("failed to create account");
    let account = bank
        .get_account(&archiver_storage_pubkey)
        .expect("account not found");
    let storage_contract = account.state().expect("couldn't unpack account data");
    if let StorageContract::ArchiverStorage { owner, .. } = storage_contract {
        assert_eq!(owner, account_owner);
    } else {
        assert!(false, "wrong account type found")
    }
}

#[test]
fn test_proof_bounds() {
    let account_owner = Pubkey::new_rand();
    let pubkey = Pubkey::new_rand();
    let mut account = Account {
        data: vec![0; STORAGE_ACCOUNT_SPACE as usize],
        ..Account::default()
    };
    {
        let mut storage_account = StorageAccount::new(pubkey, &mut account);
        storage_account
            .initialize_storage(account_owner, StorageAccountType::Archiver)
            .unwrap();
    }

    let ix = storage_instruction::mining_proof(
        &pubkey,
        Hash::default(),
        0,
        Signature::default(),
        Hash::default(),
    );
    // the proof is for segment 0, need to move the slot into segment 2
    let mut clock_account = clock::new_account(1, 0, 0, 0, 0);
    Clock::to_account(
        &Clock {
            slot: DEFAULT_SLOTS_PER_SEGMENT * 2,
            segment: 2,
            epoch: 0,
            stakers_epoch: 0,
        },
        &mut clock_account,
    );

    assert_eq!(test_instruction(&ix, &mut [account, clock_account]), Ok(()));
}

#[test]
fn test_storage_tx() {
    let pubkey = Pubkey::new_rand();
    let mut accounts = [(pubkey, Account::default())];
    let mut keyed_accounts = create_keyed_accounts(&mut accounts);
    assert!(process_instruction(&id(), &mut keyed_accounts, &[]).is_err());
}

#[test]
fn test_serialize_overflow() {
    let pubkey = Pubkey::new_rand();
    let clock_id = clock::id();
    let mut keyed_accounts = Vec::new();
    let mut user_account = Account::default();
    let mut clock_account = clock::new_account(1, 0, 0, 0, 0);
    keyed_accounts.push(KeyedAccount::new(&pubkey, true, &mut user_account));
    keyed_accounts.push(KeyedAccount::new(&clock_id, false, &mut clock_account));

    let ix = storage_instruction::advertise_recent_blockhash(&pubkey, Hash::default(), 1);

    assert_eq!(
        process_instruction(&id(), &mut keyed_accounts, &ix.data),
        Err(InstructionError::InvalidAccountData)
    );
}

#[test]
fn test_invalid_accounts_len() {
    let pubkey = Pubkey::new_rand();
    let mut accounts = [Account::default()];

    let ix = storage_instruction::mining_proof(
        &pubkey,
        Hash::default(),
        0,
        Signature::default(),
        Hash::default(),
    );
    // move tick height into segment 1
    let mut clock_account = clock::new_account(1, 0, 0, 0, 0);
    Clock::to_account(
        &Clock {
            slot: 16,
            segment: 1,
            epoch: 0,
            stakers_epoch: 0,
        },
        &mut clock_account,
    );

    assert!(test_instruction(&ix, &mut accounts).is_err());

    let mut accounts = [Account::default(), clock_account, Account::default()];

    assert!(test_instruction(&ix, &mut accounts).is_err());
}

#[test]
fn test_submit_mining_invalid_slot() {
    solana_logger::setup();
    let pubkey = Pubkey::new_rand();
    let mut accounts = [Account::default(), Account::default()];
    accounts[0].data.resize(STORAGE_ACCOUNT_SPACE as usize, 0);
    accounts[1].data.resize(STORAGE_ACCOUNT_SPACE as usize, 0);

    let ix = storage_instruction::mining_proof(
        &pubkey,
        Hash::default(),
        0,
        Signature::default(),
        Hash::default(),
    );

    // submitting a proof for a slot in the past, so this should fail
    assert!(test_instruction(&ix, &mut accounts).is_err());
}

#[test]
fn test_submit_mining_ok() {
    solana_logger::setup();
    let account_owner = Pubkey::new_rand();
    let pubkey = Pubkey::new_rand();
    let mut account = Account::default();
    account.data.resize(STORAGE_ACCOUNT_SPACE as usize, 0);
    {
        let mut storage_account = StorageAccount::new(pubkey, &mut account);
        storage_account
            .initialize_storage(account_owner, StorageAccountType::Archiver)
            .unwrap();
    }

    let ix = storage_instruction::mining_proof(
        &pubkey,
        Hash::default(),
        0,
        Signature::default(),
        Hash::default(),
    );
    // move slot into segment 1
    let mut clock_account = clock::new_account(1, 0, 0, 0, 0);
    Clock::to_account(
        &Clock {
            slot: DEFAULT_SLOTS_PER_SEGMENT,
            segment: 1,
            epoch: 0,
            stakers_epoch: 0,
        },
        &mut clock_account,
    );

    assert_matches!(test_instruction(&ix, &mut [account, clock_account]), Ok(_));
}

#[test]
fn test_validate_mining() {
    solana_logger::setup();
    let GenesisBlockInfo {
        mut genesis_block,
        mint_keypair,
        ..
    } = create_genesis_block(100_000_000_000);
    genesis_block
        .native_instruction_processors
        .push(solana_storage_program::solana_storage_program!());
    let mint_pubkey = mint_keypair.pubkey();
    // 1 owner for all archiver and validator accounts for the test
    let owner_pubkey = Pubkey::new_rand();

    let archiver_1_storage_keypair = Keypair::new();
    let archiver_1_storage_id = archiver_1_storage_keypair.pubkey();

    let archiver_2_storage_keypair = Keypair::new();
    let archiver_2_storage_id = archiver_2_storage_keypair.pubkey();

    let validator_storage_keypair = Keypair::new();
    let validator_storage_id = validator_storage_keypair.pubkey();

    let bank = Bank::new(&genesis_block);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    init_storage_accounts(
        &owner_pubkey,
        &bank_client,
        &mint_keypair,
        &[&validator_storage_id],
        &[&archiver_1_storage_id, &archiver_2_storage_id],
        10,
    );

    // create a new bank in segment 2
    let bank = Arc::new(Bank::new_from_parent(
        &bank,
        &Pubkey::default(),
        DEFAULT_SLOTS_PER_SEGMENT * 2,
    ));
    let bank_client = BankClient::new_shared(&bank);

    // advertise for storage segment 1
    let message = Message::new_with_payer(
        vec![storage_instruction::advertise_recent_blockhash(
            &validator_storage_id,
            Hash::default(),
            1,
        )],
        Some(&mint_pubkey),
    );
    assert_matches!(
        bank_client.send_message(&[&mint_keypair, &validator_storage_keypair], message),
        Ok(_)
    );

    // submit proofs 5 proofs for each archiver for segment 0
    let mut checked_proofs: HashMap<_, Vec<_>> = HashMap::new();
    for _ in 0..5 {
        checked_proofs
            .entry(archiver_1_storage_id)
            .or_default()
            .push(submit_proof(
                &mint_keypair,
                &archiver_1_storage_keypair,
                &bank_client,
                0,
            ));
        checked_proofs
            .entry(archiver_2_storage_id)
            .or_default()
            .push(submit_proof(
                &mint_keypair,
                &archiver_2_storage_keypair,
                &bank_client,
                0,
            ));
    }
    let message = Message::new_with_payer(
        vec![storage_instruction::advertise_recent_blockhash(
            &validator_storage_id,
            Hash::default(),
            2,
        )],
        Some(&mint_pubkey),
    );

    // move banks into the next segment
    let proof_segment = get_segment_from_slot(bank.slot(), bank.slots_per_segment());
    let bank = Arc::new(Bank::new_from_parent(
        &bank,
        &Pubkey::default(),
        DEFAULT_SLOTS_PER_SEGMENT + bank.slot(),
    ));
    let bank_client = BankClient::new_shared(&bank);

    assert_matches!(
        bank_client.send_message(&[&mint_keypair, &validator_storage_keypair], message),
        Ok(_)
    );

    let message = Message::new_with_payer(
        vec![storage_instruction::proof_validation(
            &validator_storage_id,
            proof_segment as u64,
            checked_proofs.into_iter().map(|entry| entry).collect(),
        )],
        Some(&mint_pubkey),
    );

    assert_matches!(
        bank_client.send_message(&[&mint_keypair, &validator_storage_keypair], message),
        Ok(_)
    );

    let message = Message::new_with_payer(
        vec![storage_instruction::advertise_recent_blockhash(
            &validator_storage_id,
            Hash::default(),
            3,
        )],
        Some(&mint_pubkey),
    );

    // move banks into the next segment
    let bank = Arc::new(Bank::new_from_parent(
        &bank,
        &Pubkey::default(),
        DEFAULT_SLOTS_PER_SEGMENT + bank.slot(),
    ));
    let bank_client = BankClient::new_shared(&bank);

    assert_matches!(
        bank_client.send_message(&[&mint_keypair, &validator_storage_keypair], message),
        Ok(_)
    );

    assert_eq!(bank_client.get_balance(&validator_storage_id).unwrap(), 10);

    let bank = Arc::new(Bank::new_from_parent(
        &bank,
        &Pubkey::default(),
        bank.slot() + bank.epoch_schedule().slots_per_epoch,
    ));
    let bank_client = BankClient::new_shared(&bank);

    let rewards = bank
        .get_account(&rewards::id())
        .map(|account| Rewards::from_account(&account).unwrap())
        .unwrap();
    let message = Message::new_with_payer(
        vec![storage_instruction::claim_reward(
            &owner_pubkey,
            &validator_storage_id,
        )],
        Some(&mint_pubkey),
    );
    assert_matches!(bank_client.send_message(&[&mint_keypair], message), Ok(_));
    assert_eq!(
        bank_client.get_balance(&owner_pubkey).unwrap(),
        1 + ((rewards.storage_point_value * 10_f64) as u64)
    );

    // tick the bank into the next storage epoch so that rewards can be claimed
    for _ in 0..=TICKS_IN_SEGMENT {
        bank.register_tick(&bank.last_blockhash());
    }

    assert_eq!(bank_client.get_balance(&archiver_1_storage_id).unwrap(), 10);

    let message = Message::new_with_payer(
        vec![storage_instruction::claim_reward(
            &owner_pubkey,
            &archiver_1_storage_id,
        )],
        Some(&mint_pubkey),
    );
    assert_matches!(bank_client.send_message(&[&mint_keypair], message), Ok(_));
    assert_eq!(
        bank_client.get_balance(&owner_pubkey).unwrap(),
        1 + ((rewards.storage_point_value * 10_f64) as u64)
            + (rewards.storage_point_value * 5_f64) as u64
    );

    let message = Message::new_with_payer(
        vec![storage_instruction::claim_reward(
            &owner_pubkey,
            &archiver_2_storage_id,
        )],
        Some(&mint_pubkey),
    );
    assert_matches!(bank_client.send_message(&[&mint_keypair], message), Ok(_));
    assert_eq!(
        bank_client.get_balance(&owner_pubkey).unwrap(),
        1 + (rewards.storage_point_value * 10_f64) as u64
            + (rewards.storage_point_value * 5_f64) as u64
            + (rewards.storage_point_value * 5_f64) as u64
    );
}

fn init_storage_accounts(
    owner: &Pubkey,
    client: &BankClient,
    mint: &Keypair,
    validator_accounts_to_create: &[&Pubkey],
    archiver_accounts_to_create: &[&Pubkey],
    lamports: u64,
) {
    let mut ixs: Vec<_> = vec![system_instruction::transfer_now(&mint.pubkey(), owner, 1)];
    ixs.append(
        &mut validator_accounts_to_create
            .into_iter()
            .flat_map(|account| {
                storage_instruction::create_storage_account(
                    &mint.pubkey(),
                    owner,
                    account,
                    lamports,
                    StorageAccountType::Validator,
                )
            })
            .collect(),
    );
    archiver_accounts_to_create.into_iter().for_each(|account| {
        ixs.append(&mut storage_instruction::create_storage_account(
            &mint.pubkey(),
            owner,
            account,
            lamports,
            StorageAccountType::Archiver,
        ))
    });
    let message = Message::new(ixs);
    client.send_message(&[mint], message).unwrap();
}

fn get_storage_segment<C: SyncClient>(client: &C, account: &Pubkey) -> u64 {
    match client.get_account_data(&account).unwrap() {
        Some(storage_system_account_data) => {
            let contract = deserialize(&storage_system_account_data);
            if let Ok(contract) = contract {
                match contract {
                    StorageContract::ValidatorStorage { segment, .. } => {
                        return segment;
                    }
                    _ => info!("error in reading segment"),
                }
            }
        }
        None => {
            info!("error in reading segment");
        }
    }
    0
}

fn submit_proof(
    mint_keypair: &Keypair,
    storage_keypair: &Keypair,
    bank_client: &BankClient,
    segment_index: u64,
) -> ProofStatus {
    let sha_state = Hash::new(Pubkey::new_rand().as_ref());
    let message = Message::new_with_payer(
        vec![storage_instruction::mining_proof(
            &storage_keypair.pubkey(),
            sha_state,
            segment_index,
            Signature::default(),
            bank_client.get_recent_blockhash().unwrap().0,
        )],
        Some(&mint_keypair.pubkey()),
    );

    assert_matches!(
        bank_client.send_message(&[&mint_keypair, &storage_keypair], message),
        Ok(_)
    );
    ProofStatus::Valid
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
    let GenesisBlockInfo {
        mut genesis_block,
        mint_keypair,
        ..
    } = create_genesis_block(1000);
    genesis_block
        .native_instruction_processors
        .push(solana_storage_program::solana_storage_program!());
    let mint_pubkey = mint_keypair.pubkey();
    let archiver_keypair = Keypair::new();
    let archiver_pubkey = archiver_keypair.pubkey();
    let validator_keypair = Keypair::new();
    let validator_pubkey = validator_keypair.pubkey();

    let bank = Bank::new(&genesis_block);
    // tick the bank up until it's moved into storage segment 2
    // create a new bank in storage segment 2
    let bank = Bank::new_from_parent(
        &Arc::new(bank),
        &Pubkey::new_rand(),
        DEFAULT_SLOTS_PER_SEGMENT * 2,
    );
    let bank_client = BankClient::new(bank);

    let x = 42;
    let x2 = x * 2;
    let storage_blockhash = hash(&[x2]);

    let message = Message::new(storage_instruction::create_storage_account(
        &mint_pubkey,
        &Pubkey::default(),
        &archiver_pubkey,
        11,
        StorageAccountType::Archiver,
    ));
    bank_client.send_message(&[&mint_keypair], message).unwrap();

    let message = Message::new(storage_instruction::create_storage_account(
        &mint_pubkey,
        &Pubkey::default(),
        &validator_pubkey,
        1,
        StorageAccountType::Validator,
    ));
    bank_client.send_message(&[&mint_keypair], message).unwrap();

    let message = Message::new_with_payer(
        vec![storage_instruction::advertise_recent_blockhash(
            &validator_pubkey,
            storage_blockhash,
            1,
        )],
        Some(&mint_pubkey),
    );

    assert_matches!(
        bank_client.send_message(&[&mint_keypair, &validator_keypair], message),
        Ok(_)
    );

    let slot = 0;
    let message = Message::new_with_payer(
        vec![storage_instruction::mining_proof(
            &archiver_pubkey,
            Hash::default(),
            slot,
            Signature::default(),
            bank_client.get_recent_blockhash().unwrap().0,
        )],
        Some(&mint_pubkey),
    );
    assert_matches!(
        bank_client.send_message(&[&mint_keypair, &archiver_keypair], message),
        Ok(_)
    );

    assert_eq!(get_storage_segment(&bank_client, &validator_pubkey), 1);
    assert_eq!(
        get_storage_blockhash(&bank_client, &validator_pubkey),
        storage_blockhash
    );
}
