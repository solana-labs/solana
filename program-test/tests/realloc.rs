use {
    solana_program_test::{processor, ProgramTest},
    solana_sdk::{
        account_info::{next_account_info, AccountInfo},
        entrypoint::ProgramResult,
        instruction::{AccountMeta, Instruction},
        program::invoke,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_instruction,
        sysvar::rent,
        transaction::Transaction,
    },
};

fn process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _input: &[u8],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let account_info = next_account_info(account_info_iter)?;
    let destination_info = next_account_info(account_info_iter)?;
    let owner_info = next_account_info(account_info_iter)?;
    let token_program_info = next_account_info(account_info_iter)?;
    invoke(
        &Instruction::new_with_bytes(
            *token_program_info.key,
            &[9], // close account
            vec![
                AccountMeta::new(*account_info.key, false),
                AccountMeta::new(*destination_info.key, false),
                AccountMeta::new_readonly(*owner_info.key, true),
            ],
        ),
        &[
            account_info.clone(),
            destination_info.clone(),
            owner_info.clone(),
        ],
    )?;
    Ok(())
}

#[tokio::test]
async fn realloc_smaller_in_cpi() {
    let program_id = Pubkey::new_unique();
    let program_test = ProgramTest::new(
        "program-test-realloc",
        program_id,
        processor!(process_instruction),
    );
    let mut context = program_test.start_with_context().await;

    let token_2022_id = Pubkey::try_from("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb").unwrap();
    let mint = Keypair::new();
    let account = Keypair::new();
    let rent = context.banks_client.get_rent().await.unwrap();
    let mint_space = 82;
    let account_space = 165;
    let transaction = Transaction::new_signed_with_payer(
        &[
            system_instruction::create_account(
                &context.payer.pubkey(),
                &mint.pubkey(),
                rent.minimum_balance(mint_space),
                mint_space as u64,
                &token_2022_id,
            ),
            Instruction::new_with_bytes(
                token_2022_id,
                &[0; 35], // initialize mint
                vec![
                    AccountMeta::new(mint.pubkey(), false),
                    AccountMeta::new_readonly(rent::id(), false),
                ],
            ),
            system_instruction::create_account(
                &context.payer.pubkey(),
                &account.pubkey(),
                rent.minimum_balance(account_space),
                account_space as u64,
                &token_2022_id,
            ),
            Instruction::new_with_bytes(
                token_2022_id,
                &[1], // initialize account
                vec![
                    AccountMeta::new(account.pubkey(), false),
                    AccountMeta::new_readonly(mint.pubkey(), false),
                    AccountMeta::new_readonly(account.pubkey(), false),
                    AccountMeta::new_readonly(rent::id(), false),
                ],
            ),
            Instruction::new_with_bytes(
                program_id,
                &[], // close account
                vec![
                    AccountMeta::new(account.pubkey(), false),
                    AccountMeta::new(mint.pubkey(), false),
                    AccountMeta::new_readonly(account.pubkey(), true),
                    AccountMeta::new_readonly(token_2022_id, false),
                ],
            ),
        ],
        Some(&context.payer.pubkey()),
        &[&context.payer, &mint, &account],
        context.last_blockhash,
    );

    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();
}
