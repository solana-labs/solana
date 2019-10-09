#[cfg(any(feature = "bpf_c", feature = "bpf_rust"))]
mod bpf {
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_runtime::genesis_utils::{create_genesis_block, GenesisBlockInfo};
    use solana_runtime::loader_utils::load_program;
    use std::env;
    use std::fs::File;
    use std::path::PathBuf;

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
        use solana_sdk::bpf_loader;
        use solana_sdk::client::SyncClient;
        use solana_sdk::signature::KeypairUtil;
        use std::io::Read;

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

                let GenesisBlockInfo {
                    genesis_block,
                    mint_keypair,
                    ..
                } = create_genesis_block(50);
                let bank = Bank::new(&genesis_block);
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
    }

    #[cfg(feature = "bpf_rust")]
    mod bpf_rust {
        use super::*;
        use solana_sdk::bpf_loader;
        use solana_sdk::client::SyncClient;
        use solana_sdk::clock::DEFAULT_SLOTS_PER_EPOCH;
        use solana_sdk::instruction::{AccountMeta, Instruction};
        use solana_sdk::pubkey::Pubkey;
        use solana_sdk::signature::{Keypair, KeypairUtil};
        use solana_sdk::sysvar::{clock, fees, rent, rewards, slot_hashes, stake_history};
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

                let GenesisBlockInfo {
                    genesis_block,
                    mint_keypair,
                    ..
                } = create_genesis_block(50);
                let bank = Arc::new(Bank::new(&genesis_block));
                // Create bank with specific slot, used by solana_bpf_rust_sysvar test
                dbg!(bank.epoch());
                let bank =
                    Bank::new_from_parent(&bank, &Pubkey::default(), DEFAULT_SLOTS_PER_EPOCH + 1);
                dbg!(bank.epoch());
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
    }
}
