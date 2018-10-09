//! The `dynamic_transaction` module provides functionality for loading and calling a program

use std::mem;

use bincode::serialize;
use dynamic_instruction::Instruction;
use dynamic_program::DynamicProgram;
use hash::Hash;
use signature::Keypair;
use solana_program_interface::pubkey::Pubkey;
use transaction::Transaction;

pub trait ProgramTransaction {
    // TODO combine all these into one more generic based om passed in instruction
    fn program_load_native(
        from_keypair: &Keypair,
        program: Pubkey,
        name: String,
        last_id: Hash,
        fee: i64,
    ) -> Self;

    fn program_load_bpffile(
        from_keypair: &Keypair,
        program: Pubkey,
        name: String,
        last_id: Hash,
        fee: i64,
    ) -> Self;

    fn program_load_bpf(
        from_keypair: &Keypair,
        program: Pubkey,
        offset: u64,
        data: Vec<u8>,
        last_id: Hash,
        fee: i64,
    ) -> Self;

    fn program_load_state(
        from_keypair: &Keypair,
        state: Pubkey,
        offset: u64,
        data: Vec<u8>,
        last_id: Hash,
        fee: i64,
    ) -> Self;

    // TODO input parameters are contained in tx user data
    //      might have to also chunk that into an account as well
    fn program_call(
        from_keypari: &Keypair,
        accounts: &Vec<Pubkey>,
        input: Vec<u8>,
        last_id: Hash,
        fee: i64,
    ) -> Self;
}

impl ProgramTransaction for Transaction {
    /// Create and sign a new Program::LoadNative transaction
    fn program_load_native(
        from_keypair: &Keypair,
        program: Pubkey,
        name: String,
        last_id: Hash,
        fee: i64,
    ) -> Self {
        println!("program_load_native name {:?}", name);
        let native = Instruction::LoadNative { name };
        let userdata = serialize(&native).unwrap();
        Transaction::new(
            from_keypair,
            &[program],
            DynamicProgram::id(),
            userdata,
            last_id,
            fee,
        )
    }

    fn program_load_bpffile(
        from_keypair: &Keypair,
        program: Pubkey,
        name: String,
        last_id: Hash,
        fee: i64,
    ) -> Self {
        println!("program_load_bpffile name {:?}", name);
        let bpffile = Instruction::LoadBpfFile { name };
        let userdata = serialize(&bpffile).unwrap();
        Transaction::new(
            from_keypair,
            &[program],
            DynamicProgram::id(),
            userdata,
            last_id,
            fee,
        )
    }

    fn program_load_bpf(
        from_keypair: &Keypair,
        program: Pubkey,
        offset: u64,
        prog: Vec<u8>,
        last_id: Hash,
        fee: i64,
    ) -> Self {
        println!("program_load_bpf size {}", prog.len());
        let inst = Instruction::LoadBpf { offset, prog };
        let userdata = serialize(&inst).unwrap();
        Transaction::new(
            from_keypair,
            &[program],
            DynamicProgram::id(),
            userdata,
            last_id,
            fee,
        )
    }

    fn program_load_state(
        from_keypair: &Keypair,
        state: Pubkey,
        offset: u64,
        data: Vec<u8>,
        last_id: Hash,
        fee: i64,
    ) -> Self {
        println!("program_load_state offset {} size {}", offset, data.len());
        let inst = Instruction::LoadState { offset, data };
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

    fn program_call(
        from_keypair: &Keypair,
        accounts: &Vec<Pubkey>,
        input: Vec<u8>,
        last_id: Hash,
        fee: i64,
    ) -> Self {
        println!("program_call size {}", input.len());
        let inst = Instruction::Call { input };
        let userdata = serialize(&inst).unwrap();
        Transaction::new(
            from_keypair,
            accounts,
            DynamicProgram::id(),
            userdata,
            last_id,
            fee,
        )
    }
}

#[cfg(test)]
mod tests {}
