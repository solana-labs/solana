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

extern crate solana_move_loader_api;

use solana_move_loader_api::account_state::LibraAccountState;
use solana_runtime::loader_utils::load_program;
use solana_sdk::account::KeyedAccount;
use solana_sdk::client::Client;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;

use types::account_address::AccountAddress;

pub fn upload_move_program<T: Client>(from: &Keypair, client: &T, code: &str) -> Pubkey {
    let address = AccountAddress::default();
    let account_state = LibraAccountState::create_program(&address, code, vec![]);
    let program_bytes = bincode::serialize(&account_state).unwrap();

    load_program(client, &from, &solana_move_loader_api::id(), program_bytes)
}

pub fn upload_mint_program<T: Client>(from: &Keypair, client: &T) -> Pubkey {
    let code = "
            import 0x0.LibraAccount;
            import 0x0.LibraCoin;
            main(payee: address, amount: u64) {
                LibraAccount.mint_to_address(move(payee), move(amount));
                return;
            }";
    upload_move_program(from, client, code)
}

pub fn upload_payment_program<T: Client>(from: &Keypair, client: &T) -> Pubkey {
    let code = "
        import 0x0.LibraAccount;
        import 0x0.LibraCoin;
        main(payee: address, amount: u64) {
            LibraAccount.pay_from_sender(move(payee), move(amount));
            return;
        }
    ";

    upload_move_program(from, client, code)
}

pub fn process_instruction(
    program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    solana_move_loader_api::processor::process_instruction(program_id, keyed_accounts, data)
}
