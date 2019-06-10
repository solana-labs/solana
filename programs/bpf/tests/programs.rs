#[cfg(any(feature = "bpf_c", feature = "bpf_rust"))]
mod bpf {
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_runtime::loader_utils::load_program;
    use solana_sdk::genesis_block::create_genesis_block;
    use solana_sdk::native_loader;
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
        fn test_program_bpf_c_noop() {
            solana_logger::setup();

            let mut file = File::open(create_bpf_path("noop")).expect("file open failed");
            let mut elf = Vec::new();
            file.read_to_end(&mut elf).unwrap();

            let (genesis_block, mint_keypair) = create_genesis_block(50);
            let bank = Bank::new(&genesis_block);
            let bank_client = BankClient::new(bank);

            // Call user program
            let program_id = load_program(&bank_client, &mint_keypair, &bpf_loader::id(), elf);
            let instruction = create_invoke_instruction(mint_keypair.pubkey(), program_id, &1u8);
            bank_client
                .send_instruction(&mint_keypair, instruction)
                .unwrap();
        }

        #[test]
        fn test_program_bpf_c() {
            solana_logger::setup();

            let programs = [
                "bpf_to_bpf",
                "multiple_static",
                "noop",
                "noop++",
                "relative_call",
                "struct_pass",
                "struct_ret",
            ];
            for program in programs.iter() {
                println!("Test program: {:?}", program);
                let mut file = File::open(create_bpf_path(program)).expect("file open failed");
                let mut elf = Vec::new();
                file.read_to_end(&mut elf).unwrap();

                let (genesis_block, mint_keypair) = create_genesis_block(50);
                let bank = Bank::new(&genesis_block);
                let bank_client = BankClient::new(bank);

                let loader_pubkey = load_program(
                    &bank_client,
                    &mint_keypair,
                    &native_loader::id(),
                    "solana_bpf_loader".as_bytes().to_vec(),
                );

                // Call user program
                let program_id = load_program(&bank_client, &mint_keypair, &loader_pubkey, elf);
                let instruction =
                    create_invoke_instruction(mint_keypair.pubkey(), program_id, &1u8);
                bank_client
                    .send_instruction(&mint_keypair, instruction)
                    .unwrap();
            }
        }
    }

    #[cfg(feature = "bpf_rust")]
    mod bpf_rust {
        use super::*;
        use solana_sdk::client::SyncClient;
        use solana_sdk::instruction::{AccountMeta, Instruction};
        use solana_sdk::signature::{Keypair, KeypairUtil};
        use std::io::Read;

        #[test]
        fn test_program_bpf_rust() {
            solana_logger::setup();

            let programs = [
                ("solana_bpf_rust_alloc", true),
                ("solana_bpf_rust_iter", true),
                ("solana_bpf_rust_noop", true),
                ("solana_bpf_rust_panic", false),
            ];
            for program in programs.iter() {
                let filename = create_bpf_path(program.0);
                println!("Test program: {:?} from {:?}", program.0, filename);
                let mut file = File::open(filename).unwrap();
                let mut elf = Vec::new();
                file.read_to_end(&mut elf).unwrap();

                let (genesis_block, mint_keypair) = create_genesis_block(50);
                let bank = Bank::new(&genesis_block);
                let bank_client = BankClient::new(bank);

                let loader_pubkey = load_program(
                    &bank_client,
                    &mint_keypair,
                    &native_loader::id(),
                    "solana_bpf_loader".as_bytes().to_vec(),
                );

                // Call user program
                let program_id = load_program(&bank_client, &mint_keypair, &loader_pubkey, elf);
                let account_metas = vec![
                    AccountMeta::new(mint_keypair.pubkey(), true),
                    AccountMeta::new(Keypair::new().pubkey(), false),
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
