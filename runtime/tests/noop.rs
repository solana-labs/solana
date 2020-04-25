use solana_runtime::{
    bank::Bank, bank_client::BankClient, loader_utils::create_invoke_instruction,
};
use solana_sdk::{
    client::SyncClient, genesis_config::create_genesis_config, pubkey::Pubkey, signature::Signer,
};

#[test]
fn test_program_native_noop() {
    solana_logger::setup();

    let (genesis_config, alice_keypair) = create_genesis_config(50);
    let program_id = Pubkey::new_rand();
    let bank = Bank::new(&genesis_config);
    bank.add_native_program("solana_noop_program", &program_id);

    // Call user program
    let instruction = create_invoke_instruction(alice_keypair.pubkey(), program_id, &1u8);
    let bank_client = BankClient::new(bank);
    bank_client
        .send_instruction(&alice_keypair, instruction)
        .unwrap();
}
