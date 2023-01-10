#![allow(clippy::integer_arithmetic)]

use {
    serde_json::Value,
    solana_cli::{
        cli::{process_command, CliCommand, CliConfig},
        program::{ProgramCliCommand, CLOSE_PROGRAM_WARNING},
    },
    solana_cli_output::OutputFormat,
    solana_faucet::faucet::run_local_faucet,
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{
        account_utils::StateMut,
        bpf_loader_upgradeable::{self, UpgradeableLoaderState},
        commitment_config::CommitmentConfig,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_test_validator::TestValidator,
    std::{env, fs::File, io::Read, path::PathBuf, str::FromStr},
};

#[test]
fn test_cli_program_deploy_non_upgradeable() {
    solana_logger::setup();

    let mut noop_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    noop_path.push("tests");
    noop_path.push("fixtures");
    noop_path.push("noop");
    noop_path.set_extension("so");

    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, Some(faucet_addr), SocketAddrSpace::Unspecified);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let mut file = File::open(noop_path.to_str().unwrap()).unwrap();
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).unwrap();
    let minimum_balance_for_programdata = rpc_client
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_programdata(
            program_data.len(),
        ))
        .unwrap();
    let minimum_balance_for_program = rpc_client
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_program())
        .unwrap();

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&keypair];
    config.command = CliCommand::Airdrop {
        pubkey: None,
        lamports: 4 * minimum_balance_for_programdata, // min balance for rent exemption for three programs + leftover for tx processing
    };
    process_command(&config).unwrap();

    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_path.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: None,
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 0,
        is_final: true,
        max_len: None,
        skip_fee_check: false,
    });
    config.output_format = OutputFormat::JsonCompact;
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let program_id_str = json
        .as_object()
        .unwrap()
        .get("programId")
        .unwrap()
        .as_str()
        .unwrap();
    let program_id = Pubkey::from_str(program_id_str).unwrap();
    let account0 = rpc_client.get_account(&program_id).unwrap();
    assert_eq!(account0.lamports, minimum_balance_for_program);
    assert_eq!(account0.owner, bpf_loader_upgradeable::id());
    assert!(account0.executable);
    let (programdata_pubkey, _) =
        Pubkey::find_program_address(&[program_id.as_ref()], &bpf_loader_upgradeable::id());
    let programdata_account = rpc_client.get_account(&programdata_pubkey).unwrap();
    assert_eq!(
        programdata_account.lamports,
        minimum_balance_for_programdata
    );
    assert_eq!(programdata_account.owner, bpf_loader_upgradeable::id());
    assert!(!programdata_account.executable);
    assert_eq!(
        programdata_account.data[UpgradeableLoaderState::size_of_programdata_metadata()..],
        program_data[..]
    );

    // Test custom address
    let custom_address_keypair = Keypair::new();
    config.signers = vec![&keypair, &custom_address_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_path.to_str().unwrap().to_string()),
        program_signer_index: Some(1),
        program_pubkey: None,
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 0,
        is_final: true,
        max_len: None,
        skip_fee_check: false,
    });
    process_command(&config).unwrap();
    let account1 = rpc_client
        .get_account(&custom_address_keypair.pubkey())
        .unwrap();
    assert_eq!(account1.lamports, minimum_balance_for_program);
    assert_eq!(account1.owner, bpf_loader_upgradeable::id());
    assert!(account1.executable);
    let (programdata_pubkey, _) = Pubkey::find_program_address(
        &[custom_address_keypair.pubkey().as_ref()],
        &bpf_loader_upgradeable::id(),
    );
    let programdata_account = rpc_client.get_account(&programdata_pubkey).unwrap();
    assert_eq!(
        programdata_account.lamports,
        minimum_balance_for_programdata
    );
    assert_eq!(programdata_account.owner, bpf_loader_upgradeable::id());
    assert!(!programdata_account.executable);
    assert_eq!(
        programdata_account.data[UpgradeableLoaderState::size_of_programdata_metadata()..],
        program_data[..]
    );

    // Attempt to redeploy to the same address
    let err = process_command(&config).unwrap_err();
    assert_eq!(
        format!(
            "Program {} is no longer upgradeable",
            custom_address_keypair.pubkey()
        ),
        format!("{err}")
    );

    // Attempt to deploy to account with excess balance
    let custom_address_keypair = Keypair::new();
    config.signers = vec![&custom_address_keypair];
    config.command = CliCommand::Airdrop {
        pubkey: None,
        lamports: 2 * minimum_balance_for_programdata, // Anything over minimum_balance_for_programdata should trigger err
    };
    process_command(&config).unwrap();
    config.signers = vec![&keypair, &custom_address_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_path.to_str().unwrap().to_string()),
        program_signer_index: Some(1),
        program_pubkey: None,
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 0,
        is_final: true,
        max_len: None,
        skip_fee_check: false,
    });
    let err = process_command(&config).unwrap_err();
    assert_eq!(
        format!(
            "Account {} is not an upgradeable program or already in use",
            custom_address_keypair.pubkey()
        ),
        format!("{err}")
    );

    // Use forcing parameter to deploy to account with excess balance
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_path.to_str().unwrap().to_string()),
        program_signer_index: Some(1),
        program_pubkey: None,
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: true,
        upgrade_authority_signer_index: 0,
        is_final: true,
        max_len: None,
        skip_fee_check: false,
    });
    process_command(&config).unwrap_err();
}

