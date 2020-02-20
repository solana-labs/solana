use serde::Serialize;
use solana_sdk::{
    client::Client,
    instruction::{AccountMeta, Instruction},
    loader_instruction,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
};

pub fn load_program<T: Client>(
    bank_client: &T,
    from_keypair: &Keypair,
    loader_pubkey: &Pubkey,
    program: Vec<u8>,
) -> Pubkey {
    let program_keypair = Keypair::new();
    let program_pubkey = program_keypair.pubkey();

    let instruction = system_instruction::create_account(
        &from_keypair.pubkey(),
        &program_pubkey,
        1,
        program.len() as u64,
        loader_pubkey,
    );
    bank_client
        .send_message(
            &[from_keypair, &program_keypair],
            Message::new(vec![instruction]),
        )
        .unwrap();

    let chunk_size = 256; // Size of chunk just needs to fit into tx
    let mut offset = 0;
    for chunk in program.chunks(chunk_size) {
        let instruction =
            loader_instruction::write(&program_pubkey, loader_pubkey, offset, chunk.to_vec());
        let message = Message::new_with_payer(vec![instruction], Some(&from_keypair.pubkey()));
        bank_client
            .send_message(&[from_keypair, &program_keypair], message)
            .unwrap();
        offset += chunk_size as u32;
    }

    let instruction = loader_instruction::finalize(&program_pubkey, loader_pubkey);
    let message = Message::new_with_payer(vec![instruction], Some(&from_keypair.pubkey()));
    bank_client
        .send_message(&[from_keypair, &program_keypair], message)
        .unwrap();

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
