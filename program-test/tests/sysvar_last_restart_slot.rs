use {
    solana_program_test::{processor, ProgramTest, ProgramTestContext},
    solana_sdk::{
        account_info::AccountInfo,
        clock::Slot,
        entrypoint::ProgramResult,
        instruction::{AccountMeta, Instruction},
        msg,
        pubkey::Pubkey,
        signature::Signer,
        sysvar::{last_restart_slot, last_restart_slot::LastRestartSlot, Sysvar},
        transaction::Transaction,
    },
};

// program to check both syscall and sysvar
fn sysvar_last_restart_slot_process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    input: &[u8],
) -> ProgramResult {
    msg!("sysvar_last_restart_slot");
    assert_eq!(input.len(), 8);
    let expected_last_hardfork_slot = u64::from_le_bytes(input[0..8].try_into().unwrap());

    let last_restart_slot = LastRestartSlot::get();
    msg!("last restart slot: {:?}", last_restart_slot);
    assert_eq!(
        last_restart_slot,
        Ok(LastRestartSlot {
            last_restart_slot: expected_last_hardfork_slot
        })
    );

    let last_restart_slot_account = &accounts[0];
    let slot_via_account = LastRestartSlot::from_account_info(last_restart_slot_account)?;
    msg!("slot via account: {:?}", slot_via_account);

    assert_eq!(
        slot_via_account,
        LastRestartSlot {
            last_restart_slot: expected_last_hardfork_slot
        }
    );

    Ok(())
}

async fn check_with_program(
    context: &mut ProgramTestContext,
    program_id: Pubkey,
    expected_last_restart_slot: u64,
) {
    let instructions = vec![Instruction::new_with_bincode(
        program_id,
        &expected_last_restart_slot.to_le_bytes(),
        vec![AccountMeta::new(last_restart_slot::id(), false)],
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

#[tokio::test]
async fn get_sysvar_last_restart_slot() {
    let program_id = Pubkey::new_unique();
    let program_test = ProgramTest::new(
        "sysvar_last_restart_slot_process",
        program_id,
        processor!(sysvar_last_restart_slot_process_instruction),
    );

    let mut context = program_test.start_with_context().await;

    check_with_program(&mut context, program_id, 0).await;
    context.warp_to_slot(40).unwrap();
    context.register_hard_fork(41 as Slot);
    check_with_program(&mut context, program_id, 0).await;
    context.warp_to_slot(41).unwrap();
    check_with_program(&mut context, program_id, 41).await;
    // check for value lower than previous hardfork
    context.register_hard_fork(40 as Slot);
    context.warp_to_slot(45).unwrap();
    check_with_program(&mut context, program_id, 41).await;
    context.register_hard_fork(47 as Slot);
    context.register_hard_fork(48 as Slot);
    context.warp_to_slot(46).unwrap();
    check_with_program(&mut context, program_id, 41).await;
    context.register_hard_fork(50 as Slot);
    context.warp_to_slot(48).unwrap();
    check_with_program(&mut context, program_id, 48).await;
    context.warp_to_slot(50).unwrap();
    check_with_program(&mut context, program_id, 50).await;
}
