use {
    solana_program_test::{processor, ProgramTest},
    solana_sdk::{
        account_info::{next_account_info, AccountInfo},
        entrypoint::ProgramResult,
        instruction::{AccountMeta, Instruction},
        msg,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_instruction,
        transaction::Transaction,
    },
};

fn move_lamports_process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _input: &[u8],
) -> ProgramResult {
    msg!("Processing lamports instruction");
    let account_info_iter = &mut accounts.iter();
    let source_info = next_account_info(account_info_iter)?;
    let _other_info = next_account_info(account_info_iter)?;
    let destination_info = next_account_info(account_info_iter)?;

    let destination_lamports = destination_info.lamports();
    **destination_info.lamports.borrow_mut() = destination_lamports
        .checked_add(source_info.lamports())
        .unwrap();
    **source_info.lamports.borrow_mut() = 0;
    Ok(())
}

#[tokio::test]
async fn move_lamports() {
    let move_lamports_program_id = Pubkey::new_unique();
    let program_test = ProgramTest::new(
        "move_lamports",
        move_lamports_program_id,
        processor!(move_lamports_process_instruction),
    );

    let lamports = 1_000_000_000;
    let source = Keypair::new();
    let mut context = program_test.start_with_context().await;
    let instructions = vec![
        system_instruction::create_account(
            &context.payer.pubkey(),
            &source.pubkey(),
            lamports,
            0,
            &move_lamports_program_id,
        ),
        Instruction {
            program_id: move_lamports_program_id,
            accounts: vec![
                AccountMeta::new(source.pubkey(), false),
                AccountMeta::new(context.payer.pubkey(), false),
                AccountMeta::new(context.payer.pubkey(), false),
            ],
            data: vec![],
        },
    ];

    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer, &source],
        context.last_blockhash,
    );

    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    assert_eq!(
        context
            .banks_client
            .get_balance(source.pubkey())
            .await
            .unwrap(),
        0
    );
}
