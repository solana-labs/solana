use {
    solana_program_test::{programs::spl_programs, ProgramTest},
    solana_sdk::bpf_loader,
};

#[tokio::test]
async fn programs_present() {
    let (mut banks_client, _, _) = ProgramTest::default().start().await;
    let rent = banks_client.get_rent().await.unwrap();

    for (program_id, _) in spl_programs(&rent) {
        let program_account = banks_client.get_account(program_id).await.unwrap().unwrap();

        assert_eq!(program_account.owner, bpf_loader::id());
    }
}
