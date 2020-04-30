use assert_matches::assert_matches;
use bincode::deserialize;
use log::*;
use solana_runtime::{
    bank::Bank,
    bank_client::BankClient,
    genesis_utils::{create_genesis_config, GenesisConfigInfo},
};
use solana_sdk::{
    account_utils::StateMut,
    client::SyncClient,
    clock::{get_segment_from_slot, DEFAULT_SLOTS_PER_SEGMENT, DEFAULT_TICKS_PER_SLOT},
    hash::{hash, Hash},
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    system_instruction,
    sysvar::{
        rewards::{self, Rewards},
        Sysvar,
    },
};
use solana_storage_program::{
    id,
    storage_contract::{ProofStatus, StorageContract},
    storage_instruction::{self, StorageAccountType},
    storage_processor::process_instruction,
};
use std::{collections::HashMap, sync::Arc};

const TICKS_IN_SEGMENT: u64 = DEFAULT_SLOTS_PER_SEGMENT * DEFAULT_TICKS_PER_SLOT;

#[test]
fn test_account_owner() {
    let account_owner = Pubkey::new_rand();
    let validator_storage_keypair = Keypair::new();
    let validator_storage_pubkey = validator_storage_keypair.pubkey();
    let archiver_storage_keypair = Keypair::new();
    let archiver_storage_pubkey = archiver_storage_keypair.pubkey();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(1000);
    let mut bank = Bank::new(&genesis_config);
    let mint_pubkey = mint_keypair.pubkey();
    bank.add_static_program("storage_program", id(), process_instruction);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let message = Message::new(&storage_instruction::create_storage_account(
        &mint_pubkey,
        &account_owner,
        &validator_storage_pubkey,
        1,
        StorageAccountType::Validator,
    ));
    bank_client
        .send_message(&[&mint_keypair, &validator_storage_keypair], message)
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

    let message = Message::new(&storage_instruction::create_storage_account(
        &mint_pubkey,
        &account_owner,
        &archiver_storage_pubkey,
        1,
        StorageAccountType::Archiver,
    ));
    bank_client
        .send_message(&[&mint_keypair, &archiver_storage_keypair], message)
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
fn test_validate_mining() {
    solana_logger::setup();
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(100_000_000_000);
    genesis_config
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

    let bank = Bank::new(&genesis_config);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    init_storage_accounts(
        &owner_pubkey,
        &bank_client,
        &mint_keypair,
        &[&validator_storage_keypair],
        &[&archiver_1_storage_keypair, &archiver_2_storage_keypair],
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
        &[storage_instruction::advertise_recent_blockhash(
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
        &[storage_instruction::advertise_recent_blockhash(
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
        &[storage_instruction::proof_validation(
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
        &[storage_instruction::advertise_recent_blockhash(
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
        &[storage_instruction::claim_reward(
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
        &[storage_instruction::claim_reward(
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
        &[storage_instruction::claim_reward(
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
    validator_accounts_to_create: &[&Keypair],
    archiver_accounts_to_create: &[&Keypair],
    lamports: u64,
) {
    let mut signers = vec![mint];
    let mut ixs: Vec<_> = vec![system_instruction::transfer(&mint.pubkey(), owner, 1)];
    ixs.append(
        &mut validator_accounts_to_create
            .into_iter()
            .flat_map(|account| {
                signers.push(&account);
                storage_instruction::create_storage_account(
                    &mint.pubkey(),
                    owner,
                    &account.pubkey(),
                    lamports,
                    StorageAccountType::Validator,
                )
            })
            .collect(),
    );
    archiver_accounts_to_create.into_iter().for_each(|account| {
        signers.push(&account);
        ixs.append(&mut storage_instruction::create_storage_account(
            &mint.pubkey(),
            owner,
            &account.pubkey(),
            lamports,
            StorageAccountType::Archiver,
        ))
    });
    let message = Message::new(&ixs);
    client.send_message(&signers, message).unwrap();
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
        &[storage_instruction::mining_proof(
            &storage_keypair.pubkey(),
            sha_state,
            segment_index,
            Signature::default(),
            bank_client.get_recent_blockhash().unwrap().0,
        )],
        Some(&mint_keypair.pubkey()),
    );

    assert_matches!(
        bank_client.send_message(&[mint_keypair, storage_keypair], message),
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
    let GenesisConfigInfo {
        mut genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(1000);
    genesis_config
        .native_instruction_processors
        .push(solana_storage_program::solana_storage_program!());
    let mint_pubkey = mint_keypair.pubkey();
    let archiver_keypair = Keypair::new();
    let archiver_pubkey = archiver_keypair.pubkey();
    let validator_keypair = Keypair::new();
    let validator_pubkey = validator_keypair.pubkey();

    let bank = Bank::new(&genesis_config);
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

    let message = Message::new(&storage_instruction::create_storage_account(
        &mint_pubkey,
        &Pubkey::default(),
        &archiver_pubkey,
        11,
        StorageAccountType::Archiver,
    ));
    bank_client
        .send_message(&[&mint_keypair, &archiver_keypair], message)
        .unwrap();

    let message = Message::new(&storage_instruction::create_storage_account(
        &mint_pubkey,
        &Pubkey::default(),
        &validator_pubkey,
        1,
        StorageAccountType::Validator,
    ));
    bank_client
        .send_message(&[&mint_keypair, &validator_keypair], message)
        .unwrap();

    let message = Message::new_with_payer(
        &[storage_instruction::advertise_recent_blockhash(
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
        &[storage_instruction::mining_proof(
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
