use bincode;
use solana_move_loader_api::account_state::pubkey_to_address;
use solana_move_loader_api::processor::InvokeCommand;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::loader_instruction::LoaderInstruction;
use solana_sdk::pubkey::Pubkey;
use types::account_address::AccountAddress;
use types::transaction::TransactionArgument;

pub fn genesis(genesis_pubkey: &Pubkey, microlibras: u64) -> Instruction {
    let data = bincode::serialize(&InvokeCommand::CreateGenesis(microlibras)).unwrap();
    let ix_data = LoaderInstruction::InvokeMain { data };

    let accounts = vec![AccountMeta::new(*genesis_pubkey, true)];

    Instruction::new(solana_move_loader_api::id(), &ix_data, accounts)
}

pub fn mint(
    program_pubkey: &Pubkey,
    from_pubkey: &Pubkey,
    to_pubkey: &Pubkey,
    microlibras: u64,
) -> Instruction {
    let args = vec![
        TransactionArgument::Address(pubkey_to_address(to_pubkey)),
        TransactionArgument::U64(microlibras),
    ];

    let data = bincode::serialize(&InvokeCommand::RunProgram {
        sender_address: AccountAddress::default(),
        function_name: "main".to_string(),
        args,
    })
    .unwrap();
    let ix_data = LoaderInstruction::InvokeMain { data };

    let accounts = vec![
        AccountMeta::new_credit_only(*program_pubkey, false),
        AccountMeta::new(*from_pubkey, true),
        AccountMeta::new(*to_pubkey, false),
    ];

    Instruction::new(solana_move_loader_api::id(), &ix_data, accounts)
}

pub fn transfer(
    program_pubkey: &Pubkey,
    mint_pubkey: &Pubkey,
    from_pubkey: &Pubkey,
    to_pubkey: &Pubkey,
    microlibras: u64,
) -> Instruction {
    let args = vec![
        TransactionArgument::Address(pubkey_to_address(to_pubkey)),
        TransactionArgument::U64(microlibras),
    ];

    let data = bincode::serialize(&InvokeCommand::RunProgram {
        sender_address: pubkey_to_address(from_pubkey),
        function_name: "main".to_string(),
        args,
    })
    .unwrap();
    let ix_data = LoaderInstruction::InvokeMain { data };

    let accounts = vec![
        AccountMeta::new_credit_only(*program_pubkey, false),
        AccountMeta::new_credit_only(*mint_pubkey, false),
        AccountMeta::new(*from_pubkey, true),
        AccountMeta::new(*to_pubkey, false),
    ];

    Instruction::new(solana_move_loader_api::id(), &ix_data, accounts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pay() {
        let from = Pubkey::new_rand();
        let to = Pubkey::new_rand();
        let program_id = Pubkey::new_rand();
        let mint_id = Pubkey::new_rand();
        transfer(&program_id, &mint_id, &from, &to, 1);
    }

    #[test]
    fn test_mint() {
        let program_id = Pubkey::new_rand();
        let from = Pubkey::new_rand();
        let to = Pubkey::new_rand();

        mint(&program_id, &from, &to, 1);
    }
}
