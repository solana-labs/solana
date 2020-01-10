use crate::librapay_instruction;
use log::*;
use solana_move_loader_program::{
    account_state::{pubkey_to_address, LibraAccountState},
    data_store::DataStore,
};
use solana_sdk::{
    client::Client,
    commitment_config::CommitmentConfig,
    hash::Hash,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    system_instruction,
    transaction::Transaction,
};
use std::{boxed::Box, error, fmt};

pub fn create_genesis(keypair: &Keypair, microlibras: u64, recent_blockhash: Hash) -> Transaction {
    let ix = librapay_instruction::genesis(&keypair.pubkey(), microlibras);
    Transaction::new_signed_with_payer(
        vec![ix],
        Some(&keypair.pubkey()),
        &[keypair],
        recent_blockhash,
    )
}

pub fn mint_tokens(
    script_pubkey: &Pubkey,
    payer_keypair: &Keypair,
    genesis_keypair: &Keypair,
    to_pubkey: &Pubkey,
    microlibras: u64,
    recent_blockhash: Hash,
) -> Transaction {
    let ix = librapay_instruction::mint(
        script_pubkey,
        &genesis_keypair.pubkey(),
        to_pubkey,
        microlibras,
    );
    Transaction::new_signed_with_payer(
        vec![ix],
        Some(&payer_keypair.pubkey()),
        &[payer_keypair, genesis_keypair],
        recent_blockhash,
    )
}

pub fn transfer(
    script_pubkey: &Pubkey,
    genesis_pubkey: &Pubkey,
    payer_keypair: &Keypair,
    from_keypair: &Keypair,
    to_pubkey: &Pubkey,
    microlibras: u64,
    recent_blockhash: Hash,
) -> Transaction {
    let ix = librapay_instruction::transfer(
        script_pubkey,
        genesis_pubkey,
        &from_keypair.pubkey(),
        to_pubkey,
        microlibras,
    );
    Transaction::new_signed_with_payer(
        vec![ix],
        Some(&payer_keypair.pubkey()),
        &[payer_keypair, from_keypair],
        recent_blockhash,
    )
}

pub fn create_accounts(
    from_keypair: &Keypair,
    to_keypair: &[&Keypair],
    lamports: u64,
    recent_blockhash: Hash,
) -> Transaction {
    let instructions = to_keypair
        .iter()
        .map(|keypair| {
            system_instruction::create_account(
                &from_keypair.pubkey(),
                &keypair.pubkey(),
                lamports,
                400,
                &solana_sdk::move_loader::id(),
            )
        })
        .collect();

    let mut from_signers = vec![from_keypair];
    from_signers.extend_from_slice(to_keypair);
    Transaction::new_signed_instructions(&from_signers, instructions, recent_blockhash)
}

pub fn create_account(
    from_keypair: &Keypair,
    to_keypair: &Keypair,
    lamports: u64,
    recent_blockhash: Hash,
) -> Transaction {
    create_accounts(from_keypair, &[to_keypair], lamports, recent_blockhash)
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
    pubkey: &Pubkey,
) -> Result<u64, Box<dyn error::Error>> {
    if let Some(account) =
        client.get_account_with_commitment(&pubkey, CommitmentConfig::recent())?
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
            .read_account_resource(&pubkey_to_address(pubkey))
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
        let (mut genesis_config, mint) = create_genesis_config(lamports);
        genesis_config.rent.lamports_per_byte_year = 0;
        let mut bank = Bank::new(&genesis_config);
        bank.add_instruction_processor(
            solana_sdk::move_loader::id(),
            solana_move_loader_program::processor::process_instruction,
        );
        let shared_bank = Arc::new(bank);
        let bank_client = BankClient::new_shared(&shared_bank);
        let genesis_pubkey = create_genesis(&mint, &bank_client, 1_000_000);
        let mint_script_pubkey = upload_mint_script(&mint, &bank_client);
        let script_pubkey = upload_payment_script(&mint, &bank_client);
        (
            shared_bank,
            mint,
            genesis_pubkey,
            mint_script_pubkey,
            script_pubkey,
        )
    }

    #[test]
    fn test_transfer() {
        solana_logger::setup();

        let (bank, mint, genesis_keypair, mint_script_pubkey, payment_script_pubkey) =
            create_bank(10_000);
        let from_keypair = Keypair::new();
        let to_keypair = Keypair::new();

        let tx = create_accounts(
            &mint,
            &[&from_keypair, &to_keypair],
            1,
            bank.last_blockhash(),
        );
        bank.process_transaction(&tx).unwrap();

        info!(
            "created accounts: mint: {} genesis_pubkey: {}",
            mint.pubkey(),
            genesis_keypair.pubkey()
        );
        info!(
            "    from: {} to: {}",
            from_keypair.pubkey(),
            to_keypair.pubkey()
        );

        let tx = mint_tokens(
            &mint_script_pubkey,
            &mint,
            &genesis_keypair,
            &from_keypair.pubkey(),
            1,
            bank.last_blockhash(),
        );
        bank.process_transaction(&tx).unwrap();
        let client = BankClient::new_shared(&bank);
        assert_eq!(
            1,
            get_libra_balance(&client, &from_keypair.pubkey()).unwrap()
        );

        info!("passed mint... doing another transfer..");

        let tx = transfer(
            &payment_script_pubkey,
            &genesis_keypair.pubkey(),
            &mint,
            &from_keypair,
            &to_keypair.pubkey(),
            1,
            bank.last_blockhash(),
        );
        bank.process_transaction(&tx).unwrap();
        assert_eq!(1, get_libra_balance(&client, &to_keypair.pubkey()).unwrap());
    }
}
