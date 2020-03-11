solana_sdk::declare_id!("LibraPay11111111111111111111111111111111111");

pub mod librapay_instruction;
pub mod librapay_transaction;

extern crate solana_move_loader_program;

use solana_move_loader_program::{account_state::LibraAccountState, processor::MoveProcessor};
use solana_runtime::loader_utils::load_program;
use solana_sdk::{
    account::KeyedAccount,
    client::Client,
    instruction::InstructionError,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
};

use types::account_config;

pub fn create_genesis<T: Client>(from: &Keypair, client: &T, amount: u64) -> Keypair {
    let genesis = Keypair::new();

    let instruction = system_instruction::create_account(
        &from.pubkey(),
        &genesis.pubkey(),
        1,
        bincode::serialized_size(&LibraAccountState::create_genesis(amount).unwrap()).unwrap()
            as u64,
        &solana_sdk::move_loader::id(),
    );

    client
        .send_message(&[&from, &genesis], Message::new(&[instruction]))
        .unwrap();

    let instruction = librapay_instruction::genesis(&genesis.pubkey(), amount);
    let message = Message::new_with_payer(&[instruction], Some(&from.pubkey()));
    client.send_message(&[from, &genesis], message).unwrap();

    genesis
}

pub fn publish_module<T: Client>(from: &Keypair, client: &T, code: &str) -> Pubkey {
    let address = account_config::association_address();
    let account_state = LibraAccountState::create_module(&address, code, vec![]);
    let bytes = bincode::serialize(&account_state).unwrap();

    load_program(client, &from, &solana_sdk::move_loader::id(), bytes)
}

pub fn upload_script<T: Client>(from: &Keypair, client: &T, code: &str) -> Pubkey {
    let address = account_config::association_address();
    let account_state = LibraAccountState::create_script(&address, code, vec![]);
    let bytes = bincode::serialize(&account_state).unwrap();

    load_program(client, &from, &solana_sdk::move_loader::id(), bytes)
}

pub fn upload_mint_script<T: Client>(from: &Keypair, client: &T) -> Pubkey {
    let code = "
            import 0x0.LibraAccount;
            import 0x0.LibraCoin;
            main(payee: address, amount: u64) {
                LibraAccount.mint_to_address(move(payee), move(amount));
                return;
            }";
    upload_script(from, client, code)
}
pub fn upload_payment_script<T: Client>(from: &Keypair, client: &T) -> Pubkey {
    let code = "
        import 0x0.LibraAccount;
        import 0x0.LibraCoin;
        main(payee: address, amount: u64) {
            LibraAccount.pay_from_sender(move(payee), move(amount));
            return;
        }
    ";

    upload_script(from, client, code)
}

pub fn process_instruction(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    MoveProcessor::process_instruction(program_id, keyed_accounts, data)
}