#[test]
fn test_cli_program_deploy_no_authority() {
    solana_logger::setup();

    let mut noop_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    noop_path.push("tests");
    noop_path.push("fixtures");
    noop_path.push("noop");
    noop_path.set_extension("so");

    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, Some(faucet_addr), SocketAddrSpace::Unspecified);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let mut file = File::open(noop_path.to_str().unwrap()).unwrap();
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).unwrap();
    let max_len = program_data.len();
    let minimum_balance_for_programdata = rpc_client
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_programdata(
            max_len,
        ))
        .unwrap();
    let minimum_balance_for_program = rpc_client
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_program())
        .unwrap();
    let upgrade_authority = Keypair::new();

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.command = CliCommand::Airdrop {
        pubkey: None,
        lamports: 100 * minimum_balance_for_programdata + minimum_balance_for_program,
    };
    config.signers = vec![&keypair];
    process_command(&config).unwrap();

    // Deploy a program
    config.signers = vec![&keypair, &upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_path.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: None,
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: true,
        max_len: None,
        skip_fee_check: false,
    });
    config.output_format = OutputFormat::JsonCompact;
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let program_id_str = json
        .as_object()
        .unwrap()
        .get("programId")
        .unwrap()
        .as_str()
        .unwrap();
    let program_id = Pubkey::from_str(program_id_str).unwrap();

    // Attempt to upgrade the program
    config.signers = vec![&keypair, &upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_path.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: Some(program_id),
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: false,
        max_len: None,
        skip_fee_check: false,
    });
    process_command(&config).unwrap_err();
}

