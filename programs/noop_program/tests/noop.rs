use solana_runtime::bank::Bank;
use solana_runtime::bank_client::BankClient;
use solana_runtime::loader_utils::{create_invoke_instruction, load_program};
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::native_loader;

#[test]
fn test_program_native_noop() {
    solana_logger::setup();

    let (genesis_block, mint_keypair) = GenesisBlock::new(50);
    let bank = Bank::new(&genesis_block);
    let alice_client = BankClient::new(&bank, mint_keypair);

    let program = "noop".as_bytes().to_vec();
    let program_id = load_program(&bank, &alice_client, &native_loader::id(), program);

    // Call user program
    let instruction = create_invoke_instruction(alice_client.pubkey(), program_id, &1u8);
    alice_client.process_instruction(instruction).unwrap();
}
