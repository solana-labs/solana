use crate::librapay_instruction;
use log::*;
use solana_move_loader_api::account_state::{pubkey_to_address, LibraAccountState};
use solana_move_loader_api::data_store::DataStore;
use solana_sdk::client::Client;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction;
use solana_sdk::transaction::Transaction;
use std::boxed::Box;
use std::error;
use std::fmt;

pub fn create_genesis(genesis: &Keypair, microlibras: u64, recent_blockhash: Hash) -> Transaction {
    let ix = librapay_instruction::genesis(&genesis.pubkey(), microlibras);
    Transaction::new_signed_with_payer(
        vec![ix],
        Some(&genesis.pubkey()),
        &[genesis],
        recent_blockhash,
    )
}

pub fn mint_tokens(
    script: &Pubkey,
    payer: &Keypair,
    genesis: &Keypair,
    to: &Pubkey,
    microlibras: u64,
    recent_blockhash: Hash,
) -> Transaction {
    let ix = librapay_instruction::mint(script, &genesis.pubkey(), to, microlibras);
    Transaction::new_signed_with_payer(
        vec![ix],
        Some(&payer.pubkey()),
        &[payer, genesis],
        recent_blockhash,
    )
}

pub fn transfer(
    script: &Pubkey,
    genesis: &Pubkey,
    payer: &Keypair,
    from: &Keypair,
    to: &Pubkey,
    microlibras: u64,
    recent_blockhash: Hash,
) -> Transaction {
    let ix = librapay_instruction::transfer(script, genesis, &from.pubkey(), to, microlibras);
    Transaction::new_signed_with_payer(
        vec![ix],
        Some(&payer.pubkey()),
        &[payer, from],
        recent_blockhash,
    )
}

pub fn create_accounts(
    from: &Keypair,
    to: &[&Keypair],
    lamports: u64,
    recent_blockhash: Hash,
) -> Transaction {
    let instructions = to
        .iter()
        .map(|to| {
            system_instruction::create_account(
                &from.pubkey(),
                &to.pubkey(),
                lamports,
                400,
                &solana_sdk::move_loader::id(),
            )
        })
        .collect();

    let mut from_signers = vec![from];
    from_signers.extend_from_slice(to);
    Transaction::new_signed_instructions(&from_signers, instructions, recent_blockhash)
}

pub fn create_account(
    from: &Keypair,
    to: &Keypair,
    lamports: u64,
    recent_blockhash: Hash,
) -> Transaction {
    create_accounts(from, &[to], lamports, recent_blockhash)
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
    if let Some(account) =
        client.get_account_with_commitment(&account_address, CommitmentConfig::recent())?
    {
        let mut data_store = DataStore::default();
        match bincode::deserialize(&account.data)? {
            LibraAccountState::User(_, write_set) => {
                data_store.apply_write_set(&write_set);
            }
            LibraAccountState::Unallocated => {
                return Ok(0);
            }
            state => {
                info!("Unknown account state: {:?}", state);
                return Err(LibrapayError::UnknownAccountState.into());
            }
        }
        let resource = data_store
            .read_account_resource(&pubkey_to_address(account_address))
            .unwrap();
        let res = resource.balance();
        Ok(res)
    } else {
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{create_genesis, upload_mint_script, upload_payment_script};
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::genesis_config::create_genesis_config;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::sync::Arc;

    fn create_bank(lamports: u64) -> (Arc<Bank>, Keypair, Keypair, Pubkey, Pubkey) {
        let (genesis_config, mint) = create_genesis_config(lamports);
        let mut bank = Bank::new(&genesis_config);
        bank.add_instruction_processor(
            solana_sdk::move_loader::id(),
            solana_move_loader_api::processor::process_instruction,
        );
        let shared_bank = Arc::new(bank);
        let bank_client = BankClient::new_shared(&shared_bank);
        let genesis = create_genesis(&mint, &bank_client, 1_000_000);
        let mint_script = upload_mint_script(&mint, &bank_client);
        let payment_script = upload_payment_script(&mint, &bank_client);
        (shared_bank, mint, genesis, mint_script, payment_script)
    }

    #[test]
    fn test_transfer() {
        solana_logger::setup();

        let (bank, mint, genesis, mint_script, payment_script) = create_bank(10_000);
        let from = Keypair::new();
        let to = Keypair::new();

        let tx = create_accounts(&mint, &[&from, &to], 1, bank.last_blockhash());
        bank.process_transaction(&tx).unwrap();

        info!(
            "created accounts: mint: {} genesis: {}",
            mint.pubkey(),
            genesis.pubkey()
        );
        info!("    from: {} to: {}", from.pubkey(), to.pubkey());

        let tx = mint_tokens(
            &mint_script,
            &mint,
            &genesis,
            &from.pubkey(),
            1,
            bank.last_blockhash(),
        );
        bank.process_transaction(&tx).unwrap();
        let client = BankClient::new_shared(&bank);
        assert_eq!(1, get_libra_balance(&client, &from.pubkey()).unwrap());

        info!("passed mint... doing another transfer..");

        let tx = transfer(
            &payment_script,
            &genesis.pubkey(),
            &mint,
            &from,
            &to.pubkey(),
            1,
            bank.last_blockhash(),
        );
        bank.process_transaction(&tx).unwrap();
        assert_eq!(1, get_libra_balance(&client, &to.pubkey()).unwrap());
    }
}
