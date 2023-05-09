use {
    bytemuck::Pod,
    solana_program_test::*,
    solana_sdk::{
        instruction::InstructionError,
        signature::Signer,
        signer::keypair::Keypair,
        system_instruction,
        transaction::{Transaction, TransactionError},
    },
    solana_zk_token_sdk::{
        encryption::elgamal::ElGamalKeypair, instruction::*, zk_token_proof_instruction::*,
        zk_token_proof_program, zk_token_proof_state::ProofContextState,
    },
    std::mem::size_of,
};

const VERIFY_INSTRUCTION_TYPES: [ProofInstruction; 6] = [
    ProofInstruction::VerifyCloseAccount,
    ProofInstruction::VerifyWithdraw,
    ProofInstruction::VerifyWithdrawWithheldTokens,
    ProofInstruction::VerifyTransfer,
    ProofInstruction::VerifyTransferWithFee,
    ProofInstruction::VerifyPubkeyValidity,
];

#[tokio::test]
async fn test_close_account() {
    let elgamal_keypair = ElGamalKeypair::new_rand();

    let zero_ciphertext = elgamal_keypair.public.encrypt(0_u64);
    let success_proof_data = CloseAccountData::new(&elgamal_keypair, &zero_ciphertext).unwrap();

    let incorrect_keypair = ElGamalKeypair {
        public: ElGamalKeypair::new_rand().public,
        secret: ElGamalKeypair::new_rand().secret,
    };
    let fail_proof_data = CloseAccountData::new(&incorrect_keypair, &zero_ciphertext).unwrap();

    test_verify_proof_without_context(
        ProofInstruction::VerifyCloseAccount,
        &success_proof_data,
        &fail_proof_data,
    )
    .await;

    test_verify_proof_with_context(
        ProofInstruction::VerifyCloseAccount,
        size_of::<ProofContextState<CloseAccountProofContext>>(),
        &success_proof_data,
        &fail_proof_data,
    )
    .await;

    test_close_context_state(
        ProofInstruction::VerifyCloseAccount,
        size_of::<ProofContextState<CloseAccountProofContext>>(),
        &success_proof_data,
    )
    .await;
}

#[tokio::test]
async fn test_withdraw_withheld_tokens() {
    let elgamal_keypair = ElGamalKeypair::new_rand();
    let destination_keypair = ElGamalKeypair::new_rand();

    let amount: u64 = 0;
    let withdraw_withheld_authority_ciphertext = elgamal_keypair.public.encrypt(amount);

    let success_proof_data = WithdrawWithheldTokensData::new(
        &elgamal_keypair,
        &destination_keypair.public,
        &withdraw_withheld_authority_ciphertext,
        amount,
    )
    .unwrap();

    let incorrect_keypair = ElGamalKeypair {
        public: ElGamalKeypair::new_rand().public,
        secret: ElGamalKeypair::new_rand().secret,
    };
    let fail_proof_data = WithdrawWithheldTokensData::new(
        &incorrect_keypair,
        &destination_keypair.public,
        &withdraw_withheld_authority_ciphertext,
        amount,
    )
    .unwrap();

    test_verify_proof_without_context(
        ProofInstruction::VerifyWithdrawWithheldTokens,
        &success_proof_data,
        &fail_proof_data,
    )
    .await;

    test_verify_proof_with_context(
        ProofInstruction::VerifyWithdrawWithheldTokens,
        size_of::<ProofContextState<WithdrawWithheldTokensProofContext>>(),
        &success_proof_data,
        &fail_proof_data,
    )
    .await;

    test_close_context_state(
        ProofInstruction::VerifyWithdrawWithheldTokens,
        size_of::<ProofContextState<WithdrawWithheldTokensProofContext>>(),
        &success_proof_data,
    )
    .await;
}

