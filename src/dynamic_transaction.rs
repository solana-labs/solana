//! The `dynamic_transaction` module provides functionality for loading and calling a program

use bincode::serialize;
use dynamic_instruction::DynamicInstruction;
use dynamic_program::DynamicProgram;
use hash::Hash;
use signature::Keypair;
use solana_program_interface::pubkey::Pubkey;
use transaction::Transaction;

pub trait ProgramTransaction {
    // TODO combine all these into one more generic based om passed in instruction
    fn program_new_load(
        from_keypair: &Keypair,
        program: Pubkey,
        instruction: DynamicInstruction,
        last_id: Hash,
        fee: i64,
    ) -> Self;

    fn program_new_load_state(
        from_keypair: &Keypair,
        state: Pubkey,
        offset: u64,
        data: Vec<u8>,
        last_id: Hash,
        fee: i64,
    ) -> Self;

    // TODO input parameters are contained in tx user data
    //      might have to also chunk that into an account as well
    fn program_new_call(
        from_keypari: &Keypair,
        program: Pubkey,
        state: Pubkey,
        accounts: &[Pubkey],
        input: Vec<u8>,
        last_id: Hash,
        fee: i64,
    ) -> Self;
}

impl ProgramTransaction for Transaction {
    /// Create and sign a new Program::LoadNative transaction
    fn program_new_load(
        from_keypair: &Keypair,
        program: Pubkey,
        instruction: DynamicInstruction,
        last_id: Hash,
        fee: i64,
    ) -> Self {
        println!("program_load {:?}", instruction);
        let userdata = serialize(&instruction).unwrap();
        Transaction::new(
            from_keypair,
            &[program],
            DynamicProgram::id(),
            userdata,
            last_id,
            fee,
        )
    }

    fn program_new_load_state(
        from_keypair: &Keypair,
        state: Pubkey,
        offset: u64,
        data: Vec<u8>,
        last_id: Hash,
        fee: i64,
    ) -> Self {
        println!("program_load_state offset {} size {}", offset, data.len());
        let inst = DynamicInstruction::LoadState { offset, data };
        let userdata = serialize(&inst).unwrap();
        Transaction::new(
            from_keypair,
            &[state],
            DynamicProgram::id(),
            userdata,
            last_id,
            fee,
        )
    }

    fn program_new_call(
        from_keypair: &Keypair,
        program: Pubkey,
        state: Pubkey,
        accounts: &[Pubkey],
        input: Vec<u8>,
        last_id: Hash,
        fee: i64,
    ) -> Self {
        println!("program_call size {}", input.len());
        let mut keys = vec![program, state];
        keys.extend_from_slice(accounts);
        let inst = DynamicInstruction::Call { input };
        let userdata = serialize(&inst).unwrap();
        Transaction::new(
            from_keypair,
            &keys,
            DynamicProgram::id(),
            userdata,
            last_id,
            fee,
        )
    }
}

#[cfg(test)]
mod tests {}
