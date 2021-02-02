use {
    solana_banks_client::BanksClient,
    solana_program::{
        account_info::{next_account_info, AccountInfo},
        entrypoint::ProgramResult,
        hash::Hash,
        instruction::{AccountMeta, Instruction},
        msg,
        program::invoke,
        pubkey::Pubkey,
        rent::Rent,
    },
    solana_program_test::{processor, ProgramTest},
    solana_sdk::{
        signature::Keypair, signature::Signer, system_instruction, transaction::Transaction,
    },
};

// Process instruction to invoke into another program
fn invoker_process_instruction(
    _program_id: &Pubkey,
    accounts: &[AccountInfo],
    _input: &[u8],
) -> ProgramResult {
    // if we can call `msg!` successfully, then InvokeContext exists as required
    msg!("Processing invoker instruction before CPI");
    let account_info_iter = &mut accounts.iter();
    let invoked_program_info = next_account_info(account_info_iter)?;
    invoke(
        &Instruction::new(*invoked_program_info.key, &[0], vec![]),
        &[invoked_program_info.clone()],
    )?;
    msg!("Processing invoker instruction after CPI");
    Ok(())
}

// Process instruction to be invoked by another program
#[allow(clippy::unnecessary_wraps)]
fn invoked_process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _input: &[u8],
) -> ProgramResult {
    // if we can call `msg!` successfully, then InvokeContext exists as required
    msg!("Processing invoked instruction");
    Ok(())
}

#[test]
fn simulate_fuzz() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let invoker_program_id = Pubkey::new_unique();
    // Initialize and start the test network
    let mut program_test = ProgramTest::new(
        "program-test-fuzz-invoker",
        invoker_program_id,
        processor!(invoker_process_instruction),
    );
    let invoked_program_id = Pubkey::new_unique();
    program_test.add_program(
        "program-test-fuzz-invoked",
        invoked_program_id,
        processor!(invoked_process_instruction),
    );

    let (mut banks_client, payer, last_blockhash) =
        rt.block_on(async { program_test.start().await });

    // the honggfuzz `fuzz!` macro does not allow for async closures,
    // so we have to use the runtime directly to run async functions
    rt.block_on(async {
        run_fuzz_instructions(
            &[1, 2, 3, 4, 5],
            &mut banks_client,
            &payer,
            last_blockhash,
            &invoker_program_id,
            &invoked_program_id,
        )
        .await
    });
}

#[test]
fn simulate_fuzz_with_context() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let invoker_program_id = Pubkey::new_unique();
    // Initialize and start the test network
    let mut program_test = ProgramTest::new(
        "program-test-fuzz-invoker",
        invoker_program_id,
        processor!(invoker_process_instruction),
    );
    let invoked_program_id = Pubkey::new_unique();
    program_test.add_program(
        "program-test-fuzz-invoked",
        invoked_program_id,
        processor!(invoked_process_instruction),
    );

    let mut test_state = rt.block_on(async { program_test.start_with_context().await });

    // the honggfuzz `fuzz!` macro does not allow for async closures,
    // so we have to use the runtime directly to run async functions
    rt.block_on(async {
        run_fuzz_instructions(
            &[1, 2, 3, 4, 5],
            &mut test_state.banks_client,
            &test_state.payer,
            test_state.last_blockhash,
            &invoker_program_id,
            &invoked_program_id,
        )
        .await
    });
}

async fn run_fuzz_instructions(
    fuzz_instruction: &[u8],
    banks_client: &mut BanksClient,
    payer: &Keypair,
    last_blockhash: Hash,
    invoker_program_id: &Pubkey,
    invoked_program_id: &Pubkey,
) {
    let mut instructions = vec![];
    let mut signer_keypairs = vec![];
    for &i in fuzz_instruction {
        let keypair = Keypair::new();
        let instruction = system_instruction::create_account(
            &payer.pubkey(),
            &keypair.pubkey(),
            Rent::default().minimum_balance(i as usize),
            i as u64,
            invoker_program_id,
        );
        instructions.push(instruction);
        instructions.push(Instruction::new(
            *invoker_program_id,
            &[0],
            vec![AccountMeta::new_readonly(*invoked_program_id, false)],
        ));
        signer_keypairs.push(keypair);
    }
    // Process transaction on test network
    let mut transaction = Transaction::new_with_payer(&instructions, Some(&payer.pubkey()));
    let signers = [payer]
        .iter()
        .copied()
        .chain(signer_keypairs.iter())
        .collect::<Vec<&Keypair>>();
    transaction.partial_sign(&signers, last_blockhash);

    banks_client.process_transaction(transaction).await.unwrap();
    for keypair in signer_keypairs {
        let account = banks_client
            .get_account(keypair.pubkey())
            .await
            .expect("account exists")
            .unwrap();
        assert!(account.lamports > 0);
        assert!(!account.data.is_empty());
    }
}
