use crate::{
    args::{DistributeTokensArgs, SplTokenArgs},
    commands::{Allocation, Error, FundingSource},
};
use console::style;
use solana_account_decoder::parse_token::{
    pubkey_from_spl_token_v2_0, spl_token_v2_0_pubkey, token_amount_to_ui_amount,
};
use solana_banks_client::{BanksClient, BanksClientExt};
use solana_sdk::{instruction::Instruction, native_token::lamports_to_sol};
use solana_transaction_status::parse_token::spl_token_v2_0_instruction;
use spl_associated_token_account_v1_0::{
    create_associated_token_account, get_associated_token_address,
};
use spl_token_v2_0::{
    solana_program::program_pack::Pack,
    state::{Account as SplTokenAccount, Mint},
};

pub async fn update_token_args(
    client: &mut BanksClient,
    args: &mut Option<SplTokenArgs>,
) -> Result<(), Error> {
    if let Some(spl_token_args) = args {
        let sender_account = client
            .get_account(spl_token_args.token_account_address)
            .await?
            .unwrap_or_default();
        let mint_address =
            pubkey_from_spl_token_v2_0(&SplTokenAccount::unpack(&sender_account.data)?.mint);
        spl_token_args.mint = mint_address;
        update_decimals(client, args).await?;
    }
    Ok(())
}

pub async fn update_decimals(
    client: &mut BanksClient,
    args: &mut Option<SplTokenArgs>,
) -> Result<(), Error> {
    if let Some(spl_token_args) = args {
        let mint_account = client
            .get_account(spl_token_args.mint)
            .await?
            .unwrap_or_default();
        let mint = Mint::unpack(&mint_account.data)?;
        spl_token_args.decimals = mint.decimals;
    }
    Ok(())
}

pub fn spl_token_amount(amount: f64, decimals: u8) -> u64 {
    (amount * 10_usize.pow(decimals as u32) as f64) as u64
}

pub fn build_spl_token_instructions(
    allocation: &Allocation,
    args: &DistributeTokensArgs,
    do_create_associated_token_account: bool,
) -> Vec<Instruction> {
    let spl_token_args = args
        .spl_token_args
        .as_ref()
        .expect("spl_token_args must be some");
    let wallet_address = allocation.recipient.parse().unwrap();
    let associated_token_address = get_associated_token_address(
        &wallet_address,
        &spl_token_v2_0_pubkey(&spl_token_args.mint),
    );
    let mut instructions = vec![];
    if do_create_associated_token_account {
        let create_associated_token_account_instruction = create_associated_token_account(
            &spl_token_v2_0_pubkey(&args.fee_payer.pubkey()),
            &wallet_address,
            &spl_token_v2_0_pubkey(&spl_token_args.mint),
        );
        instructions.push(spl_token_v2_0_instruction(
            create_associated_token_account_instruction,
        ));
    }
    let spl_instruction = spl_token_v2_0::instruction::transfer_checked(
        &spl_token_v2_0::id(),
        &spl_token_v2_0_pubkey(&spl_token_args.token_account_address),
        &spl_token_v2_0_pubkey(&spl_token_args.mint),
        &associated_token_address,
        &spl_token_v2_0_pubkey(&args.sender_keypair.pubkey()),
        &[],
        allocation.amount,
        spl_token_args.decimals,
    )
    .unwrap();
    instructions.push(spl_token_v2_0_instruction(spl_instruction));
    instructions
}

pub async fn check_spl_token_balances(
    num_signatures: usize,
    allocations: &[Allocation],
    client: &mut BanksClient,
    args: &DistributeTokensArgs,
    created_accounts: u64,
) -> Result<(), Error> {
    let spl_token_args = args
        .spl_token_args
        .as_ref()
        .expect("spl_token_args must be some");
    let allocation_amount: u64 = allocations.iter().map(|x| x.amount).sum();

    let (fee_calculator, _blockhash, _last_valid_slot) = client.get_fees().await?;
    let fees = fee_calculator
        .lamports_per_signature
        .checked_mul(num_signatures as u64)
        .unwrap();

    let rent = client.get_rent().await?;
    let token_account_rent_exempt_balance = rent.minimum_balance(SplTokenAccount::LEN);
    let account_creation_amount = created_accounts * token_account_rent_exempt_balance;
    let fee_payer_balance = client.get_balance(args.fee_payer.pubkey()).await?;
    if fee_payer_balance < fees + account_creation_amount {
        return Err(Error::InsufficientFunds(
            vec![FundingSource::FeePayer].into(),
            lamports_to_sol(fees + account_creation_amount),
        ));
    }
    let source_token_account = client
        .get_account(spl_token_args.token_account_address)
        .await?
        .unwrap_or_default();
    let source_token = SplTokenAccount::unpack(&source_token_account.data)?;
    if source_token.amount < allocation_amount {
        return Err(Error::InsufficientFunds(
            vec![FundingSource::SplTokenAccount].into(),
            token_amount_to_ui_amount(allocation_amount, spl_token_args.decimals).ui_amount,
        ));
    }
    Ok(())
}

