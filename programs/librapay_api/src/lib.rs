const LIBRAPAY_PROGRAM_ID: [u8; 32] = [
    5, 13, 18, 222, 165, 11, 80, 225, 56, 103, 125, 38, 15, 252, 181, 16, 125, 99, 110, 106, 186,
    28, 136, 119, 235, 245, 20, 80, 0, 0, 0, 0,
];

solana_sdk::solana_name_id!(
    LIBRAPAY_PROGRAM_ID,
    "LibraPay11111111111111111111111111111111111"
);

pub mod librapay_instruction;
pub mod librapay_transaction;

extern crate solana_move_loader_program;

use solana_move_loader_program::account_state::LibraAccountState;
use solana_runtime::loader_utils::load_program;
use solana_sdk::account::KeyedAccount;
use solana_sdk::client::Client;
use solana_sdk::instruction::InstructionError;
use solana_sdk::message::Message;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction;

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
        .send_message(&[&from, &genesis], Message::new(vec![instruction]))
        .unwrap();

    let instruction = librapay_instruction::genesis(&genesis.pubkey(), amount);
    let message = Message::new_with_payer(vec![instruction], Some(&from.pubkey()));
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
    solana_move_loader_program::processor::process_instruction(program_id, keyed_accounts, data)
}
