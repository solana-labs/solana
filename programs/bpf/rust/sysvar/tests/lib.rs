use {
    solana_bpf_rust_sysvar::process_instruction,
    solana_program_test::*,
    solana_sdk::{
        feature_set::disable_fees_sysvar,
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
        signature::Signer,
        sysvar::{
            clock, epoch_schedule, fees, instructions, recent_blockhashes, rent, slot_hashes,
            slot_history, stake_history,
        },
        transaction::Transaction,
    },
};

#[tokio::test]
async fn test_sysvars() {
    let program_id = Pubkey::new_unique();

    let program_test = ProgramTest::new(
        "solana_bpf_rust_sysvar",
        program_id,
        processor!(process_instruction),
    );
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;

    let mut transaction = Transaction::new_with_payer(
        &[Instruction::new_with_bincode(
            program_id,
            &[0u8],
            vec![
                AccountMeta::new(payer.pubkey(), true),
                AccountMeta::new(Pubkey::new_unique(), false),
                AccountMeta::new_readonly(clock::id(), false),
                AccountMeta::new_readonly(epoch_schedule::id(), false),
                AccountMeta::new_readonly(instructions::id(), false),
                #[allow(deprecated)]
                AccountMeta::new_readonly(recent_blockhashes::id(), false),
                AccountMeta::new_readonly(rent::id(), false),
                AccountMeta::new_readonly(slot_hashes::id(), false),
                AccountMeta::new_readonly(slot_history::id(), false),
                AccountMeta::new_readonly(stake_history::id(), false),
                #[allow(deprecated)]
                AccountMeta::new_readonly(fees::id(), false),
            ],
        )],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[&payer], recent_blockhash);
    banks_client.process_transaction(transaction).await.unwrap();

    let mut program_test = ProgramTest::new(
        "solana_bpf_rust_sysvar",
        program_id,
        processor!(process_instruction),
    );
    program_test.deactivate_feature(disable_fees_sysvar::id());
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;

    let mut transaction = Transaction::new_with_payer(
        &[Instruction::new_with_bincode(
            program_id,
            &[1u8],
            vec![
                AccountMeta::new(payer.pubkey(), true),
                AccountMeta::new(Pubkey::new_unique(), false),
                AccountMeta::new_readonly(clock::id(), false),
                AccountMeta::new_readonly(epoch_schedule::id(), false),
                AccountMeta::new_readonly(instructions::id(), false),
                #[allow(deprecated)]
                AccountMeta::new_readonly(recent_blockhashes::id(), false),
                AccountMeta::new_readonly(rent::id(), false),
                AccountMeta::new_readonly(slot_hashes::id(), false),
                AccountMeta::new_readonly(slot_history::id(), false),
                AccountMeta::new_readonly(stake_history::id(), false),
                #[allow(deprecated)]
                AccountMeta::new_readonly(fees::id(), false),
            ],
        )],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[&payer], recent_blockhash);
    banks_client.process_transaction(transaction).await.unwrap();
}
