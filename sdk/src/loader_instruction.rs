use crate::instruction::{AccountMeta, Instruction};
use crate::pubkey::Pubkey;
use crate::sysvar::rent;
use bincode::serialize;
use serde::Serialize;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum LoaderInstruction {
    /// Write program data into an account
    ///
    /// * key[0] - the account to write into.
    ///
    /// The transaction must be signed by key[0]
    Write {
        offset: u32,
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    },

    /// Finalize an account loaded with program data for execution.
    /// The exact preparation steps is loader specific but on success the loader must set the executable
    /// bit of the Account
    ///
    /// * key[0] - the account to prepare for execution
    /// * key[1] - rent sysvar account
    ///
    /// The transaction must be signed by key[0]
    Finalize,

    /// Invoke the "main" entrypoint with the given data.
    ///
    /// * key[0] - an executable account
    InvokeMain {
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
}

/// Create an instruction to write program data into an account
pub fn write(
    account_pubkey: &Pubkey,
    program_id: &Pubkey,
    offset: u32,
    bytes: Vec<u8>,
) -> Instruction {
    let account_metas = vec![AccountMeta::new(*account_pubkey, true)];
    Instruction::new(
        *program_id,
        &LoaderInstruction::Write { offset, bytes },
        account_metas,
    )
}

/// Create an instruction to finalize a program data account, once finalized it can no longer be modified
pub fn finalize(account_pubkey: &Pubkey, program_id: &Pubkey) -> Instruction {
    let account_metas = vec![
        AccountMeta::new(*account_pubkey, true),
        AccountMeta::new(rent::id(), false),
    ];
    Instruction::new(*program_id, &LoaderInstruction::Finalize, account_metas)
}

// Create an instruction to Invoke a program's "main" entrypoint with the given data
pub fn invoke_main<T: Serialize>(
    program_id: &Pubkey,
    data: &T,
    account_metas: Vec<AccountMeta>,
) -> Instruction {
    let ix_data = LoaderInstruction::InvokeMain {
        data: serialize(data).unwrap().to_vec(),
    };
    Instruction::new(*program_id, &ix_data, account_metas)
}
