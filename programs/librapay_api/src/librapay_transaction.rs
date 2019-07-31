use crate::librapay_instruction;
use language_e2e_tests::account::AccountResource;
use log::*;
use solana_move_loader_api::account_state::{pubkey_to_address, LibraAccountState};
use solana_move_loader_api::data_store::DataStore;
use solana_sdk::client::Client;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction;
use solana_sdk::transaction::Transaction;
use std::boxed::Box;
use std::error;
use std::fmt;

pub fn create_genesis(
    genesis_keypair: &Keypair,
    microlibras: u64,
    recent_blockhash: Hash,
) -> Transaction {
    let ix = librapay_instruction::genesis(&genesis_keypair.pubkey(), microlibras);
    Transaction::new_signed_with_payer(
        vec![ix],
        Some(&genesis_keypair.pubkey()),
        &[genesis_keypair],
        recent_blockhash,
    )
}

pub fn mint_tokens(
    program_id: &Pubkey,
    payer: &Keypair,
    mint: &Keypair,
    to: &Pubkey,
    microlibras: u64,
    recent_blockhash: Hash,
) -> Transaction {
    let ix = librapay_instruction::mint(program_id, &mint.pubkey(), to, microlibras);
    Transaction::new_signed_with_payer(
        vec![ix],
        Some(&payer.pubkey()),
        &[payer, mint],
        recent_blockhash,
    )
}

pub fn transfer(
    program_id: &Pubkey,
    mint: &Pubkey,
    payer: &Keypair,
    from: &Keypair,
    to: &Pubkey,
    microlibras: u64,
    recent_blockhash: Hash,
) -> Transaction {
    let ix = librapay_instruction::transfer(program_id, mint, &from.pubkey(), to, microlibras);
    Transaction::new_signed_with_payer(
        vec![ix],
        Some(&payer.pubkey()),
        &[payer, from],
        recent_blockhash,
    )
}

pub fn create_accounts(
    from: &Keypair,
    tos: &[Pubkey],
    lamports: u64,
    recent_blockhash: Hash,
) -> Transaction {
    let instructions = tos
        .iter()
        .map(|to| {
            system_instruction::create_account(
                &from.pubkey(),
                to,
                lamports,
                128,
                &solana_move_loader_api::id(),
            )
        })
        .collect();
    Transaction::new_signed_instructions(&[from], instructions, recent_blockhash)
}

pub fn create_account(
    from: &Keypair,
    to: &Pubkey,
    lamports: u64,
    recent_blockhash: Hash,
) -> Transaction {
    create_accounts(from, &[*to], lamports, recent_blockhash)
}

#[derive(Debug)]
enum LibrapayError {
    UnknownAccountState,
}

impl fmt::Display for LibrapayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl error::Error for LibrapayError {}

pub fn get_libra_balance<T: Client>(
    client: &T,
    account_address: &Pubkey,
) -> Result<u64, Box<dyn error::Error>> {
    let account = client.get_account_data(&account_address)?;
    if let Some(account) = account {
        let mut data_store = DataStore::default();
        match bincode::deserialize(&account)? {
            LibraAccountState::User(_, write_set) => {
                data_store.apply_write_set(&write_set);
            }
            LibraAccountState::Unallocated => {
                return Ok(0);
            }
            state => {
                info!("Unknown account state: {:?}", state);
                return Err(LibrapayError::UnknownAccountState)?;
            }
        }
        let resource = data_store
            .read_account_resource(&pubkey_to_address(account_address))
            .unwrap();

        let res = AccountResource::read_balance(&resource);
        Ok(res)
    } else {
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{create_genesis, upload_mint_program, upload_payment_program};
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::genesis_block::create_genesis_block;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::sync::Arc;

    fn create_bank(lamports: u64) -> (Arc<Bank>, Keypair, Keypair, Pubkey, Pubkey) {
        let (genesis_block, mint_keypair) = create_genesis_block(lamports);
        let mut bank = Bank::new(&genesis_block);
        bank.add_instruction_processor(
            solana_move_loader_api::id(),
            solana_move_loader_api::processor::process_instruction,
        );
        let shared_bank = Arc::new(bank);
        let bank_client = BankClient::new_shared(&shared_bank);
        let genesis_keypair = create_genesis(&mint_keypair, &bank_client, 1_000_000);
        let mint_program_pubkey = upload_mint_program(&mint_keypair, &bank_client);
        let program_pubkey = upload_payment_program(&mint_keypair, &bank_client);
        (
            shared_bank,
            mint_keypair,
            genesis_keypair,
            mint_program_pubkey,
            program_pubkey,
        )
    }

    #[test]
    fn test_transfer() {
        let (bank, mint_keypair, libra_genesis_keypair, mint_program_id, program_id) =
            create_bank(10_000);
        let from = Keypair::new();
        let to = Keypair::new();

        let tx = create_accounts(
            &mint_keypair,
            &[from.pubkey(), to.pubkey()],
            1,
            bank.last_blockhash(),
        );
        bank.process_transaction(&tx).unwrap();

        info!(
            "created accounts: mint: {} libra_mint: {}",
            mint_keypair.pubkey(),
            libra_genesis_keypair.pubkey()
        );
        info!("    from: {} to: {}", from.pubkey(), to.pubkey());

        let tx = mint_tokens(
            &mint_program_id,
            &mint_keypair,
            &libra_genesis_keypair,
            &from.pubkey(),
            1,
            bank.last_blockhash(),
        );
        bank.process_transaction(&tx).unwrap();
        let client = BankClient::new_shared(&bank);
        assert_eq!(1, get_libra_balance(&client, &from.pubkey()).unwrap());

        info!("passed mint... doing another transfer..");

        let tx = transfer(
            &program_id,
            &libra_genesis_keypair.pubkey(),
            &mint_keypair,
            &from,
            &to.pubkey(),
            1,
            bank.last_blockhash(),
        );
        bank.process_transaction(&tx).unwrap();
        assert_eq!(1, get_libra_balance(&client, &to.pubkey()).unwrap());
    }
}