#[test]
fn test_cli_program_deploy_with_authority() {
    solana_logger::setup();

    let mut noop_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    noop_path.push("tests");
    noop_path.push("fixtures");
    noop_path.push("noop");
    noop_path.set_extension("so");

    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, Some(faucet_addr), SocketAddrSpace::Unspecified);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let mut file = File::open(noop_path.to_str().unwrap()).unwrap();
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).unwrap();
    let max_len = program_data.len();
    let minimum_balance_for_programdata = rpc_client
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_programdata(
            max_len,
        ))
        .unwrap();
    let minimum_balance_for_program = rpc_client
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_program())
        .unwrap();
    let upgrade_authority = Keypair::new();

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&keypair];
    config.command = CliCommand::Airdrop {
        pubkey: None,
        lamports: 100 * minimum_balance_for_programdata + minimum_balance_for_program,
    };
    process_command(&config).unwrap();

    // Deploy the upgradeable program with specified program_id
    let program_keypair = Keypair::new();
    config.signers = vec![&keypair, &upgrade_authority, &program_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_path.to_str().unwrap().to_string()),
        program_signer_index: Some(2),
        program_pubkey: Some(program_keypair.pubkey()),
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: false,
        max_len: Some(max_len),
        skip_fee_check: false,
    });
    config.output_format = OutputFormat::JsonCompact;
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let program_pubkey_str = json
        .as_object()
        .unwrap()
        .get("programId")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        program_keypair.pubkey(),
        Pubkey::from_str(program_pubkey_str).unwrap()
    );
    let program_account = rpc_client.get_account(&program_keypair.pubkey()).unwrap();
    assert_eq!(program_account.lamports, minimum_balance_for_program);
    assert_eq!(program_account.owner, bpf_loader_upgradeable::id());
    assert!(program_account.executable);
    let (programdata_pubkey, _) = Pubkey::find_program_address(
        &[program_keypair.pubkey().as_ref()],
        &bpf_loader_upgradeable::id(),
    );
    let programdata_account = rpc_client.get_account(&programdata_pubkey).unwrap();
    assert_eq!(
        programdata_account.lamports,
        minimum_balance_for_programdata
    );
    assert_eq!(programdata_account.owner, bpf_loader_upgradeable::id());
    assert!(!programdata_account.executable);
    assert_eq!(
        programdata_account.data[UpgradeableLoaderState::size_of_programdata_metadata()..],
        program_data[..]
    );

    // Deploy the upgradeable program
    config.signers = vec![&keypair, &upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_path.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: None,
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: false,
        max_len: Some(max_len),
        skip_fee_check: false,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let program_pubkey_str = json
        .as_object()
        .unwrap()
        .get("programId")
        .unwrap()
        .as_str()
        .unwrap();
    let program_pubkey = Pubkey::from_str(program_pubkey_str).unwrap();
    let program_account = rpc_client.get_account(&program_pubkey).unwrap();
    assert_eq!(program_account.lamports, minimum_balance_for_program);
    assert_eq!(program_account.owner, bpf_loader_upgradeable::id());
    assert!(program_account.executable);
    let (programdata_pubkey, _) =
        Pubkey::find_program_address(&[program_pubkey.as_ref()], &bpf_loader_upgradeable::id());
    let programdata_account = rpc_client.get_account(&programdata_pubkey).unwrap();
    assert_eq!(
        programdata_account.lamports,
        minimum_balance_for_programdata
    );
    assert_eq!(programdata_account.owner, bpf_loader_upgradeable::id());
    assert!(!programdata_account.executable);
    assert_eq!(
        programdata_account.data[UpgradeableLoaderState::size_of_programdata_metadata()..],
        program_data[..]
    );

    // Upgrade the program
    config.signers = vec![&keypair, &upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_path.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: Some(program_pubkey),
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: false,
        max_len: Some(max_len),
        skip_fee_check: false,
    });
    process_command(&config).unwrap();
    let program_account = rpc_client.get_account(&program_pubkey).unwrap();
    assert_eq!(program_account.lamports, minimum_balance_for_program);
    assert_eq!(program_account.owner, bpf_loader_upgradeable::id());
    assert!(program_account.executable);
    let (programdata_pubkey, _) =
        Pubkey::find_program_address(&[program_pubkey.as_ref()], &bpf_loader_upgradeable::id());
    let programdata_account = rpc_client.get_account(&programdata_pubkey).unwrap();
    assert_eq!(
        programdata_account.lamports,
        minimum_balance_for_programdata
    );
    assert_eq!(programdata_account.owner, bpf_loader_upgradeable::id());
    assert!(!programdata_account.executable);
    assert_eq!(
        programdata_account.data[UpgradeableLoaderState::size_of_programdata_metadata()..],
        program_data[..]
    );

    // Set a new authority
    let new_upgrade_authority = Keypair::new();
    config.signers = vec![&keypair, &upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::SetUpgradeAuthority {
        program_pubkey,
        upgrade_authority_index: Some(1),
        new_upgrade_authority: Some(new_upgrade_authority.pubkey()),
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let new_upgrade_authority_str = json
        .as_object()
        .unwrap()
        .get("authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        Pubkey::from_str(new_upgrade_authority_str).unwrap(),
        new_upgrade_authority.pubkey()
    );

    // Upgrade with new authority
    config.signers = vec![&keypair, &new_upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_path.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: Some(program_pubkey),
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: false,
        max_len: None,
        skip_fee_check: false,
    });
    process_command(&config).unwrap();
    let program_account = rpc_client.get_account(&program_pubkey).unwrap();
    assert_eq!(program_account.lamports, minimum_balance_for_program);
    assert_eq!(program_account.owner, bpf_loader_upgradeable::id());
    assert!(program_account.executable);
    let (programdata_pubkey, _) =
        Pubkey::find_program_address(&[program_pubkey.as_ref()], &bpf_loader_upgradeable::id());
    let programdata_account = rpc_client.get_account(&programdata_pubkey).unwrap();
    assert_eq!(
        programdata_account.lamports,
        minimum_balance_for_programdata
    );
    assert_eq!(programdata_account.owner, bpf_loader_upgradeable::id());
    assert!(!programdata_account.executable);
    assert_eq!(
        programdata_account.data[UpgradeableLoaderState::size_of_programdata_metadata()..],
        program_data[..]
    );

    // Get upgrade authority
    config.signers = vec![&keypair];
    config.command = CliCommand::Program(ProgramCliCommand::Show {
        account_pubkey: Some(program_pubkey),
        authority_pubkey: keypair.pubkey(),
        get_programs: false,
        get_buffers: false,
        all: false,
        use_lamports_unit: false,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let authority_pubkey_str = json
        .as_object()
        .unwrap()
        .get("authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        new_upgrade_authority.pubkey(),
        Pubkey::from_str(authority_pubkey_str).unwrap()
    );

    // Set no authority
    config.signers = vec![&keypair, &new_upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::SetUpgradeAuthority {
        program_pubkey,
        upgrade_authority_index: Some(1),
        new_upgrade_authority: None,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let new_upgrade_authority_str = json
        .as_object()
        .unwrap()
        .get("authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(new_upgrade_authority_str, "none");

    // Upgrade with no authority
    config.signers = vec![&keypair, &new_upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_path.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: Some(program_pubkey),
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: false,
        max_len: None,
        skip_fee_check: false,
    });
    process_command(&config).unwrap_err();

    // deploy with finality
    config.signers = vec![&keypair, &new_upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_path.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: None,
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: true,
        max_len: None,
        skip_fee_check: false,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let program_pubkey_str = json
        .as_object()
        .unwrap()
        .get("programId")
        .unwrap()
        .as_str()
        .unwrap();
    let program_pubkey = Pubkey::from_str(program_pubkey_str).unwrap();
    let (programdata_pubkey, _) =
        Pubkey::find_program_address(&[program_pubkey.as_ref()], &bpf_loader_upgradeable::id());
    let programdata_account = rpc_client.get_account(&programdata_pubkey).unwrap();
    if let UpgradeableLoaderState::ProgramData {
        slot: _,
        upgrade_authority_address,
    } = programdata_account.state().unwrap()
    {
        assert_eq!(upgrade_authority_address, None);
    } else {
        panic!("not a ProgramData account");
    }

    // Get buffer authority
    config.signers = vec![&keypair];
    config.command = CliCommand::Program(ProgramCliCommand::Show {
        account_pubkey: Some(program_pubkey),
        authority_pubkey: keypair.pubkey(),
        get_programs: false,
        get_buffers: false,
        all: false,
        use_lamports_unit: false,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let authority_pubkey_str = json
        .as_object()
        .unwrap()
        .get("authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!("none", authority_pubkey_str);
}

#[test]
fn test_cli_program_close_program() {
    solana_logger::setup();

    let mut noop_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    noop_path.push("tests");
    noop_path.push("fixtures");
    noop_path.push("noop");
    noop_path.set_extension("so");

    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, Some(faucet_addr), SocketAddrSpace::Unspecified);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let mut file = File::open(noop_path.to_str().unwrap()).unwrap();
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).unwrap();
    let max_len = program_data.len();
    let minimum_balance_for_programdata = rpc_client
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_programdata(
            max_len,
        ))
        .unwrap();
    let minimum_balance_for_program = rpc_client
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_program())
        .unwrap();
    let upgrade_authority = Keypair::new();

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&keypair];
    config.command = CliCommand::Airdrop {
        pubkey: None,
        lamports: 100 * minimum_balance_for_programdata + minimum_balance_for_program,
    };
    process_command(&config).unwrap();

    // Deploy the upgradeable program
    let program_keypair = Keypair::new();
    config.signers = vec![&keypair, &upgrade_authority, &program_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_path.to_str().unwrap().to_string()),
        program_signer_index: Some(2),
        program_pubkey: Some(program_keypair.pubkey()),
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: false,
        max_len: Some(max_len),
        skip_fee_check: false,
    });
    config.output_format = OutputFormat::JsonCompact;
    process_command(&config).unwrap();

    let (programdata_pubkey, _) = Pubkey::find_program_address(
        &[program_keypair.pubkey().as_ref()],
        &bpf_loader_upgradeable::id(),
    );

    // Close program
    let close_account = rpc_client.get_account(&programdata_pubkey).unwrap();
    let programdata_lamports = close_account.lamports;
    let recipient_pubkey = Pubkey::new_unique();
    config.signers = vec![&keypair, &upgrade_authority];

    // Close without --bypass-warning flag
    config.command = CliCommand::Program(ProgramCliCommand::Close {
        account_pubkey: Some(program_keypair.pubkey()),
        recipient_pubkey,
        authority_index: 1,
        use_lamports_unit: false,
        bypass_warning: false,
    });
    assert_eq!(
        process_command(&config).unwrap_err().to_string(),
        CLOSE_PROGRAM_WARNING.to_string()
    );

    // Close with --bypass-warning flag
    config.command = CliCommand::Program(ProgramCliCommand::Close {
        account_pubkey: Some(program_keypair.pubkey()),
        recipient_pubkey,
        authority_index: 1,
        use_lamports_unit: false,
        bypass_warning: true,
    });
    process_command(&config).unwrap();

    rpc_client.get_account(&programdata_pubkey).unwrap_err();
    let recipient_account = rpc_client.get_account(&recipient_pubkey).unwrap();
    assert_eq!(programdata_lamports, recipient_account.lamports);
}

#[test]
fn test_cli_program_write_buffer() {
    solana_logger::setup();

    let mut noop_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    noop_path.push("tests");
    noop_path.push("fixtures");
    noop_path.push("noop");
    noop_path.set_extension("so");

    let mut noop_large_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    noop_large_path.push("tests");
    noop_large_path.push("fixtures");
    noop_large_path.push("noop_large");
    noop_large_path.set_extension("so");

    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, Some(faucet_addr), SocketAddrSpace::Unspecified);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let mut file = File::open(noop_path.to_str().unwrap()).unwrap();
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).unwrap();
    let max_len = program_data.len();
    let minimum_balance_for_buffer = rpc_client
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_programdata(
            max_len,
        ))
        .unwrap();
    let minimum_balance_for_buffer_default = rpc_client
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_programdata(
            max_len,
        ))
        .unwrap();

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&keypair];
    config.command = CliCommand::Airdrop {
        pubkey: None,
        lamports: 100 * minimum_balance_for_buffer,
    };
    process_command(&config).unwrap();

    // Write a buffer with default params
    config.signers = vec![&keypair];
    config.command = CliCommand::Program(ProgramCliCommand::WriteBuffer {
        program_location: noop_path.to_str().unwrap().to_string(),
        buffer_signer_index: None,
        buffer_pubkey: None,
        buffer_authority_signer_index: 0,
        max_len: None,
        skip_fee_check: false,
    });
    config.output_format = OutputFormat::JsonCompact;
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let buffer_pubkey_str = json
        .as_object()
        .unwrap()
        .get("buffer")
        .unwrap()
        .as_str()
        .unwrap();
    let new_buffer_pubkey = Pubkey::from_str(buffer_pubkey_str).unwrap();
    let buffer_account = rpc_client.get_account(&new_buffer_pubkey).unwrap();
    assert_eq!(buffer_account.lamports, minimum_balance_for_buffer_default);
    assert_eq!(buffer_account.owner, bpf_loader_upgradeable::id());
    if let UpgradeableLoaderState::Buffer { authority_address } = buffer_account.state().unwrap() {
        assert_eq!(authority_address, Some(keypair.pubkey()));
    } else {
        panic!("not a buffer account");
    }
    assert_eq!(
        buffer_account.data[UpgradeableLoaderState::size_of_buffer_metadata()..],
        program_data[..]
    );

    // Specify buffer keypair and max_len
    let buffer_keypair = Keypair::new();
    config.signers = vec![&keypair, &buffer_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::WriteBuffer {
        program_location: noop_path.to_str().unwrap().to_string(),
        buffer_signer_index: Some(1),
        buffer_pubkey: Some(buffer_keypair.pubkey()),
        buffer_authority_signer_index: 0,
        max_len: Some(max_len),
        skip_fee_check: false,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let buffer_pubkey_str = json
        .as_object()
        .unwrap()
        .get("buffer")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        buffer_keypair.pubkey(),
        Pubkey::from_str(buffer_pubkey_str).unwrap()
    );
    let buffer_account = rpc_client.get_account(&buffer_keypair.pubkey()).unwrap();
    assert_eq!(buffer_account.lamports, minimum_balance_for_buffer);
    assert_eq!(buffer_account.owner, bpf_loader_upgradeable::id());
    if let UpgradeableLoaderState::Buffer { authority_address } = buffer_account.state().unwrap() {
        assert_eq!(authority_address, Some(keypair.pubkey()));
    } else {
        panic!("not a buffer account");
    }
    assert_eq!(
        buffer_account.data[UpgradeableLoaderState::size_of_buffer_metadata()..],
        program_data[..]
    );

    // Get buffer authority
    config.signers = vec![];
    config.command = CliCommand::Program(ProgramCliCommand::Show {
        account_pubkey: Some(buffer_keypair.pubkey()),
        authority_pubkey: keypair.pubkey(),
        get_programs: false,
        get_buffers: false,
        all: false,
        use_lamports_unit: false,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let authority_pubkey_str = json
        .as_object()
        .unwrap()
        .get("authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        keypair.pubkey(),
        Pubkey::from_str(authority_pubkey_str).unwrap()
    );

    // Specify buffer authority
    let buffer_keypair = Keypair::new();
    let authority_keypair = Keypair::new();
    config.signers = vec![&keypair, &buffer_keypair, &authority_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::WriteBuffer {
        program_location: noop_path.to_str().unwrap().to_string(),
        buffer_signer_index: Some(1),
        buffer_pubkey: Some(buffer_keypair.pubkey()),
        buffer_authority_signer_index: 2,
        max_len: None,
        skip_fee_check: false,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let buffer_pubkey_str = json
        .as_object()
        .unwrap()
        .get("buffer")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        buffer_keypair.pubkey(),
        Pubkey::from_str(buffer_pubkey_str).unwrap()
    );
    let buffer_account = rpc_client.get_account(&buffer_keypair.pubkey()).unwrap();
    assert_eq!(buffer_account.lamports, minimum_balance_for_buffer_default);
    assert_eq!(buffer_account.owner, bpf_loader_upgradeable::id());
    if let UpgradeableLoaderState::Buffer { authority_address } = buffer_account.state().unwrap() {
        assert_eq!(authority_address, Some(authority_keypair.pubkey()));
    } else {
        panic!("not a buffer account");
    }
    assert_eq!(
        buffer_account.data[UpgradeableLoaderState::size_of_buffer_metadata()..],
        program_data[..]
    );

    // Specify authority only
    let buffer_keypair = Keypair::new();
    let authority_keypair = Keypair::new();
    config.signers = vec![&keypair, &buffer_keypair, &authority_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::WriteBuffer {
        program_location: noop_path.to_str().unwrap().to_string(),
        buffer_signer_index: None,
        buffer_pubkey: None,
        buffer_authority_signer_index: 2,
        max_len: None,
        skip_fee_check: false,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let buffer_pubkey_str = json
        .as_object()
        .unwrap()
        .get("buffer")
        .unwrap()
        .as_str()
        .unwrap();
    let buffer_pubkey = Pubkey::from_str(buffer_pubkey_str).unwrap();
    let buffer_account = rpc_client.get_account(&buffer_pubkey).unwrap();
    assert_eq!(buffer_account.lamports, minimum_balance_for_buffer_default);
    assert_eq!(buffer_account.owner, bpf_loader_upgradeable::id());
    if let UpgradeableLoaderState::Buffer { authority_address } = buffer_account.state().unwrap() {
        assert_eq!(authority_address, Some(authority_keypair.pubkey()));
    } else {
        panic!("not a buffer account");
    }
    assert_eq!(
        buffer_account.data[UpgradeableLoaderState::size_of_buffer_metadata()..],
        program_data[..]
    );

    // Get buffer authority
    config.signers = vec![];
    config.command = CliCommand::Program(ProgramCliCommand::Show {
        account_pubkey: Some(buffer_pubkey),
        authority_pubkey: keypair.pubkey(),
        get_programs: false,
        get_buffers: false,
        all: false,
        use_lamports_unit: false,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let authority_pubkey_str = json
        .as_object()
        .unwrap()
        .get("authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        authority_keypair.pubkey(),
        Pubkey::from_str(authority_pubkey_str).unwrap()
    );

    // Close buffer
    let close_account = rpc_client.get_account(&buffer_pubkey).unwrap();
    assert_eq!(minimum_balance_for_buffer, close_account.lamports);
    let recipient_pubkey = Pubkey::new_unique();
    config.signers = vec![&keypair, &authority_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::Close {
        account_pubkey: Some(buffer_pubkey),
        recipient_pubkey,
        authority_index: 1,
        use_lamports_unit: false,
        bypass_warning: false,
    });
    process_command(&config).unwrap();
    rpc_client.get_account(&buffer_pubkey).unwrap_err();
    let recipient_account = rpc_client.get_account(&recipient_pubkey).unwrap();
    assert_eq!(minimum_balance_for_buffer, recipient_account.lamports);

    // Write a buffer with default params
    config.signers = vec![&keypair];
    config.command = CliCommand::Program(ProgramCliCommand::WriteBuffer {
        program_location: noop_path.to_str().unwrap().to_string(),
        buffer_signer_index: None,
        buffer_pubkey: None,
        buffer_authority_signer_index: 0,
        max_len: None,
        skip_fee_check: false,
    });
    config.output_format = OutputFormat::JsonCompact;
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let buffer_pubkey_str = json
        .as_object()
        .unwrap()
        .get("buffer")
        .unwrap()
        .as_str()
        .unwrap();
    let new_buffer_pubkey = Pubkey::from_str(buffer_pubkey_str).unwrap();

    // Close buffers and deposit default keypair
    let pre_lamports = rpc_client.get_account(&keypair.pubkey()).unwrap().lamports;
    config.signers = vec![&keypair];
    config.command = CliCommand::Program(ProgramCliCommand::Close {
        account_pubkey: Some(new_buffer_pubkey),
        recipient_pubkey: keypair.pubkey(),
        authority_index: 0,
        use_lamports_unit: false,
        bypass_warning: false,
    });
    process_command(&config).unwrap();
    rpc_client.get_account(&new_buffer_pubkey).unwrap_err();
    let recipient_account = rpc_client.get_account(&keypair.pubkey()).unwrap();
    assert_eq!(
        pre_lamports + minimum_balance_for_buffer,
        recipient_account.lamports
    );

    // write small buffer then attempt to deploy larger program
    let buffer_keypair = Keypair::new();
    config.signers = vec![&keypair, &buffer_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::WriteBuffer {
        program_location: noop_path.to_str().unwrap().to_string(),
        buffer_signer_index: Some(1),
        buffer_pubkey: Some(buffer_keypair.pubkey()),
        buffer_authority_signer_index: 0,
        max_len: None, //Some(max_len),
        skip_fee_check: false,
    });
    process_command(&config).unwrap();
    config.signers = vec![&keypair, &buffer_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_large_path.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: None,
        buffer_signer_index: Some(1),
        buffer_pubkey: Some(buffer_keypair.pubkey()),
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: true,
        max_len: None,
        skip_fee_check: false,
    });
    config.output_format = OutputFormat::JsonCompact;
    let error = process_command(&config).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Buffer account passed is not large enough, may have been for a different deploy?"
    );
}

#[test]
fn test_cli_program_set_buffer_authority() {
    solana_logger::setup();

    let mut noop_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    noop_path.push("tests");
    noop_path.push("fixtures");
    noop_path.push("noop");
    noop_path.set_extension("so");

    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, Some(faucet_addr), SocketAddrSpace::Unspecified);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let mut file = File::open(noop_path.to_str().unwrap()).unwrap();
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).unwrap();
    let max_len = program_data.len();
    let minimum_balance_for_buffer = rpc_client
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_programdata(
            max_len,
        ))
        .unwrap();

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&keypair];
    config.command = CliCommand::Airdrop {
        pubkey: None,
        lamports: 100 * minimum_balance_for_buffer,
    };
    process_command(&config).unwrap();

    // Write a buffer
    let buffer_keypair = Keypair::new();
    config.signers = vec![&keypair, &buffer_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::WriteBuffer {
        program_location: noop_path.to_str().unwrap().to_string(),
        buffer_signer_index: Some(1),
        buffer_pubkey: Some(buffer_keypair.pubkey()),
        buffer_authority_signer_index: 0,
        max_len: None,
        skip_fee_check: false,
    });
    process_command(&config).unwrap();
    let buffer_account = rpc_client.get_account(&buffer_keypair.pubkey()).unwrap();
    if let UpgradeableLoaderState::Buffer { authority_address } = buffer_account.state().unwrap() {
        assert_eq!(authority_address, Some(keypair.pubkey()));
    } else {
        panic!("not a buffer account");
    }

    // Set new authority
    let new_buffer_authority = Keypair::new();
    config.signers = vec![&keypair, &buffer_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::SetBufferAuthority {
        buffer_pubkey: buffer_keypair.pubkey(),
        buffer_authority_index: Some(0),
        new_buffer_authority: new_buffer_authority.pubkey(),
    });
    config.output_format = OutputFormat::JsonCompact;
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let new_buffer_authority_str = json
        .as_object()
        .unwrap()
        .get("authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        Pubkey::from_str(new_buffer_authority_str).unwrap(),
        new_buffer_authority.pubkey()
    );
    let buffer_account = rpc_client.get_account(&buffer_keypair.pubkey()).unwrap();
    if let UpgradeableLoaderState::Buffer { authority_address } = buffer_account.state().unwrap() {
        assert_eq!(authority_address, Some(new_buffer_authority.pubkey()));
    } else {
        panic!("not a buffer account");
    }

    // Set authority to buffer
    config.signers = vec![&keypair, &new_buffer_authority];
    config.command = CliCommand::Program(ProgramCliCommand::SetBufferAuthority {
        buffer_pubkey: buffer_keypair.pubkey(),
        buffer_authority_index: Some(1),
        new_buffer_authority: buffer_keypair.pubkey(),
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let buffer_authority_str = json
        .as_object()
        .unwrap()
        .get("authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        Pubkey::from_str(buffer_authority_str).unwrap(),
        buffer_keypair.pubkey()
    );
    let buffer_account = rpc_client.get_account(&buffer_keypair.pubkey()).unwrap();
    if let UpgradeableLoaderState::Buffer { authority_address } = buffer_account.state().unwrap() {
        assert_eq!(authority_address, Some(buffer_keypair.pubkey()));
    } else {
        panic!("not a buffer account");
    }
}

#[test]
fn test_cli_program_mismatch_buffer_authority() {
    solana_logger::setup();

    let mut noop_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    noop_path.push("tests");
    noop_path.push("fixtures");
    noop_path.push("noop");
    noop_path.set_extension("so");

    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, Some(faucet_addr), SocketAddrSpace::Unspecified);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let mut file = File::open(noop_path.to_str().unwrap()).unwrap();
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).unwrap();
    let max_len = program_data.len();
    let minimum_balance_for_buffer = rpc_client
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_programdata(
            max_len,
        ))
        .unwrap();

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&keypair];
    config.command = CliCommand::Airdrop {
        pubkey: None,
        lamports: 100 * minimum_balance_for_buffer,
    };
    process_command(&config).unwrap();

    // Write a buffer
    let buffer_authority = Keypair::new();
    let buffer_keypair = Keypair::new();
    config.signers = vec![&keypair, &buffer_keypair, &buffer_authority];
    config.command = CliCommand::Program(ProgramCliCommand::WriteBuffer {
        program_location: noop_path.to_str().unwrap().to_string(),
        buffer_signer_index: Some(1),
        buffer_pubkey: Some(buffer_keypair.pubkey()),
        buffer_authority_signer_index: 2,
        max_len: None,
        skip_fee_check: false,
    });
    process_command(&config).unwrap();
    let buffer_account = rpc_client.get_account(&buffer_keypair.pubkey()).unwrap();
    if let UpgradeableLoaderState::Buffer { authority_address } = buffer_account.state().unwrap() {
        assert_eq!(authority_address, Some(buffer_authority.pubkey()));
    } else {
        panic!("not a buffer account");
    }

    // Attempt to deploy with mismatched authority
    let upgrade_authority = Keypair::new();
    config.signers = vec![&keypair, &upgrade_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_path.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: None,
        buffer_signer_index: None,
        buffer_pubkey: Some(buffer_keypair.pubkey()),
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: true,
        max_len: None,
        skip_fee_check: false,
    });
    process_command(&config).unwrap_err();

    // Attempt to deploy matched authority
    config.signers = vec![&keypair, &buffer_authority];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_path.to_str().unwrap().to_string()),
        program_signer_index: None,
        program_pubkey: None,
        buffer_signer_index: None,
        buffer_pubkey: Some(buffer_keypair.pubkey()),
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: true,
        max_len: None,
        skip_fee_check: false,
    });
    process_command(&config).unwrap();
}