#[tokio::test]
async fn test_transfer() {
    let source_keypair = ElGamalKeypair::new_rand();
    let dest_pubkey = ElGamalKeypair::new_rand().public;
    let auditor_pubkey = ElGamalKeypair::new_rand().public;

    let spendable_balance: u64 = 0;
    let spendable_ciphertext = source_keypair.public.encrypt(spendable_balance);

    let transfer_amount: u64 = 0;

    let success_proof_data = TransferData::new(
        transfer_amount,
        (spendable_balance, &spendable_ciphertext),
        &source_keypair,
        (&dest_pubkey, &auditor_pubkey),
    )
    .unwrap();

    let incorrect_keypair = ElGamalKeypair {
        public: ElGamalKeypair::new_rand().public,
        secret: ElGamalKeypair::new_rand().secret,
    };

    let fail_proof_data = TransferData::new(
        transfer_amount,
        (spendable_balance, &spendable_ciphertext),
        &incorrect_keypair,
        (&dest_pubkey, &auditor_pubkey),
    )
    .unwrap();

    test_verify_proof_without_context(
        ProofInstruction::VerifyTransfer,
        &success_proof_data,
        &fail_proof_data,
    )
    .await;

    test_verify_proof_with_context(
        ProofInstruction::VerifyTransfer,
        size_of::<ProofContextState<TransferProofContext>>(),
        &success_proof_data,
        &fail_proof_data,
    )
    .await;

    test_close_context_state(
        ProofInstruction::VerifyTransfer,
        size_of::<ProofContextState<TransferProofContext>>(),
        &success_proof_data,
    )
    .await;
}

#[tokio::test]
async fn test_transfer_with_fee() {
    let source_keypair = ElGamalKeypair::new_rand();
    let destination_pubkey = ElGamalKeypair::new_rand().public;
    let auditor_pubkey = ElGamalKeypair::new_rand().public;
    let withdraw_withheld_authority_pubkey = ElGamalKeypair::new_rand().public;

    let spendable_balance: u64 = 120;
    let spendable_ciphertext = source_keypair.public.encrypt(spendable_balance);

    let transfer_amount: u64 = 0;

    let fee_parameters = FeeParameters {
        fee_rate_basis_points: 400,
        maximum_fee: 3,
    };

    let success_proof_data = TransferWithFeeData::new(
        transfer_amount,
        (spendable_balance, &spendable_ciphertext),
        &source_keypair,
        (&destination_pubkey, &auditor_pubkey),
        fee_parameters,
        &withdraw_withheld_authority_pubkey,
    )
    .unwrap();

    let incorrect_keypair = ElGamalKeypair {
        public: ElGamalKeypair::new_rand().public,
        secret: ElGamalKeypair::new_rand().secret,
    };

    let fail_proof_data = TransferWithFeeData::new(
        transfer_amount,
        (spendable_balance, &spendable_ciphertext),
        &incorrect_keypair,
        (&destination_pubkey, &auditor_pubkey),
        fee_parameters,
        &withdraw_withheld_authority_pubkey,
    )
    .unwrap();

    test_verify_proof_without_context(
        ProofInstruction::VerifyTransferWithFee,
        &success_proof_data,
        &fail_proof_data,
    )
    .await;

    test_verify_proof_with_context(
        ProofInstruction::VerifyTransferWithFee,
        size_of::<ProofContextState<TransferWithFeeProofContext>>(),
        &success_proof_data,
        &fail_proof_data,
    )
    .await;

    test_close_context_state(
        ProofInstruction::VerifyTransferWithFee,
        size_of::<ProofContextState<TransferWithFeeProofContext>>(),
        &success_proof_data,
    )
    .await;
}

