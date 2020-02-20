#[cfg(any(feature = "bpf_c", feature = "bpf_rust"))]
mod bpf {
    use solana_runtime::{
        bank::Bank,
        bank_client::BankClient,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        loader_utils::load_program,
    };
    use std::{env, fs::File, path::PathBuf};

    /// BPF program file extension
    const PLATFORM_FILE_EXTENSION_BPF: &str = "so";

    /// Create a BPF program file name
    fn create_bpf_path(name: &str) -> PathBuf {
        let mut pathbuf = {
            let current_exe = env::current_exe().unwrap();
            PathBuf::from(current_exe.parent().unwrap().parent().unwrap())
        };
        pathbuf.push("bpf/");
        pathbuf.push(name);
        pathbuf.set_extension(PLATFORM_FILE_EXTENSION_BPF);
        pathbuf
    }

    #[cfg(feature = "bpf_c")]
    mod bpf_c {
        use super::*;
        use solana_runtime::loader_utils::create_invoke_instruction;
        use solana_sdk::{
            account::Account,
            bpf_loader,
            client::SyncClient,
            instruction::{AccountMeta, Instruction, InstructionError},
            pubkey::Pubkey,
            signature::Signer,
            transaction::TransactionError,
        };
        use std::{io::Read, sync::Arc};

        #[test]
        fn test_program_bpf_c() {
            solana_logger::setup();

            let programs = [
                ("bpf_to_bpf", true),
                ("multiple_static", true),
                ("noop", true),
                ("noop++", true),
                ("panic", false),
                ("relative_call", true),
                ("struct_pass", true),
                ("struct_ret", true),
            ];
            for program in programs.iter() {
                println!("Test program: {:?}", program.0);
                let mut file = File::open(create_bpf_path(program.0)).expect("file open failed");
                let mut elf = Vec::new();
                file.read_to_end(&mut elf).unwrap();

                let GenesisConfigInfo {
                    genesis_config,
                    mint_keypair,
                    ..
                } = create_genesis_config(50);
                let bank = Bank::new(&genesis_config);
                let bank_client = BankClient::new(bank);

                // Call user program
                let program_id = load_program(&bank_client, &mint_keypair, &bpf_loader::id(), elf);
                let instruction =
                    create_invoke_instruction(mint_keypair.pubkey(), program_id, &1u8);
                let result = bank_client.send_instruction(&mint_keypair, instruction);
                if program.1 {
                    assert!(result.is_ok());
                } else {
                    assert!(result.is_err());
                }
            }
        }