#[test]
fn test_cli_program_show() {
    solana_logger::setup();

    let mut noop_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    noop_path.push("tests");
    noop_path.push("fixtures");
    noop_path.push("noop");
    noop_path.set_extension("so");

    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, Some(faucet_addr), SocketAddrSpace::Unspecified);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let mut file = File::open(noop_path.to_str().unwrap()).unwrap();
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).unwrap();
    let max_len = program_data.len();
    let minimum_balance_for_buffer = rpc_client
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_programdata(
            max_len,
        ))
        .unwrap();

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.output_format = OutputFormat::Json;

    // Airdrop
    config.signers = vec![&keypair];
    config.command = CliCommand::Airdrop {
        pubkey: None,
        lamports: 100 * minimum_balance_for_buffer,
    };
    process_command(&config).unwrap();

    // Write a buffer
    let buffer_keypair = Keypair::new();
    let authority_keypair = Keypair::new();
    config.signers = vec![&keypair, &buffer_keypair, &authority_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::WriteBuffer {
        program_location: noop_path.to_str().unwrap().to_string(),
        buffer_signer_index: Some(1),
        buffer_pubkey: Some(buffer_keypair.pubkey()),
        buffer_authority_signer_index: 2,
        max_len: None,
        skip_fee_check: false,
    });
    process_command(&config).unwrap();

    // Verify show
    config.signers = vec![&keypair];
    config.command = CliCommand::Program(ProgramCliCommand::Show {
        account_pubkey: Some(buffer_keypair.pubkey()),
        authority_pubkey: keypair.pubkey(),
        get_programs: false,
        get_buffers: false,
        all: false,
        use_lamports_unit: false,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let address_str = json
        .as_object()
        .unwrap()
        .get("address")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        buffer_keypair.pubkey(),
        Pubkey::from_str(address_str).unwrap()
    );
    let authority_str = json
        .as_object()
        .unwrap()
        .get("authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        authority_keypair.pubkey(),
        Pubkey::from_str(authority_str).unwrap()
    );
    let data_len = json
        .as_object()
        .unwrap()
        .get("dataLen")
        .unwrap()
        .as_u64()
        .unwrap();
    assert_eq!(max_len, data_len as usize);

    // Deploy
    let program_keypair = Keypair::new();
    config.signers = vec![&keypair, &authority_keypair, &program_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::Deploy {
        program_location: Some(noop_path.to_str().unwrap().to_string()),
        program_signer_index: Some(2),
        program_pubkey: Some(program_keypair.pubkey()),
        buffer_signer_index: None,
        buffer_pubkey: None,
        allow_excessive_balance: false,
        upgrade_authority_signer_index: 1,
        is_final: false,
        max_len: Some(max_len),
        skip_fee_check: false,
    });
    config.output_format = OutputFormat::JsonCompact;
    let min_slot = rpc_client.get_slot().unwrap();
    process_command(&config).unwrap();
    let max_slot = rpc_client.get_slot().unwrap();

    // Verify show
    config.signers = vec![&keypair];
    config.command = CliCommand::Program(ProgramCliCommand::Show {
        account_pubkey: Some(program_keypair.pubkey()),
        authority_pubkey: keypair.pubkey(),
        get_programs: false,
        get_buffers: false,
        all: false,
        use_lamports_unit: false,
    });
    let response = process_command(&config);
    let json: Value = serde_json::from_str(&response.unwrap()).unwrap();
    let address_str = json
        .as_object()
        .unwrap()
        .get("programId")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        program_keypair.pubkey(),
        Pubkey::from_str(address_str).unwrap()
    );
    let programdata_address_str = json
        .as_object()
        .unwrap()
        .get("programdataAddress")
        .unwrap()
        .as_str()
        .unwrap();
    let (programdata_pubkey, _) = Pubkey::find_program_address(
        &[program_keypair.pubkey().as_ref()],
        &bpf_loader_upgradeable::id(),
    );
    assert_eq!(
        programdata_pubkey,
        Pubkey::from_str(programdata_address_str).unwrap()
    );
    let authority_str = json
        .as_object()
        .unwrap()
        .get("authority")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(
        authority_keypair.pubkey(),
        Pubkey::from_str(authority_str).unwrap()
    );
    let deployed_slot = json
        .as_object()
        .unwrap()
        .get("lastDeploySlot")
        .unwrap()
        .as_u64()
        .unwrap();
    assert!(deployed_slot >= min_slot);
    assert!(deployed_slot <= max_slot);
    let data_len = json
        .as_object()
        .unwrap()
        .get("dataLen")
        .unwrap()
        .as_u64()
        .unwrap();
    assert_eq!(max_len, data_len as usize);
}