#[tokio::test]
async fn test_withdraw() {
    let elgamal_keypair = ElGamalKeypair::new_rand();

    let current_balance: u64 = 77;
    let current_ciphertext = elgamal_keypair.public.encrypt(current_balance);
    let withdraw_amount: u64 = 55;

    let success_proof_data = WithdrawData::new(
        withdraw_amount,
        &elgamal_keypair,
        current_balance,
        &current_ciphertext,
    )
    .unwrap();

    let incorrect_keypair = ElGamalKeypair {
        public: ElGamalKeypair::new_rand().public,
        secret: ElGamalKeypair::new_rand().secret,
    };
    let fail_proof_data = WithdrawData::new(
        withdraw_amount,
        &incorrect_keypair,
        current_balance,
        &current_ciphertext,
    )
    .unwrap();

    test_verify_proof_without_context(
        ProofInstruction::VerifyWithdraw,
        &success_proof_data,
        &fail_proof_data,
    )
    .await;

    test_verify_proof_with_context(
        ProofInstruction::VerifyWithdraw,
        size_of::<ProofContextState<WithdrawProofContext>>(),
        &success_proof_data,
        &fail_proof_data,
    )
    .await;

    test_close_context_state(
        ProofInstruction::VerifyWithdraw,
        size_of::<ProofContextState<WithdrawProofContext>>(),
        &success_proof_data,
    )
    .await;
}

#[tokio::test]
async fn test_pubkey_validity() {
    let elgamal_keypair = ElGamalKeypair::new_rand();

    let success_proof_data = PubkeyValidityData::new(&elgamal_keypair).unwrap();

    let incorrect_keypair = ElGamalKeypair {
        public: ElGamalKeypair::new_rand().public,
        secret: ElGamalKeypair::new_rand().secret,
    };

    let fail_proof_data = PubkeyValidityData::new(&incorrect_keypair).unwrap();

    test_verify_proof_without_context(
        ProofInstruction::VerifyPubkeyValidity,
        &success_proof_data,
        &fail_proof_data,
    )
    .await;

    test_verify_proof_with_context(
        ProofInstruction::VerifyPubkeyValidity,
        size_of::<ProofContextState<PubkeyValidityProofContext>>(),
        &success_proof_data,
        &fail_proof_data,
    )
    .await;

    test_close_context_state(
        ProofInstruction::VerifyPubkeyValidity,
        size_of::<ProofContextState<PubkeyValidityProofContext>>(),
        &success_proof_data,
    )
    .await;
}

async fn test_verify_proof_without_context<T, U>(
    proof_instruction: ProofInstruction,
    success_proof_data: &T,
    fail_proof_data: &T,
) where
    T: Pod + ZkProofData<U>,
    U: Pod,
{
    let mut context = ProgramTest::default().start_with_context().await;

    let client = &mut context.banks_client;
    let payer = &context.payer;
    let recent_blockhash = context.last_blockhash;

    // verify a valid proof (wihtout creating a context account)
    let instructions = vec![proof_instruction.encode_verify_proof(None, success_proof_data)];
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    client.process_transaction(transaction).await.unwrap();

    // try to verify an invalid proof (without creating a context account)
    let instructions = vec![proof_instruction.encode_verify_proof(None, fail_proof_data)];
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    let err = client
        .process_transaction(transaction)
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(
        err,
        TransactionError::InstructionError(0, InstructionError::InvalidInstructionData)
    );

    // try to verify a valid proof, but with a wrong proof type
    for wrong_instruction_type in VERIFY_INSTRUCTION_TYPES {
        if proof_instruction == wrong_instruction_type {
            continue;
        }

        let instruction =
            vec![wrong_instruction_type.encode_verify_proof(None, success_proof_data)];
        let transaction = Transaction::new_signed_with_payer(
            &instruction,
            Some(&payer.pubkey()),
            &[payer],
            recent_blockhash,
        );
        let err = client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap();
        assert_eq!(
            err,
            TransactionError::InstructionError(0, InstructionError::InvalidInstructionData)
        );
    }
}

