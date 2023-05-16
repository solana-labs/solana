use {
    solana_program_test::{processor, ProgramTest},
    solana_sdk::{
        account_info::AccountInfo, entrypoint::ProgramResult, msg, pubkey::Pubkey,
        signature::Signer, sysvar::Sysvar, transaction::Transaction,
    },
};
use solana_sdk::clock::Slot;
use solana_sdk::instruction::Instruction;
use solana_sdk::sysvar::last_restart_slot;
use solana_sdk::sysvar::last_restart_slot::LastRestartSlot;

fn sysvar_last_restart_slot_process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _input: &[u8],
) -> ProgramResult {
    msg!("sysvar_last_restart_slot");

    let last_restart_slot = LastRestartSlot::get();
    msg!("last restart slot: {:?}", last_restart_slot);
    assert_eq!(last_restart_slot, Ok(LastRestartSlot { last_restart_slot: 33333 }));

    Ok(())
}

#[tokio::test]
async fn get_sysvar_last_restart_slot() {
    let program_id = Pubkey::new_unique();
    let program_test = ProgramTest::new(
        "sysvar_last_restart_slot_process_instruction",
        program_id,
        processor!(sysvar_last_restart_slot_process_instruction),
    );

    let mut context = program_test.start_with_context().await;
    context.warp_to_slot(40).unwrap();
    context.register_hard_fork(33333 as Slot);
    context.warp_to_slot(42).unwrap();

    let instructions = vec![Instruction::new_with_bincode(program_id, &(), vec![])];

    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    context
        .banks_client
        .process_transaction(transaction)
        .await.unwrap();
}