#[test]
fn test_cli_program_dump() {
    solana_logger::setup();

    let mut noop_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    noop_path.push("tests");
    noop_path.push("fixtures");
    noop_path.push("noop");
    noop_path.set_extension("so");

    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, Some(faucet_addr), SocketAddrSpace::Unspecified);

    let rpc_client =
        RpcClient::new_with_commitment(test_validator.rpc_url(), CommitmentConfig::processed());

    let mut file = File::open(noop_path.to_str().unwrap()).unwrap();
    let mut program_data = Vec::new();
    file.read_to_end(&mut program_data).unwrap();
    let max_len = program_data.len();
    let minimum_balance_for_buffer = rpc_client
        .get_minimum_balance_for_rent_exemption(UpgradeableLoaderState::size_of_programdata(
            max_len,
        ))
        .unwrap();

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.output_format = OutputFormat::Json;

    // Airdrop
    config.signers = vec![&keypair];
    config.command = CliCommand::Airdrop {
        pubkey: None,
        lamports: 100 * minimum_balance_for_buffer,
    };
    process_command(&config).unwrap();

    // Write a buffer
    let buffer_keypair = Keypair::new();
    let authority_keypair = Keypair::new();
    config.signers = vec![&keypair, &buffer_keypair, &authority_keypair];
    config.command = CliCommand::Program(ProgramCliCommand::WriteBuffer {
        program_location: noop_path.to_str().unwrap().to_string(),
        buffer_signer_index: Some(1),
        buffer_pubkey: Some(buffer_keypair.pubkey()),
        buffer_authority_signer_index: 2,
        max_len: None,
        skip_fee_check: false,
    });
    process_command(&config).unwrap();

    // Verify dump
    let mut out_file = {
        let current_exe = env::current_exe().unwrap();
        PathBuf::from(current_exe.parent().unwrap().parent().unwrap())
    };
    out_file.set_file_name("out.txt");
    config.signers = vec![&keypair];
    config.command = CliCommand::Program(ProgramCliCommand::Dump {
        account_pubkey: Some(buffer_keypair.pubkey()),
        output_location: out_file.clone().into_os_string().into_string().unwrap(),
    });
    process_command(&config).unwrap();

    let mut file = File::open(out_file).unwrap();
    let mut out_data = Vec::new();
    file.read_to_end(&mut out_data).unwrap();
    assert_eq!(program_data.len(), out_data.len());
    for i in 0..program_data.len() {
        assert_eq!(program_data[i], out_data[i]);
    }
}