async fn test_verify_proof_with_context<T, U>(
    instruction_type: ProofInstruction,
    space: usize,
    success_proof_data: &T,
    fail_proof_data: &T,
) where
    T: Pod + ZkProofData<U>,
    U: Pod,
{
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

    // try to create proof context state with an invalid proof
    let instructions = vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &context_state_account.pubkey(),
            rent.minimum_balance(space),
            space as u64,
            &zk_token_proof_program::id(),
        ),
        instruction_type.encode_verify_proof(Some(context_state_info), fail_proof_data),
    ];
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer, &context_state_account],
        recent_blockhash,
    );
    let err = client
        .process_transaction(transaction)
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(
        err,
        TransactionError::InstructionError(1, InstructionError::InvalidInstructionData)
    );

    // try to create proof context state with incorrect account data length
    let instructions = vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &context_state_account.pubkey(),
            rent.minimum_balance(space),
            (space.checked_sub(1).unwrap()) as u64,
            &zk_token_proof_program::id(),
        ),
        instruction_type.encode_verify_proof(Some(context_state_info), success_proof_data),
    ];
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer, &context_state_account],
        recent_blockhash,
    );
    let err = client
        .process_transaction(transaction)
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(
        err,
        TransactionError::InstructionError(1, InstructionError::InvalidAccountData)
    );

    // try to create proof context state with insufficient rent
    let instructions = vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &context_state_account.pubkey(),
            rent.minimum_balance(space).checked_sub(1).unwrap(),
            space as u64,
            &zk_token_proof_program::id(),
        ),
        instruction_type.encode_verify_proof(Some(context_state_info), success_proof_data),
    ];
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer, &context_state_account],
        recent_blockhash,
    );
    let err = client
        .process_transaction(transaction)
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(
        err,
        TransactionError::InsufficientFundsForRent { account_index: 1 },
    );

    // try to create proof context state with an invalid `ProofType`
    for wrong_instruction_type in VERIFY_INSTRUCTION_TYPES {
        if instruction_type == wrong_instruction_type {
            continue;
        }

        let instructions = vec![
            system_instruction::create_account(
                &payer.pubkey(),
                &context_state_account.pubkey(),
                rent.minimum_balance(space),
                space as u64,
                &zk_token_proof_program::id(),
            ),
            wrong_instruction_type
                .encode_verify_proof(Some(context_state_info), success_proof_data),
        ];
        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&payer.pubkey()),
            &[payer, &context_state_account],
            recent_blockhash,
        );
        let err = client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap();
        assert_eq!(
            err,
            TransactionError::InstructionError(1, InstructionError::InvalidInstructionData)
        );
    }

    // successfully create a proof context state
    let instructions = vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &context_state_account.pubkey(),
            rent.minimum_balance(space),
            space as u64,
            &zk_token_proof_program::id(),
        ),
        instruction_type.encode_verify_proof(Some(context_state_info), success_proof_data),
    ];
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer, &context_state_account],
        recent_blockhash,
    );
    client.process_transaction(transaction).await.unwrap();

    // try overwriting the context state
    let instructions =
        vec![instruction_type.encode_verify_proof(Some(context_state_info), success_proof_data)];
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );
    let err = client
        .process_transaction(transaction)
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(
        err,
        TransactionError::InstructionError(0, InstructionError::AccountAlreadyInitialized)
    );

    // self-owned context state account
    let context_state_account_and_authority = Keypair::new();
    let context_state_info = ContextStateInfo {
        context_state_account: &context_state_account_and_authority.pubkey(),
        context_state_authority: &context_state_account_and_authority.pubkey(),
    };

    let instructions = vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &context_state_account_and_authority.pubkey(),
            rent.minimum_balance(space),
            space as u64,
            &zk_token_proof_program::id(),
        ),
        instruction_type.encode_verify_proof(Some(context_state_info), success_proof_data),
    ];
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer, &context_state_account_and_authority],
        recent_blockhash,
    );
    client.process_transaction(transaction).await.unwrap();
}

