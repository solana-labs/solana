use bincode::deserialize;
use log::*;
use solana_runtime::bank::Bank;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
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

fn get_storage_block_hash(bank: &Bank, account: Pubkey) -> Hash {
    if let Some(storage_system_account) = bank.get_account(&account) {
        let state = deserialize(&storage_system_account.userdata);
        if let Ok(state) = state {
            let state: storage_program::StorageProgramState = state;
            return state.hash;
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
    let block_hash = genesis_block.hash();
    let x2 = x * 2;
    let storage_block_hash = hash(&[x2]);

    bank.register_tick(&block_hash);

    bank.transfer(10, &alice, jill.pubkey(), block_hash)
        .unwrap();

    bank.transfer(10, &alice, bob.pubkey(), block_hash).unwrap();
    bank.transfer(10, &alice, jack.pubkey(), block_hash)
        .unwrap();

    let tx = SystemTransaction::new_program_account(
        &alice,
        bob.pubkey(),
        block_hash,
        1,
        4 * 1024,
        storage_program::id(),
        0,
    );

    bank.process_transaction(&tx).unwrap();

    let tx = StorageTransaction::new_advertise_recent_block_hash(
        &bob,
        storage_block_hash,
        block_hash,
        ENTRIES_PER_SEGMENT,
    );

    bank.process_transaction(&tx).unwrap();

    // TODO: This triggers a ProgramError(0, InvalidArgument). Why?
    // Oddly, the assertions below it succeed without running this code.
    //let entry_height = 0;
    //let tx = StorageTransaction::new_mining_proof(
    //    &jack,
    //    Hash::default(),
    //    block_hash,
    //    entry_height,
    //    Signature::default(),
    //);
    //let _result = bank.process_transaction(&tx).unwrap();

    assert_eq!(
        get_storage_entry_height(&bank, bob.pubkey()),
        ENTRIES_PER_SEGMENT
    );
    assert_eq!(
        get_storage_block_hash(&bank, bob.pubkey()),
        storage_block_hash
    );
}
