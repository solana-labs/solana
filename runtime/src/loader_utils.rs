use crate::bank::Bank;
use crate::bank_client::BankClient;
use serde::Serialize;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::loader_instruction::LoaderInstruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_instruction::SystemInstruction;

pub fn load_program(
    bank: &Bank,
    from_client: &BankClient,
    loader_id: &Pubkey,
    program: Vec<u8>,
) -> Pubkey {
    let program_keypair = Keypair::new();
    let program_pubkey = program_keypair.pubkey();

    let instruction = SystemInstruction::new_program_account(
        &from_client.pubkey(),
        &program_pubkey,
        1,
        program.len() as u64,
        loader_id,
    );
    from_client.process_instruction(instruction).unwrap();

    let program_client = BankClient::new(bank, program_keypair);

    let chunk_size = 256; // Size of chunk just needs to fit into tx
    let mut offset = 0;
    for chunk in program.chunks(chunk_size) {
        let instruction =
            LoaderInstruction::new_write(&program_pubkey, loader_id, offset, chunk.to_vec());
        program_client.process_instruction(instruction).unwrap();
        offset += chunk_size as u32;
    }

    let instruction = LoaderInstruction::new_finalize(&program_pubkey, loader_id);
    program_client.process_instruction(instruction).unwrap();

    program_pubkey
}

// Return an Instruction that invokes `program_id` with `data` and required
// a signature from `from_pubkey`.
pub fn create_invoke_instruction<T: Serialize>(
    from_pubkey: Pubkey,
    program_id: Pubkey,
    data: &T,
) -> Instruction {
    let account_metas = vec![AccountMeta::new(from_pubkey, true)];
    Instruction::new(program_id, data, account_metas)
}
