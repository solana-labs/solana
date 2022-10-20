use {
    solana_sdk::{
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
    },
    spl_instruction_padding::solana_program::{
        instruction::{AccountMeta as SplAccountMeta, Instruction as SplInstruction},
        pubkey::Pubkey as SplPubkey,
    },
};

pub fn solana_instruction(instruction: SplInstruction) -> Instruction {
    Instruction {
        program_id: solana_pubkey(&instruction.program_id),
        accounts: instruction
            .accounts
            .iter()
            .map(|meta| AccountMeta {
                pubkey: solana_pubkey(&meta.pubkey),
                is_signer: meta.is_signer,
                is_writable: meta.is_writable,
            })
            .collect(),
        data: instruction.data,
    }
}

pub fn spl_instruction(instruction: Instruction) -> SplInstruction {
    SplInstruction {
        program_id: spl_pubkey(&instruction.program_id),
        accounts: instruction
            .accounts
            .iter()
            .map(|meta| SplAccountMeta {
                pubkey: spl_pubkey(&meta.pubkey),
                is_signer: meta.is_signer,
                is_writable: meta.is_writable,
            })
            .collect(),
        data: instruction.data,
    }
}

pub fn solana_pubkey(pubkey: &SplPubkey) -> Pubkey {
    Pubkey::new_from_array(pubkey.to_bytes())
}

pub fn spl_pubkey(pubkey: &Pubkey) -> SplPubkey {
    SplPubkey::new_from_array(pubkey.to_bytes())
}
