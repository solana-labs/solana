use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey, rent::Rent,
};
use solana_program_test::{processor, ProgramTest, ProgramTestState};
use solana_sdk::{
    signature::Keypair, signature::Signer, system_instruction, transaction::Transaction,
};

// Dummy process instruction required to instantiate ProgramTest
fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _input: &[u8],
) -> ProgramResult {
    Ok(())
}

#[test]
fn simulate_fuzz() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let program_id = Pubkey::new_unique();
    // Initialize and start the test network
    let program_test = ProgramTest::new(
        "program-test-fuzz",
        program_id,
        processor!(process_instruction),
    );

    let mut test_state = rt.block_on(async { program_test.start().await });

    // the honggfuzz `fuzz!` macro does not allow for async closures,
    // so we have to use the runtime directly to run async functions
    rt.block_on(async {
        run_fuzz_instructions(&[1, 2, 3, 4, 5], &mut test_state, &program_id).await
    });
}

async fn run_fuzz_instructions(
    fuzz_instruction: &[u8],
    test_state: &mut ProgramTestState,
    program_id: &Pubkey,
) {
    let mut instructions = vec![];
    let mut signer_keypairs = vec![];
    for &i in fuzz_instruction {
        let keypair = Keypair::new();
        let instruction = system_instruction::create_account(
            &test_state.payer.pubkey(),
            &keypair.pubkey(),
            Rent::default().minimum_balance(i as usize),
            i as u64,
            program_id,
        );
        instructions.push(instruction);
        signer_keypairs.push(keypair);
    }
    // Process transaction on test network
    let mut transaction =
        Transaction::new_with_payer(&instructions, Some(&test_state.payer.pubkey()));
    let signers = [&test_state.payer]
        .iter()
        .copied()
        .chain(signer_keypairs.iter())
        .collect::<Vec<&Keypair>>();
    transaction.partial_sign(&signers, test_state.last_blockhash);

    test_state
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap_or_else(|e| {
            print!("{:?}", e);
        });
    for keypair in signer_keypairs {
        let account = test_state
            .banks_client
            .get_account(keypair.pubkey())
            .await
            .expect("account exists")
            .unwrap();
        assert!(account.lamports > 0);
        assert!(!account.data.is_empty());
    }
}