pub async fn print_token_balances(
    client: &mut BanksClient,
    allocation: &Allocation,
    spl_token_args: &SplTokenArgs,
) -> Result<(), Error> {
    let address = allocation.recipient.parse().unwrap();
    let expected = allocation.amount;
    let associated_token_address = get_associated_token_address(
        &spl_token_v2_0_pubkey(&address),
        &spl_token_v2_0_pubkey(&spl_token_args.mint),
    );
    let recipient_account = client
        .get_account(pubkey_from_spl_token_v2_0(&associated_token_address))
        .await?
        .unwrap_or_default();
    let (actual, difference) = if let Ok(recipient_token) =
        SplTokenAccount::unpack(&recipient_account.data)
    {
        let actual_ui_amount =
            token_amount_to_ui_amount(recipient_token.amount, spl_token_args.decimals).ui_amount;
        let expected_ui_amount =
            token_amount_to_ui_amount(expected, spl_token_args.decimals).ui_amount;
        (
            style(format!(
                "{:>24.1$}",
                actual_ui_amount, spl_token_args.decimals as usize
            )),
            format!(
                "{:>24.1$}",
                actual_ui_amount - expected_ui_amount,
                spl_token_args.decimals as usize
            ),
        )
    } else {
        (
            style("Associated token account not yet created".to_string()).yellow(),
            "".to_string(),
        )
    };
    println!(
        "{:<44}  {:>24.4$}  {:>24}  {:>24}",
        allocation.recipient,
        token_amount_to_ui_amount(expected, spl_token_args.decimals).ui_amount,
        actual,
        difference,
        spl_token_args.decimals as usize
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        commands::{process_allocations, tests::tmp_file_path, Allocation},
        db::{self, check_output_file},
    };
    use solana_account_decoder::parse_token::{spl_token_id_v2_0, spl_token_v2_0_pubkey};
    use solana_program_test::*;
    use solana_sdk::{
        hash::Hash,
        signature::{read_keypair_file, write_keypair_file, Keypair, Signer},
        system_instruction,
        transaction::Transaction,
    };
    use solana_transaction_status::parse_token::spl_token_v2_0_instruction;
    use spl_associated_token_account_v1_0::{
        create_associated_token_account, get_associated_token_address,
    };
    use spl_token_v2_0::{
        instruction::{initialize_account, initialize_mint, mint_to},
        solana_program::pubkey::Pubkey,
    };
    use tempfile::{tempdir, NamedTempFile};

    fn program_test() -> ProgramTest {
        // Add SPL Associated Token program
        let mut pc = ProgramTest::new(
            "spl_associated_token_account",
            pubkey_from_spl_token_v2_0(&spl_associated_token_account_v1_0::id()),
            None,
        );
        // Add SPL Token program
        pc.add_program("spl_token", spl_token_id_v2_0(), None);
        pc
    }

    async fn initialize_test_mint(
        banks_client: &mut BanksClient,
        fee_payer: &Keypair,
        mint: &Keypair,
        decimals: u8,
        recent_blockhash: Hash,
    ) {
        let rent = banks_client.get_rent().await.unwrap();
        let expected_mint_balance = rent.minimum_balance(Mint::LEN);
        let instructions = vec![
            system_instruction::create_account(
                &fee_payer.pubkey(),
                &mint.pubkey(),
                expected_mint_balance,
                Mint::LEN as u64,
                &spl_token_id_v2_0(),
            ),
            spl_token_v2_0_instruction(
                initialize_mint(
                    &spl_token_v2_0::id(),
                    &spl_token_v2_0_pubkey(&mint.pubkey()),
                    &spl_token_v2_0_pubkey(&mint.pubkey()),
                    None,
                    decimals,
                )
                .unwrap(),
            ),
        ];
        let mut transaction = Transaction::new_with_payer(&instructions, Some(&fee_payer.pubkey()));
        transaction.sign(&[fee_payer, mint], recent_blockhash);
        banks_client.process_transaction(transaction).await.unwrap();
    }

    async fn initialize_token_account(
        banks_client: &mut BanksClient,
        fee_payer: &Keypair,
        sender_account: &Keypair,
        mint: &Keypair,
        owner: &Keypair,
        recent_blockhash: Hash,
    ) {
        let rent = banks_client.get_rent().await.unwrap();
        let expected_token_account_balance = rent.minimum_balance(SplTokenAccount::LEN);
        let instructions = vec![
            system_instruction::create_account(
                &fee_payer.pubkey(),
                &sender_account.pubkey(),
                expected_token_account_balance,
                SplTokenAccount::LEN as u64,
                &spl_token_id_v2_0(),
            ),
            spl_token_v2_0_instruction(
                initialize_account(
                    &spl_token_v2_0::id(),
                    &spl_token_v2_0_pubkey(&sender_account.pubkey()),
                    &spl_token_v2_0_pubkey(&mint.pubkey()),
                    &spl_token_v2_0_pubkey(&owner.pubkey()),
                )
                .unwrap(),
            ),
        ];
        let mut transaction = Transaction::new_with_payer(&instructions, Some(&fee_payer.pubkey()));
        transaction.sign(&[fee_payer, sender_account], recent_blockhash);
        banks_client.process_transaction(transaction).await.unwrap();
    }

    async fn mint_to_account(
        banks_client: &mut BanksClient,
        fee_payer: &Keypair,
        sender_account: &Keypair,
        mint: &Keypair,
        recent_blockhash: Hash,
    ) {
        let instructions = vec![spl_token_v2_0_instruction(
            mint_to(
                &spl_token_v2_0::id(),
                &spl_token_v2_0_pubkey(&mint.pubkey()),
                &spl_token_v2_0_pubkey(&sender_account.pubkey()),
                &spl_token_v2_0_pubkey(&mint.pubkey()),
                &[],
                200_000,
            )
            .unwrap(),
        )];
        let mut transaction = Transaction::new_with_payer(&instructions, Some(&fee_payer.pubkey()));
        transaction.sign(&[fee_payer, mint], recent_blockhash);
        banks_client.process_transaction(transaction).await.unwrap();
    }

    async fn test_process_distribute_spl_tokens_with_client(
        banks_client: &mut BanksClient,
        fee_payer: Keypair,
        transfer_amount: Option<u64>,
        recent_blockhash: Hash,
    ) {
        // Initialize Token Mint
        let decimals = 2;
        let mint = Keypair::new();
        initialize_test_mint(banks_client, &fee_payer, &mint, decimals, recent_blockhash).await;

        // Initialize Sender Token Account and Mint
        let sender_account = Keypair::new();
        let owner = Keypair::new();
        initialize_token_account(
            banks_client,
            &fee_payer,
            &sender_account,
            &mint,
            &owner,
            recent_blockhash,
        )
        .await;

        mint_to_account(
            banks_client,
            &fee_payer,
            &sender_account,
            &mint,
            recent_blockhash,
        )
        .await;

        // Initialize one recipient Associated Token Account
        let wallet_address_0 = Pubkey::new_unique();
        let instructions = vec![spl_token_v2_0_instruction(create_associated_token_account(
            &spl_token_v2_0_pubkey(&fee_payer.pubkey()),
            &wallet_address_0,
            &spl_token_v2_0_pubkey(&mint.pubkey()),
        ))];
        let mut transaction = Transaction::new_with_payer(&instructions, Some(&fee_payer.pubkey()));
        transaction.sign(&[&fee_payer], recent_blockhash);
        banks_client.process_transaction(transaction).await.unwrap();

        let wallet_address_1 = Pubkey::new_unique();

        // Create allocations csv
        let allocation_amount = if let Some(amount) = transfer_amount {
            amount
        } else {
            100_000
        };
        let allocations_file = NamedTempFile::new().unwrap();
        let input_csv = allocations_file.path().to_str().unwrap().to_string();
        let mut wtr = csv::WriterBuilder::new().from_writer(allocations_file);
        wtr.write_record(&["recipient", "amount"]).unwrap();
        wtr.write_record(&[wallet_address_0.to_string(), allocation_amount.to_string()])
            .unwrap();
        wtr.write_record(&[wallet_address_1.to_string(), allocation_amount.to_string()])
            .unwrap();
        wtr.flush().unwrap();

        let dir = tempdir().unwrap();
        let transaction_db = dir
            .path()
            .join("transactions.db")
            .to_str()
            .unwrap()
            .to_string();

        let output_file = NamedTempFile::new().unwrap();
        let output_path = output_file.path().to_str().unwrap().to_string();

        let args = DistributeTokensArgs {
            sender_keypair: Box::new(owner),
            fee_payer: Box::new(fee_payer),
            dry_run: false,
            input_csv,
            transaction_db: transaction_db.clone(),
            output_path: Some(output_path.clone()),
            stake_args: None,
            spl_token_args: Some(SplTokenArgs {
                token_account_address: sender_account.pubkey(),
                mint: mint.pubkey(),
                decimals,
            }),
            transfer_amount,
        };

        // Distribute Allocations
        let confirmations = process_allocations(banks_client, &args).await.unwrap();
        assert_eq!(confirmations, None);

        let associated_token_address_0 =
            get_associated_token_address(&wallet_address_0, &spl_token_v2_0_pubkey(&mint.pubkey()));
        let associated_token_address_1 =
            get_associated_token_address(&wallet_address_1, &spl_token_v2_0_pubkey(&mint.pubkey()));

        let transaction_infos =
            db::read_transaction_infos(&db::open_db(&transaction_db, true).unwrap());
        assert_eq!(transaction_infos.len(), 2);
        assert!(transaction_infos
            .iter()
            .any(|info| info.recipient == pubkey_from_spl_token_v2_0(&wallet_address_0)));
        assert!(transaction_infos
            .iter()
            .any(|info| info.recipient == pubkey_from_spl_token_v2_0(&wallet_address_1)));
        assert_eq!(transaction_infos[0].amount, allocation_amount);
        assert_eq!(transaction_infos[1].amount, allocation_amount);

        let recipient_account_0 = banks_client
            .get_account(pubkey_from_spl_token_v2_0(&associated_token_address_0))
            .await
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            SplTokenAccount::unpack(&recipient_account_0.data)
                .unwrap()
                .amount,
            allocation_amount,
        );
        let recipient_account_1 = banks_client
            .get_account(pubkey_from_spl_token_v2_0(&associated_token_address_1))
            .await
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            SplTokenAccount::unpack(&recipient_account_1.data)
                .unwrap()
                .amount,
            allocation_amount,
        );

        check_output_file(&output_path, &db::open_db(&transaction_db, true).unwrap());

        // Now, run it again, and check there's no double-spend.
        process_allocations(banks_client, &args).await.unwrap();
        let transaction_infos =
            db::read_transaction_infos(&db::open_db(&transaction_db, true).unwrap());
        assert_eq!(transaction_infos.len(), 2);
        assert!(transaction_infos
            .iter()
            .any(|info| info.recipient == pubkey_from_spl_token_v2_0(&wallet_address_0)));
        assert!(transaction_infos
            .iter()
            .any(|info| info.recipient == pubkey_from_spl_token_v2_0(&wallet_address_1)));
        assert_eq!(transaction_infos[0].amount, allocation_amount);
        assert_eq!(transaction_infos[1].amount, allocation_amount);

        let recipient_account_0 = banks_client
            .get_account(pubkey_from_spl_token_v2_0(&associated_token_address_0))
            .await
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            SplTokenAccount::unpack(&recipient_account_0.data)
                .unwrap()
                .amount,
            allocation_amount,
        );
        let recipient_account_1 = banks_client
            .get_account(pubkey_from_spl_token_v2_0(&associated_token_address_1))
            .await
            .unwrap()
            .unwrap_or_default();
        assert_eq!(
            SplTokenAccount::unpack(&recipient_account_1.data)
                .unwrap()
                .amount,
            allocation_amount,
        );

        check_output_file(&output_path, &db::open_db(&transaction_db, true).unwrap());
    }

    #[tokio::test]
    async fn test_process_spl_token_allocations() {
        let (mut banks_client, payer, recent_blockhash) = program_test().start().await;
        test_process_distribute_spl_tokens_with_client(
            &mut banks_client,
            payer,
            None,
            recent_blockhash,
        )
        .await;
    }

    #[tokio::test]
    async fn test_process_spl_token_transfer_amount_allocations() {
        let (mut banks_client, payer, recent_blockhash) = program_test().start().await;
        test_process_distribute_spl_tokens_with_client(
            &mut banks_client,
            payer,
            Some(10550),
            recent_blockhash,
        )
        .await;
    }

    #[tokio::test]
    async fn test_check_check_spl_token_balances() {
        let (mut banks_client, payer, recent_blockhash) = program_test().start().await;

        let (fee_calculator, _, _) = banks_client.get_fees().await.unwrap();
        let signatures = 2;
        let fees = fee_calculator.lamports_per_signature * signatures;
        let fees_in_sol = lamports_to_sol(fees);

        let rent = banks_client.get_rent().await.unwrap();
        let expected_token_account_balance = rent.minimum_balance(SplTokenAccount::LEN);
        let expected_token_account_balance_sol = lamports_to_sol(expected_token_account_balance);

        // Initialize Token Mint
        let decimals = 2;
        let mint = Keypair::new();
        initialize_test_mint(&mut banks_client, &payer, &mint, decimals, recent_blockhash).await;

        // Initialize Sender Token Account and Mint
        let sender_account = Keypair::new();
        let owner = Keypair::new();
        let owner_keypair_file = tmp_file_path("keypair_file", &owner.pubkey());
        write_keypair_file(&owner, &owner_keypair_file).unwrap();

        initialize_token_account(
            &mut banks_client,
            &payer,
            &sender_account,
            &mint,
            &owner,
            recent_blockhash,
        )
        .await;

        let unfunded_fee_payer = Keypair::new();

        let allocation_amount = 4200;
        let allocations = vec![Allocation {
            recipient: Pubkey::new_unique().to_string(),
            amount: allocation_amount,
            lockup_date: "".to_string(),
        }];
        let mut args = DistributeTokensArgs {
            sender_keypair: read_keypair_file(&owner_keypair_file).unwrap().into(),
            fee_payer: Box::new(unfunded_fee_payer),
            dry_run: false,
            input_csv: "".to_string(),
            transaction_db: "".to_string(),
            output_path: None,
            stake_args: None,
            spl_token_args: Some(SplTokenArgs {
                token_account_address: sender_account.pubkey(),
                mint: mint.pubkey(),
                decimals,
            }),
            transfer_amount: None,
        };

        // Unfunded fee_payer
        let err_result = check_spl_token_balances(
            signatures as usize,
            &allocations,
            &mut banks_client,
            &args,
            1,
        )
        .await
        .unwrap_err();
        if let Error::InsufficientFunds(sources, amount) = err_result {
            assert_eq!(sources, vec![FundingSource::FeePayer].into());
            assert!(
                (amount - (fees_in_sol + expected_token_account_balance_sol)).abs() < f64::EPSILON
            );
        } else {
            panic!("check_spl_token_balances should have errored");
        }

        // Unfunded sender SPL Token account
        let fee_payer = Keypair::new();

        let instruction = system_instruction::transfer(
            &payer.pubkey(),
            &fee_payer.pubkey(),
            fees + expected_token_account_balance,
        );
        let mut transaction = Transaction::new_with_payer(&[instruction], Some(&payer.pubkey()));
        transaction.sign(&[&payer], recent_blockhash);
        banks_client.process_transaction(transaction).await.unwrap();

        args.fee_payer = Box::new(fee_payer);
        let err_result = check_spl_token_balances(
            signatures as usize,
            &allocations,
            &mut banks_client,
            &args,
            1,
        )
        .await
        .unwrap_err();
        if let Error::InsufficientFunds(sources, amount) = err_result {
            assert_eq!(sources, vec![FundingSource::SplTokenAccount].into());
            assert!(
                (amount - token_amount_to_ui_amount(allocation_amount, decimals).ui_amount).abs()
                    < f64::EPSILON
            );
        } else {
            panic!("check_spl_token_balances should have errored");
        }

        // Fully funded payers
        mint_to_account(
            &mut banks_client,
            &payer,
            &sender_account,
            &mint,
            recent_blockhash,
        )
        .await;

        check_spl_token_balances(
            signatures as usize,
            &allocations,
            &mut banks_client,
            &args,
            1,
        )
        .await
        .unwrap();

        // Partially-funded fee payer can afford fees, but not to create Associated Token Account
        let partially_funded_fee_payer = Keypair::new();

        let instruction = system_instruction::transfer(
            &payer.pubkey(),
            &partially_funded_fee_payer.pubkey(),
            fees,
        );
        let mut transaction = Transaction::new_with_payer(&[instruction], Some(&payer.pubkey()));
        transaction.sign(&[&payer], recent_blockhash);
        banks_client.process_transaction(transaction).await.unwrap();

        args.fee_payer = Box::new(partially_funded_fee_payer);
        let err_result = check_spl_token_balances(
            signatures as usize,
            &allocations,
            &mut banks_client,
            &args,
            1,
        )
        .await
        .unwrap_err();
        if let Error::InsufficientFunds(sources, amount) = err_result {
            assert_eq!(sources, vec![FundingSource::FeePayer].into());
            assert!(
                (amount - (fees_in_sol + expected_token_account_balance_sol)).abs() < f64::EPSILON
            );
        } else {
            panic!("check_spl_token_balances should have errored");
        }

        // Succeeds if no account creation required
        check_spl_token_balances(
            signatures as usize,
            &allocations,
            &mut banks_client,
            &args,
            0,
        )
        .await
        .unwrap();
    }
}
