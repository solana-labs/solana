use safecoin_runtime::{
    bank::Bank, bank_client::BankClient, loader_utils::create_invoke_instruction,
};
use safecoin_sdk::{client::SyncClient, genesis_config::create_genesis_config, signature::Signer};

#[test]
fn test_program_native_noop() {
    safecoin_logger::setup();

    let (genesis_config, alice_keypair) = create_genesis_config(50);
    let program_id = safecoin_sdk::pubkey::new_rand();
    let bank = Bank::new(&genesis_config);
    bank.add_native_program("safecoin_noop_program", &program_id, false);

    // Call user program
    let instruction = create_invoke_instruction(alice_keypair.pubkey(), program_id, &1u8);
    let bank_client = BankClient::new(bank);
    bank_client
        .send_and_confirm_instruction(&alice_keypair, instruction)
        .unwrap();
}
