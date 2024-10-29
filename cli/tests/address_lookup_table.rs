use {
    solana_cli::{
        address_lookup_table::{
            AddressLookupTableCliCommand, DEACTIVATE_LOOKUP_TABLE_WARNING,
            FREEZE_LOOKUP_TABLE_WARNING,
        },
        cli::{process_command, CliCommand, CliConfig},
    },
    solana_cli_output::{CliAddressLookupTable, CliAddressLookupTableCreated, OutputFormat},
    solana_faucet::faucet::run_local_faucet,
    solana_sdk::{
        native_token::LAMPORTS_PER_SOL,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
    },
    solana_streamer::socket::SocketAddrSpace,
    solana_test_validator::TestValidator,
    std::str::FromStr,
};

#[test]
fn test_cli_create_extend_and_freeze_address_lookup_table() {
    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, Some(faucet_addr), SocketAddrSpace::Unspecified);

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&keypair];
    config.output_format = OutputFormat::JsonCompact;

    // Airdrop SOL for transaction fees
    config.command = CliCommand::Airdrop {
        pubkey: None,
        lamports: 10 * LAMPORTS_PER_SOL,
    };
    process_command(&config).unwrap();

    // Create lookup table
    config.command =
        CliCommand::AddressLookupTable(AddressLookupTableCliCommand::CreateLookupTable {
            authority_pubkey: keypair.pubkey(),
            authority_signer_index: None,
            payer_signer_index: 0,
        });
    let response: CliAddressLookupTableCreated =
        serde_json::from_str(&process_command(&config).unwrap()).unwrap();
    let lookup_table_pubkey = Pubkey::from_str(&response.lookup_table_address).unwrap();

    // Validate created lookup table
    {
        config.command =
            CliCommand::AddressLookupTable(AddressLookupTableCliCommand::ShowLookupTable {
                lookup_table_pubkey,
            });
        let response: CliAddressLookupTable =
            serde_json::from_str(&process_command(&config).unwrap()).unwrap();
        assert_eq!(
            response,
            CliAddressLookupTable {
                lookup_table_address: lookup_table_pubkey.to_string(),
                authority: Some(keypair.pubkey().to_string()),
                deactivation_slot: u64::MAX,
                last_extended_slot: 0,
                addresses: vec![],
            }
        );
    }

    // Extend lookup table
    let new_addresses: Vec<Pubkey> = (0..5).map(|_| Pubkey::new_unique()).collect();
    config.command =
        CliCommand::AddressLookupTable(AddressLookupTableCliCommand::ExtendLookupTable {
            lookup_table_pubkey,
            authority_signer_index: 0,
            payer_signer_index: 0,
            new_addresses: new_addresses.clone(),
        });
    process_command(&config).unwrap();

    // Validate extended lookup table
    {
        config.command =
            CliCommand::AddressLookupTable(AddressLookupTableCliCommand::ShowLookupTable {
                lookup_table_pubkey,
            });
        let CliAddressLookupTable {
            addresses,
            last_extended_slot,
            ..
        } = serde_json::from_str(&process_command(&config).unwrap()).unwrap();
        assert_eq!(
            addresses
                .into_iter()
                .map(|address| Pubkey::from_str(&address).unwrap())
                .collect::<Vec<Pubkey>>(),
            new_addresses
        );
        assert!(last_extended_slot > 0);
    }

    // Freeze lookup table w/o bypass
    config.command =
        CliCommand::AddressLookupTable(AddressLookupTableCliCommand::FreezeLookupTable {
            lookup_table_pubkey,
            authority_signer_index: 0,
            bypass_warning: false,
        });
    let process_err = process_command(&config).unwrap_err();
    assert_eq!(process_err.to_string(), FREEZE_LOOKUP_TABLE_WARNING);

    // Freeze lookup table w/ bypass
    config.command =
        CliCommand::AddressLookupTable(AddressLookupTableCliCommand::FreezeLookupTable {
            lookup_table_pubkey,
            authority_signer_index: 0,
            bypass_warning: true,
        });
    process_command(&config).unwrap();

    // Validate frozen lookup table
    {
        config.command =
            CliCommand::AddressLookupTable(AddressLookupTableCliCommand::ShowLookupTable {
                lookup_table_pubkey,
            });
        let CliAddressLookupTable { authority, .. } =
            serde_json::from_str(&process_command(&config).unwrap()).unwrap();
        assert!(authority.is_none());
    }
}

#[test]
fn test_cli_create_and_deactivate_address_lookup_table() {
    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    let faucet_addr = run_local_faucet(mint_keypair, None);
    let test_validator =
        TestValidator::with_no_fees(mint_pubkey, Some(faucet_addr), SocketAddrSpace::Unspecified);

    let mut config = CliConfig::recent_for_tests();
    let keypair = Keypair::new();
    config.json_rpc_url = test_validator.rpc_url();
    config.signers = vec![&keypair];
    config.output_format = OutputFormat::JsonCompact;

    // Airdrop SOL for transaction fees
    config.command = CliCommand::Airdrop {
        pubkey: None,
        lamports: 10 * LAMPORTS_PER_SOL,
    };
    process_command(&config).unwrap();

    // Create lookup table
    config.command =
        CliCommand::AddressLookupTable(AddressLookupTableCliCommand::CreateLookupTable {
            authority_pubkey: keypair.pubkey(),
            authority_signer_index: Some(0),
            payer_signer_index: 0,
        });
    let response: CliAddressLookupTableCreated =
        serde_json::from_str(&process_command(&config).unwrap()).unwrap();
    let lookup_table_pubkey = Pubkey::from_str(&response.lookup_table_address).unwrap();

    // Validate created lookup table
    {
        config.command =
            CliCommand::AddressLookupTable(AddressLookupTableCliCommand::ShowLookupTable {
                lookup_table_pubkey,
            });
        let response: CliAddressLookupTable =
            serde_json::from_str(&process_command(&config).unwrap()).unwrap();
        assert_eq!(
            response,
            CliAddressLookupTable {
                lookup_table_address: lookup_table_pubkey.to_string(),
                authority: Some(keypair.pubkey().to_string()),
                deactivation_slot: u64::MAX,
                last_extended_slot: 0,
                addresses: vec![],
            }
        );
    }

    // Deactivate lookup table w/o bypass
    config.command =
        CliCommand::AddressLookupTable(AddressLookupTableCliCommand::DeactivateLookupTable {
            lookup_table_pubkey,
            authority_signer_index: 0,
            bypass_warning: false,
        });
    let process_err = process_command(&config).unwrap_err();
    assert_eq!(process_err.to_string(), DEACTIVATE_LOOKUP_TABLE_WARNING);

    // Deactivate lookup table w/ bypass
    config.command =
        CliCommand::AddressLookupTable(AddressLookupTableCliCommand::DeactivateLookupTable {
            lookup_table_pubkey,
            authority_signer_index: 0,
            bypass_warning: true,
        });
    process_command(&config).unwrap();

    // Validate deactivated lookup table
    {
        config.command =
            CliCommand::AddressLookupTable(AddressLookupTableCliCommand::ShowLookupTable {
                lookup_table_pubkey,
            });
        let CliAddressLookupTable {
            deactivation_slot, ..
        } = serde_json::from_str(&process_command(&config).unwrap()).unwrap();
        assert_ne!(deactivation_slot, u64::MAX);
    }
}
