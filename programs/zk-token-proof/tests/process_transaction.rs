use {
    solana_program_test::*,
    solana_sdk::{
        instruction::InstructionError,
        signature::Signer,
        signer::keypair::Keypair,
        system_instruction,
        transaction::{Transaction, TransactionError},
    },
    solana_zk_token_sdk::{
        encryption::elgamal::ElGamalKeypair, instruction::*,
        zk_token_proof_instruction::*, zk_token_proof_program,
    },
};

fn close_account_test_data() -> (ProofType, usize, CloseAccountData, CloseAccountData) {
    let elgamal_keypair = ElGamalKeypair::new_rand();

    let zero_ciphertext = elgamal_keypair.public.encrypt(0_u64);
    let success_data = CloseAccountData::new(&elgamal_keypair, &zero_ciphertext).unwrap();

    let non_zero_ciphertext = elgamal_keypair.public.encrypt(1_u64);
    let fail_data = CloseAccountData::new(&elgamal_keypair, &non_zero_ciphertext).unwrap();

    (ProofType::CloseAccount, ProofContextState::<CloseAccountProofContext>::size(), success_data, fail_data)
}

#[tokio::test]
async fn test_verify_proof_without_context() {
    let mut context = ProgramTest::default().start_with_context().await;

    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;

    let test_proof_data = vec![
        close_account_test_data(),
    ];

    for (proof_type, _, success_data, fail_data) in test_proof_data {
        // verify a valid proof (wihtout creating a context account)
        let instructions = vec![
            verify_proof(
                proof_type,
                None,
                &success_data,
            )
        ];
        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        );
        client.process_transaction(transaction).await.unwrap();

        // try to verify an invalid proof (without creating a context account)
        let instructions = vec![
            verify_proof(
                proof_type,
                None,
                &fail_data,
            )
        ];
        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        );
        let err = client.process_transaction(transaction).await.unwrap_err().unwrap();
        assert_eq!(err, TransactionError::InstructionError(0, InstructionError::InvalidInstructionData));
    }
}

#[tokio::test]
async fn test_verify_proof_with_context() {
    let mut context = ProgramTest::default().start_with_context().await;
    let rent = context.banks_client.get_rent().await.unwrap();

    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;

    let context_state_account = Keypair::new();
    let context_state_authority = Keypair::new();

    let context_state_info = ContextStateInfo {
        context_state_account: &context_state_account.pubkey(),
        context_state_authority: &context_state_authority.pubkey(),
    };

    let test_proof_data = vec![
        close_account_test_data(),
    ];

    for (proof_type, space, success_data, fail_data) in test_proof_data {
        // try to create proof context state with an invalid proof
        let instructions = vec![
            system_instruction::create_account(
                &payer.pubkey(),
                &context_state_account.pubkey(),
                rent.minimum_balance(space),
                space as u64,
                &zk_token_proof_program::id(),
            ),
            verify_proof(
                proof_type,
                Some(context_state_info),
                &fail_data,
            ),
        ];
        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&payer.pubkey()),
            &[payer, &context_state_account],
            recent_blockhash,
        );
        let err = client.process_transaction(transaction).await.unwrap_err().unwrap();
        assert_eq!(err, TransactionError::InstructionError(1, InstructionError::InvalidInstructionData));

        // try to create proof context state with incorrect account data length
        let instructions = vec![
            system_instruction::create_account(
                &payer.pubkey(),
                &context_state_account.pubkey(),
                rent.minimum_balance(space),
                (space-1) as u64,
                &zk_token_proof_program::id(),
            ),
            verify_proof(
                proof_type,
                Some(context_state_info),
                &success_data,
            ),
        ];
        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&payer.pubkey()),
            &[payer, &context_state_account],
            recent_blockhash,
        );
        let err = client.process_transaction(transaction).await.unwrap_err().unwrap();
        assert_eq!(err, TransactionError::InstructionError(1, InstructionError::InvalidAccountData));

        // try to create proof context state with insufficient rent
        let instructions = vec![
            system_instruction::create_account(
                &payer.pubkey(),
                &context_state_account.pubkey(),
                rent.minimum_balance(space) - 1,
                space as u64,
                &zk_token_proof_program::id(),
            ),
            verify_proof(
                proof_type,
                Some(context_state_info),
                &success_data,
            ),
        ];
        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&payer.pubkey()),
            &[payer, &context_state_account],
            recent_blockhash,
        );
        let err = client.process_transaction(transaction).await.unwrap_err().unwrap();
        assert_eq!(err, TransactionError::InstructionError(1, InstructionError::InsufficientFunds));

        // successfully create a proof context state
        let instructions = vec![
            system_instruction::create_account(
                &payer.pubkey(),
                &context_state_account.pubkey(),
                rent.minimum_balance(space),
                space as u64,
                &zk_token_proof_program::id(),
            ),
            verify_proof(
                proof_type,
                Some(context_state_info),
                &success_data,
            ),
        ];
        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&payer.pubkey()),
            &[payer, &context_state_account],
            recent_blockhash,
        );
        client.process_transaction(transaction).await.unwrap();
    }
}

#[tokio::test]
async fn test_close_context_state() {
    let mut context = ProgramTest::default().start_with_context().await;
    let rent = context.banks_client.get_rent().await.unwrap();

    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;

    let context_state_account = Keypair::new();
    let context_state_authority = Keypair::new();

    let context_state_info = ContextStateInfo {
        context_state_account: &context_state_account.pubkey(),
        context_state_authority: &context_state_authority.pubkey(),
    };

    let destination_account = Keypair::new();

    let test_proof_data = vec![
        close_account_test_data(),
    ];

    for (proof_type, space, success_data, _) in test_proof_data {
        // create a proof context state
        let instructions = vec![
            system_instruction::create_account(
                &payer.pubkey(),
                &context_state_account.pubkey(),
                rent.minimum_balance(space),
                space as u64,
                &zk_token_proof_program::id(),
            ),
            verify_proof(
                proof_type,
                Some(context_state_info),
                &success_data,
            ),
        ];
        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&payer.pubkey()),
            &[payer, &context_state_account],
            recent_blockhash,
        );
        client.process_transaction(transaction).await.unwrap();

        // try to close context state with incorrect authority
        let incorrect_authority = Keypair::new();
        let instruction = close_context_state(
            &context_state_account.pubkey(), 
            &destination_account.pubkey(), 
            &incorrect_authority.pubkey(),
            proof_type,
        );
        let transaction = Transaction::new_signed_with_payer(
            &[instruction],
            Some(&payer.pubkey()),
            &[payer, &incorrect_authority],
            recent_blockhash,
        );
        let err = client.process_transaction(transaction).await.unwrap_err().unwrap();
        assert_eq!(err, TransactionError::InstructionError(0, InstructionError::InvalidAccountOwner));

        // successfully close proof context state
        let instruction = close_context_state(
            &context_state_account.pubkey(), 
            &destination_account.pubkey(), 
            &context_state_authority.pubkey(),
            proof_type,
        );
        let transaction = Transaction::new_signed_with_payer(
            &[instruction.clone()],
            Some(&payer.pubkey()),
            &[payer, &context_state_authority],
            recent_blockhash,
        );
        client.process_transaction(transaction).await.unwrap();
    }
}

