use {
    solana_program_test::ProgramTest,
    solana_sdk::{
        account::Account,
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        instruction::AccountMeta,
        instruction::Instruction,
        pubkey::Pubkey,
        signature::Signer,
        transaction::Transaction,
    },
    std::mem,
};

#[tokio::test]
async fn add_program() {
    let program_id = Pubkey::new_unique();
    let greeted_pubkey = Pubkey::new_unique();

    let mut program_test = ProgramTest::default();
    program_test.prefer_bpf(true);

    program_test.add_program("helloworld", program_id, None);

    // Add greeting account
    program_test.add_account(
        greeted_pubkey,
        Account {
            lamports: 5,
            data: vec![0_u8; mem::size_of::<u32>()],
            owner: program_id,
            ..Account::default()
        },
    );

    let mut ctx = program_test.start_with_context().await;

    // Greet once
    let mut transaction = Transaction::new_with_payer(
        &[Instruction::new_with_bincode(
            program_id,
            &[0], // ignored but makes the instruction unique in the slot
            vec![AccountMeta::new(greeted_pubkey, false)],
        )],
        Some(&ctx.payer.pubkey()),
    );
    transaction.sign(&[&ctx.payer], ctx.last_blockhash);
    ctx.banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    // Verify account has one greeting
    let greeted_account = ctx
        .banks_client
        .get_account(greeted_pubkey)
        .await
        .expect("get_account")
        .expect("greeted_account not found");

    assert_eq!(greeted_account.data, vec![1, 0, 0, 0]);

    // Verify programdata address
    let (programdata_address, _) =
        Pubkey::find_program_address(&[program_id.as_ref()], &bpf_loader_upgradeable::ID);

    let program_acc = ctx
        .banks_client
        .get_account(program_id)
        .await
        .expect("get_account")
        .expect("program account not found");
    let program: UpgradeableLoaderState =
        bincode::deserialize(&program_acc.data).expect("deserializing program data");
    assert_eq!(
        program,
        UpgradeableLoaderState::Program {
            programdata_address,
        }
    );

    // Verify upgrade authority
    let programdata_acc = ctx
        .banks_client
        .get_account(programdata_address)
        .await
        .expect("get_account")
        .expect("programdata account not found");
    let programdata: UpgradeableLoaderState =
        bincode::deserialize(&programdata_acc.data).expect("deserializing program data");
    assert_eq!(
        programdata,
        UpgradeableLoaderState::ProgramData {
            slot: 1,
            upgrade_authority_address: Some(ctx.payer.pubkey()),
        }
    );
}