        #[test]
        fn test_program_bpf_c_duplicate_accounts() {
            solana_logger::setup();

            let filename = create_bpf_path("dup_accounts");
            let mut file = File::open(filename).unwrap();
            let mut elf = Vec::new();
            file.read_to_end(&mut elf).unwrap();

            let GenesisConfigInfo {
                genesis_config,
                mint_keypair,
                ..
            } = create_genesis_config(50);

            let bank = Arc::new(Bank::new(&genesis_config));
            let bank_client = BankClient::new_shared(&bank);
            let program_id = load_program(&bank_client, &mint_keypair, &bpf_loader::id(), elf);

            let payee_account = Account::new(10, 1, &program_id);
            let payee_pubkey = Pubkey::new_rand();
            bank.store_account(&payee_pubkey, &payee_account);

            let account = Account::new(10, 1, &program_id);
            let pubkey = Pubkey::new_rand();
            let account_metas = vec![
                AccountMeta::new(mint_keypair.pubkey(), true),
                AccountMeta::new(payee_pubkey, false),
                AccountMeta::new(pubkey, false),
                AccountMeta::new(pubkey, false),
            ];

            bank.store_account(&pubkey, &account);
            let instruction = Instruction::new(program_id, &1u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
            assert!(result.is_ok());
            assert_eq!(data[0], 1);

            bank.store_account(&pubkey, &account);
            let instruction = Instruction::new(program_id, &2u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
            assert!(result.is_ok());
            assert_eq!(data[0], 2);

            bank.store_account(&pubkey, &account);
            let instruction = Instruction::new(program_id, &3u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
            assert!(result.is_ok());
            assert_eq!(data[0], 3);

            bank.store_account(&pubkey, &account);
            let instruction = Instruction::new(program_id, &4u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            let lamports = bank_client.get_balance(&pubkey).unwrap();
            assert!(result.is_ok());
            assert_eq!(lamports, 11);

            bank.store_account(&pubkey, &account);
            let instruction = Instruction::new(program_id, &5u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            let lamports = bank_client.get_balance(&pubkey).unwrap();
            assert!(result.is_ok());
            assert_eq!(lamports, 12);

            bank.store_account(&pubkey, &account);
            let instruction = Instruction::new(program_id, &6u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            let lamports = bank_client.get_balance(&pubkey).unwrap();
            assert!(result.is_ok());
            assert_eq!(lamports, 13);
        }

        #[test]
        fn test_program_bpf_c_error_handling() {
            solana_logger::setup();

            let filename = create_bpf_path("error_handling");
            let mut file = File::open(filename).unwrap();
            let mut elf = Vec::new();
            file.read_to_end(&mut elf).unwrap();

            let GenesisConfigInfo {
                genesis_config,
                mint_keypair,
                ..
            } = create_genesis_config(50);

            let bank = Bank::new(&genesis_config);
            let bank_client = BankClient::new(bank);
            let program_id = load_program(&bank_client, &mint_keypair, &bpf_loader::id(), elf);
            let account_metas = vec![AccountMeta::new(mint_keypair.pubkey(), true)];

            let instruction = Instruction::new(program_id, &1u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            assert!(result.is_ok());

            let instruction = Instruction::new(program_id, &2u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            assert_eq!(
                result.unwrap_err().unwrap(),
                TransactionError::InstructionError(0, InstructionError::InvalidAccountData)
            );

            let instruction = Instruction::new(program_id, &3u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            assert_eq!(
                result.unwrap_err().unwrap(),
                TransactionError::InstructionError(0, InstructionError::CustomError(42))
            );

            let instruction = Instruction::new(program_id, &4u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            assert_eq!(
                result.unwrap_err().unwrap(),
                TransactionError::InstructionError(0, InstructionError::InvalidError)
            );

            let instruction = Instruction::new(program_id, &5u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            assert_eq!(
                result.unwrap_err().unwrap(),
                TransactionError::InstructionError(0, InstructionError::InvalidError)
            );

            let instruction = Instruction::new(program_id, &6u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            assert_eq!(
                result.unwrap_err().unwrap(),
                TransactionError::InstructionError(0, InstructionError::InvalidInstructionData)
            );
        }
    }

    #[cfg(feature = "bpf_rust")]
    mod bpf_rust {
        use super::*;
        use solana_sdk::{
            account::Account,
            bpf_loader,
            client::SyncClient,
            clock::DEFAULT_SLOTS_PER_EPOCH,
            instruction::{AccountMeta, Instruction, InstructionError},
            pubkey::Pubkey,
            signature::{Keypair, Signer},
            sysvar::{clock, fees, rent, rewards, slot_hashes, stake_history},
            transaction::TransactionError,
        };
        use std::io::Read;
        use std::sync::Arc;

        #[test]
        fn test_program_bpf_rust() {
            solana_logger::setup();

            let programs = [
                ("solana_bpf_rust_128bit", true),
                ("solana_bpf_rust_alloc", true),
                ("solana_bpf_rust_dep_crate", true),
                ("solana_bpf_rust_external_spend", false),
                ("solana_bpf_rust_iter", true),
                ("solana_bpf_rust_many_args", true),
                ("solana_bpf_rust_noop", true),
                ("solana_bpf_rust_panic", false),
                ("solana_bpf_rust_param_passing", true),
                ("solana_bpf_rust_sysval", true),
            ];
            for program in programs.iter() {
                let filename = create_bpf_path(program.0);
                println!("Test program: {:?} from {:?}", program.0, filename);
                let mut file = File::open(filename).unwrap();
                let mut elf = Vec::new();
                file.read_to_end(&mut elf).unwrap();

                let GenesisConfigInfo {
                    genesis_config,
                    mint_keypair,
                    ..
                } = create_genesis_config(50);
                let bank = Arc::new(Bank::new(&genesis_config));
                // Create bank with specific slot, used by solana_bpf_rust_sysvar test
                let bank =
                    Bank::new_from_parent(&bank, &Pubkey::default(), DEFAULT_SLOTS_PER_EPOCH + 1);
                let bank_client = BankClient::new(bank);

                // Call user program
                let program_id = load_program(&bank_client, &mint_keypair, &bpf_loader::id(), elf);
                let account_metas = vec![
                    AccountMeta::new(mint_keypair.pubkey(), true),
                    AccountMeta::new(Keypair::new().pubkey(), false),
                    AccountMeta::new(clock::id(), false),
                    AccountMeta::new(fees::id(), false),
                    AccountMeta::new(rewards::id(), false),
                    AccountMeta::new(slot_hashes::id(), false),
                    AccountMeta::new(stake_history::id(), false),
                    AccountMeta::new(rent::id(), false),
                ];
                let instruction = Instruction::new(program_id, &1u8, account_metas);
                let result = bank_client.send_instruction(&mint_keypair, instruction);
                if program.1 {
                    assert!(result.is_ok());
                } else {
                    assert!(result.is_err());
                }
            }
        }

        #[test]
        fn test_program_bpf_rust_duplicate_accounts() {
            solana_logger::setup();

            let filename = create_bpf_path("solana_bpf_rust_dup_accounts");
            let mut file = File::open(filename).unwrap();
            let mut elf = Vec::new();
            file.read_to_end(&mut elf).unwrap();

            let GenesisConfigInfo {
                genesis_config,
                mint_keypair,
                ..
            } = create_genesis_config(50);

            let bank = Arc::new(Bank::new(&genesis_config));
            let bank_client = BankClient::new_shared(&bank);
            let program_id = load_program(&bank_client, &mint_keypair, &bpf_loader::id(), elf);

            let payee_account = Account::new(10, 1, &program_id);
            let payee_pubkey = Pubkey::new_rand();
            bank.store_account(&payee_pubkey, &payee_account);

            let account = Account::new(10, 1, &program_id);
            let pubkey = Pubkey::new_rand();
            let account_metas = vec![
                AccountMeta::new(mint_keypair.pubkey(), true),
                AccountMeta::new(payee_pubkey, false),
                AccountMeta::new(pubkey, false),
                AccountMeta::new(pubkey, false),
            ];

            bank.store_account(&pubkey, &account);
            let instruction = Instruction::new(program_id, &1u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
            assert!(result.is_ok());
            assert_eq!(data[0], 1);

            bank.store_account(&pubkey, &account);
            let instruction = Instruction::new(program_id, &2u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
            assert!(result.is_ok());
            assert_eq!(data[0], 2);

            bank.store_account(&pubkey, &account);
            let instruction = Instruction::new(program_id, &3u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            let data = bank_client.get_account_data(&pubkey).unwrap().unwrap();
            assert!(result.is_ok());
            assert_eq!(data[0], 3);

            bank.store_account(&pubkey, &account);
            let instruction = Instruction::new(program_id, &4u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            let lamports = bank_client.get_balance(&pubkey).unwrap();
            assert!(result.is_ok());
            assert_eq!(lamports, 11);

            bank.store_account(&pubkey, &account);
            let instruction = Instruction::new(program_id, &5u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            let lamports = bank_client.get_balance(&pubkey).unwrap();
            assert!(result.is_ok());
            assert_eq!(lamports, 12);

            bank.store_account(&pubkey, &account);
            let instruction = Instruction::new(program_id, &6u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            let lamports = bank_client.get_balance(&pubkey).unwrap();
            assert!(result.is_ok());
            assert_eq!(lamports, 13);
        }

        #[test]
        fn test_program_bpf_rust_error_handling() {
            solana_logger::setup();

            let filename = create_bpf_path("solana_bpf_rust_error_handling");
            let mut file = File::open(filename).unwrap();
            let mut elf = Vec::new();
            file.read_to_end(&mut elf).unwrap();

            let GenesisConfigInfo {
                genesis_config,
                mint_keypair,
                ..
            } = create_genesis_config(50);

            let bank = Bank::new(&genesis_config);
            let bank_client = BankClient::new(bank);
            let program_id = load_program(&bank_client, &mint_keypair, &bpf_loader::id(), elf);
            let account_metas = vec![AccountMeta::new(mint_keypair.pubkey(), true)];

            let instruction = Instruction::new(program_id, &1u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            assert!(result.is_ok());

            let instruction = Instruction::new(program_id, &2u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            assert_eq!(
                result.unwrap_err().unwrap(),
                TransactionError::InstructionError(0, InstructionError::InvalidAccountData)
            );

            let instruction = Instruction::new(program_id, &3u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            assert_eq!(
                result.unwrap_err().unwrap(),
                TransactionError::InstructionError(0, InstructionError::CustomError(0))
            );

            let instruction = Instruction::new(program_id, &4u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            assert_eq!(
                result.unwrap_err().unwrap(),
                TransactionError::InstructionError(0, InstructionError::CustomError(42))
            );

            let instruction = Instruction::new(program_id, &6u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            assert_eq!(
                result.unwrap_err().unwrap(),
                TransactionError::InstructionError(0, InstructionError::AccountBorrowFailed)
            );

            let instruction = Instruction::new(program_id, &7u8, account_metas.clone());
            let result = bank_client.send_instruction(&mint_keypair, instruction);
            assert_eq!(
                result.unwrap_err().unwrap(),
                TransactionError::InstructionError(0, InstructionError::InvalidInstructionData)
            );
        }
    }
}