async fn test_close_context_state<T, U>(
    instruction_type: ProofInstruction,
    space: usize,
    success_proof_data: &T,
) where
    T: Pod + ZkProofData<U>,
    U: Pod,
{
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

    // create a proof context state
    let instructions = vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &context_state_account.pubkey(),
            rent.minimum_balance(space),
            space as u64,
            &zk_token_proof_program::id(),
        ),
        instruction_type.encode_verify_proof(Some(context_state_info), success_proof_data),
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
        ContextStateInfo {
            context_state_account: &context_state_account.pubkey(),
            context_state_authority: &incorrect_authority.pubkey(),
        },
        &destination_account.pubkey(),
    );
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&payer.pubkey()),
        &[payer, &incorrect_authority],
        recent_blockhash,
    );
    let err = client
        .process_transaction(transaction)
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(
        err,
        TransactionError::InstructionError(0, InstructionError::InvalidAccountOwner)
    );

    // successfully close proof context state
    let instruction = close_context_state(
        ContextStateInfo {
            context_state_account: &context_state_account.pubkey(),
            context_state_authority: &context_state_authority.pubkey(),
        },
        &destination_account.pubkey(),
    );
    let transaction = Transaction::new_signed_with_payer(
        &[instruction.clone()],
        Some(&payer.pubkey()),
        &[payer, &context_state_authority],
        recent_blockhash,
    );
    client.process_transaction(transaction).await.unwrap();

    // create and close proof context in a single transaction
    let instructions = vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &context_state_account.pubkey(),
            0_u64, // do not deposit rent
            space as u64,
            &zk_token_proof_program::id(),
        ),
        instruction_type.encode_verify_proof(Some(context_state_info), success_proof_data),
        close_context_state(
            ContextStateInfo {
                context_state_account: &context_state_account.pubkey(),
                context_state_authority: &context_state_authority.pubkey(),
            },
            &destination_account.pubkey(),
        ),
    ];
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer, &context_state_account, &context_state_authority],
        recent_blockhash,
    );
    client.process_transaction(transaction).await.unwrap();

    // close proof context state with owner as destination
    let instructions = vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &context_state_account.pubkey(),
            0_u64,
            space as u64,
            &zk_token_proof_program::id(),
        ),
        instruction_type.encode_verify_proof(Some(context_state_info), success_proof_data),
        close_context_state(
            ContextStateInfo {
                context_state_account: &context_state_account.pubkey(),
                context_state_authority: &context_state_authority.pubkey(),
            },
            &context_state_authority.pubkey(),
        ),
    ];
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer, &context_state_account, &context_state_authority],
        recent_blockhash,
    );
    client.process_transaction(transaction).await.unwrap();

    // try close account with itself as destination
    let instructions = vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &context_state_account.pubkey(),
            0_u64,
            space as u64,
            &zk_token_proof_program::id(),
        ),
        instruction_type.encode_verify_proof(Some(context_state_info), success_proof_data),
        close_context_state(
            ContextStateInfo {
                context_state_account: &context_state_account.pubkey(),
                context_state_authority: &context_state_authority.pubkey(),
            },
            &context_state_account.pubkey(),
        ),
    ];
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer, &context_state_account, &context_state_authority],
        recent_blockhash,
    );
    let err = client
        .process_transaction(transaction)
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(
        err,
        TransactionError::InstructionError(2, InstructionError::InvalidInstructionData)
    );

    // close self-owned proof context accounts
    let context_state_account_and_authority = Keypair::new();
    let context_state_info = ContextStateInfo {
        context_state_account: &context_state_account_and_authority.pubkey(),
        context_state_authority: &context_state_account_and_authority.pubkey(),
    };

    let instructions = vec![
        system_instruction::create_account(
            &payer.pubkey(),
            &context_state_account_and_authority.pubkey(),
            0_u64,
            space as u64,
            &zk_token_proof_program::id(),
        ),
        instruction_type.encode_verify_proof(Some(context_state_info), success_proof_data),
        close_context_state(context_state_info, &context_state_account.pubkey()),
    ];
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[payer, &context_state_account_and_authority],
        recent_blockhash,
    );
    client.process_transaction(transaction).await.unwrap();
}
