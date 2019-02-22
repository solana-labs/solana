use bincode::deserialize;
use log::info;
use solana_runtime::bank::Bank;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::storage_program;
use solana_sdk::storage_program::{StorageTransaction, ENTRIES_PER_SEGMENT};
use solana_sdk::system_transaction::SystemTransaction;

fn get_storage_entry_height(bank: &Bank, account: Pubkey) -> u64 {
    match bank.get_account(&account) {
        Some(storage_system_account) => {
            let state = deserialize(&storage_system_account.userdata);
            if let Ok(state) = state {
                let state: storage_program::StorageProgramState = state;
                return state.entry_height;
            }
        }
        None => {
            info!("error in reading entry_height");
        }
    }
    0
}

fn get_storage_last_id(bank: &Bank, account: Pubkey) -> Hash {
    if let Some(storage_system_account) = bank.get_account(&account) {
        let state = deserialize(&storage_system_account.userdata);
        if let Ok(state) = state {
            let state: storage_program::StorageProgramState = state;
            return state.id;
        }
    }
    Hash::default()
}

#[test]
fn test_bank_storage() {
    let (genesis_block, alice) = GenesisBlock::new(1000);
    let bank = Bank::new(&genesis_block);

    let bob = Keypair::new();
    let jack = Keypair::new();
    let jill = Keypair::new();

    let x = 42;
    let last_id = hash(&[x]);
    let x2 = x * 2;
    let storage_last_id = hash(&[x2]);

    bank.register_tick(&last_id);

    bank.transfer(10, &alice, jill.pubkey(), last_id).unwrap();

    bank.transfer(10, &alice, bob.pubkey(), last_id).unwrap();
    bank.transfer(10, &alice, jack.pubkey(), last_id).unwrap();

    let tx = SystemTransaction::new_program_account(
        &alice,
        bob.pubkey(),
        last_id,
        1,
        4 * 1024,
        storage_program::id(),
        0,
    );

    bank.process_transaction(&tx).unwrap();

    let tx = StorageTransaction::new_advertise_last_id(
        &bob,
        storage_last_id,
        last_id,
        ENTRIES_PER_SEGMENT,
    );

    bank.process_transaction(&tx).unwrap();

    let entry_height = 0;

    let tx = StorageTransaction::new_mining_proof(
        &jack,
        Hash::default(),
        last_id,
        entry_height,
        Signature::default(),
    );

    bank.process_transaction(&tx).unwrap();

    assert_eq!(
        get_storage_entry_height(&bank, bob.pubkey()),
        ENTRIES_PER_SEGMENT
    );
    assert_eq!(get_storage_last_id(&bank, bob.pubkey()), storage_last_id);
}
