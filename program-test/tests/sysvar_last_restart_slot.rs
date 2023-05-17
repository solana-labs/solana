use solana_program_runtime::solana_rbpf::syscalls::bpf_syscall_string;
use solana_sdk::account_info::next_account_info;
use solana_sdk::clock::Slot;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::sysvar;
use solana_sdk::sysvar::last_restart_slot;
use solana_sdk::sysvar::last_restart_slot::LastRestartSlot;
use {
    solana_program_test::{processor, ProgramTest},
    solana_sdk::{
        account_info::AccountInfo, entrypoint::ProgramResult, msg, pubkey::Pubkey,
        signature::Signer, sysvar::Sysvar, transaction::Transaction,
    },
};

fn sysvar_last_restart_slot_process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _input: &[u8],
) -> ProgramResult {
    msg!("sysvar_last_restart_slot");

    let last_restart_slot = LastRestartSlot::get();
    msg!("last restart slot: {:?}", last_restart_slot);
    assert_eq!(
        last_restart_slot,
        Ok(LastRestartSlot {
            last_restart_slot: 33333
        })
    );

    Ok(())
}

#[tokio::test]
async fn get_sysvar_last_restart_slot() {
    let program_id = Pubkey::new_unique();
    let program_test = ProgramTest::new(
        "sysvar_last_restart_slot_process",
        program_id,
        processor!(sysvar_last_restart_slot_process_instruction),
    );

    let mut context = program_test.start_with_context().await;
    context.warp_to_slot(40).unwrap();
    context.register_hard_fork(33333 as Slot);
    context.warp_to_slot(42).unwrap();

    let instructions = vec![Instruction::new_with_bincode(
        program_id,
        &(),
        vec![AccountMeta::new(sysvar::last_restart_slot::id(), false)],
    )];

    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();
}

fn sysvar_last_restart_slot_from_account_process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _input: &[u8],
) -> ProgramResult {
    msg!("sysvar_last_restart_slot_from_account");

    let last_restart_slot_account = &accounts[0];

    let slot_via_account = LastRestartSlot::from_account_info(last_restart_slot_account)?;
    msg!("slot via account: {:?}", slot_via_account);

    assert_eq!(
        slot_via_account,
        LastRestartSlot {
            last_restart_slot: 33333
        }
    );

    Ok(())
}

#[tokio::test]
async fn get_sysvar_last_restart_slot_from_account() {
    let program_id = Pubkey::new_unique();
    let program_test = ProgramTest::new(
        "sysvar_last_restart_slot_from_account",
        program_id,
        processor!(sysvar_last_restart_slot_from_account_process_instruction),
    );

    let mut context = program_test.start_with_context().await;
    context.warp_to_slot(40).unwrap();
    context.register_hard_fork(33333 as Slot);
    context.warp_to_slot(42).unwrap();

    let instructions = vec![Instruction::new_with_bincode(
        program_id,
        &(),
        // note: here we pass the sysvar as an account
        vec![AccountMeta::new(sysvar::last_restart_slot::id(), false)],
    )];

    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&context.payer.pubkey()),
        &[&context.payer],
        context.last_blockhash,
    );

    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();
}
