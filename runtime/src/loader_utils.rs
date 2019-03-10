use crate::bank::Bank;
use solana_sdk::loader_transaction::LoaderTransaction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;

pub fn load_program(bank: &Bank, from: &Keypair, loader_id: &Pubkey, program: Vec<u8>) -> Pubkey {
    let program_account = Keypair::new();

    let tx = SystemTransaction::new_program_account(
        from,
        &program_account.pubkey(),
        bank.last_blockhash(),
        1,
        program.len() as u64,
        loader_id,
        0,
    );
    bank.process_transaction(&tx).unwrap();
    assert_eq!(bank.get_signature_status(&tx.signatures[0]), Some(Ok(())));

    let chunk_size = 256; // Size of chunk just needs to fit into tx
    let mut offset = 0;
    for chunk in program.chunks(chunk_size) {
        let tx = LoaderTransaction::new_write(
            &program_account,
            loader_id,
            offset,
            chunk.to_vec(),
            bank.last_blockhash(),
            0,
        );
        bank.process_transaction(&tx).unwrap();
        offset += chunk_size as u32;
    }

    let tx = LoaderTransaction::new_finalize(&program_account, loader_id, bank.last_blockhash(), 0);
    bank.process_transaction(&tx).unwrap();
    assert_eq!(bank.get_signature_status(&tx.signatures[0]), Some(Ok(())));

    program_account.pubkey()
}
