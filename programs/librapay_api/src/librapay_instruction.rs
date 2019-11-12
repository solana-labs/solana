use bincode;
use solana_move_loader_api::account_state::pubkey_to_address;
use solana_move_loader_api::processor::InvokeCommand;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::loader_instruction::LoaderInstruction;
use solana_sdk::pubkey::Pubkey;
use types::account_config;
use types::transaction::TransactionArgument;

pub fn genesis(genesis: &Pubkey, microlibras: u64) -> Instruction {
    let data = bincode::serialize(&InvokeCommand::CreateGenesis(microlibras)).unwrap();
    let ix_data = LoaderInstruction::InvokeMain { data };

    let accounts = vec![AccountMeta::new(*genesis, true)];

    Instruction::new(solana_sdk::move_loader::id(), &ix_data, accounts)
}

pub fn mint(script: &Pubkey, genesis: &Pubkey, to: &Pubkey, microlibras: u64) -> Instruction {
    let args = vec![
        TransactionArgument::Address(pubkey_to_address(to)),
        TransactionArgument::U64(microlibras),
    ];

    let data = bincode::serialize(&InvokeCommand::RunScript {
        sender_address: account_config::association_address(),
        function_name: "main".to_string(),
        args,
    })
    .unwrap();
    let ix_data = LoaderInstruction::InvokeMain { data };

    let accounts = vec![
        AccountMeta::new_readonly(*script, false),
        AccountMeta::new(*genesis, true),
        AccountMeta::new(*to, false),
    ];

    Instruction::new(solana_sdk::move_loader::id(), &ix_data, accounts)
}

pub fn transfer(
    script: &Pubkey,
    genesis: &Pubkey,
    from: &Pubkey,
    to: &Pubkey,
    microlibras: u64,
) -> Instruction {
    let args = vec![
        TransactionArgument::Address(pubkey_to_address(to)),
        TransactionArgument::U64(microlibras),
    ];

    let data = bincode::serialize(&InvokeCommand::RunScript {
        sender_address: pubkey_to_address(from),
        function_name: "main".to_string(),
        args,
    })
    .unwrap();
    let ix_data = LoaderInstruction::InvokeMain { data };

    let accounts = vec![
        AccountMeta::new_readonly(*script, false),
        AccountMeta::new_readonly(*genesis, false),
        AccountMeta::new(*from, true),
        AccountMeta::new(*to, false),
    ];

    Instruction::new(solana_sdk::move_loader::id(), &ix_data, accounts)
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
