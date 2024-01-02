// TODO docs

// XXX found a note that "new rbpf features" must be added to "Bank::apply_builtin_program_feature_transitions()"
// i dont think this applies to me because i didnt see any other syscall feature flags there

pub mod deprecate_rewards_sysvar {
    crate::declare_id!("GaBtBJvmS4Arjj5W1NmFcyvPjsHN38UGYDq2MDwbs9Qu");
}

pub mod pico_inflation {
    crate::declare_id!("4RWNif6C2WCNiKVW7otP4G7dkmkHGyKQWRpuZ1pxKU5m");
}

pub mod full_inflation {
    pub mod devnet_and_testnet {
        crate::declare_id!("DT4n6ABDqs6w4bnfwrXT9rsprcPf6cdDga1egctaPkLC");
    }

    pub mod mainnet {
        pub mod certusone {
            pub mod vote {
                crate::declare_id!("BzBBveUDymEYoYzcMWNQCx3cd4jQs7puaVFHLtsbB6fm");
            }
            pub mod enable {
                crate::declare_id!("7XRJcS5Ud5vxGB54JbK9N2vBZVwnwdBNeJW1ibRgD9gx");
            }
        }
    }
}

pub mod secp256k1_program_enabled {
    crate::declare_id!("E3PHP7w8kB7np3CTQ1qQ2tW3KCtjRSXBQgW9vM2mWv2Y");
}

pub mod spl_token_v2_multisig_fix {
    crate::declare_id!("E5JiFDQCwyC6QfT9REFyMpfK2mHcmv1GUDySU1Ue7TYv");
}

pub mod no_overflow_rent_distribution {
    crate::declare_id!("4kpdyrcj5jS47CZb2oJGfVxjYbsMm2Kx97gFyZrxxwXz");
}

pub mod filter_stake_delegation_accounts {
    crate::declare_id!("GE7fRxmW46K6EmCD9AMZSbnaJ2e3LfqCZzdHi9hmYAgi");
}

pub mod require_custodian_for_locked_stake_authorize {
    crate::declare_id!("D4jsDcXaqdW8tDAWn8H4R25Cdns2YwLneujSL1zvjW6R");
}

pub mod spl_token_v2_self_transfer_fix {
    crate::declare_id!("BL99GYhdjjcv6ys22C9wPgn2aTVERDbPHHo4NbS3hgp7");
}

pub mod warp_timestamp_again {
    crate::declare_id!("GvDsGDkH5gyzwpDhxNixx8vtx1kwYHH13RiNAPw27zXb");
}

pub mod check_init_vote_data {
    crate::declare_id!("3ccR6QpxGYsAbWyfevEtBNGfWV4xBffxRj2tD6A9i39F");
}

pub mod secp256k1_recover_syscall_enabled {
    crate::declare_id!("6RvdSWHh8oh72Dp7wMTS2DBkf3fRPtChfNrAo3cZZoXJ");
}

pub mod system_transfer_zero_check {
    crate::declare_id!("BrTR9hzw4WBGFP65AJMbpAo64DcA3U6jdPSga9fMV5cS");
}

pub mod blake3_syscall_enabled {
    crate::declare_id!("HTW2pSyErTj4BV6KBM9NZ9VBUJVxt7sacNWcf76wtzb3");
}

pub mod dedupe_config_program_signers {
    crate::declare_id!("8kEuAshXLsgkUEdcFVLqrjCGGHVWFW99ZZpxvAzzMtBp");
}

pub mod verify_tx_signatures_len {
    crate::declare_id!("EVW9B5xD9FFK7vw1SBARwMA4s5eRo5eKJdKpsBikzKBz");
}

pub mod vote_stake_checked_instructions {
    crate::declare_id!("BcWknVcgvonN8sL4HE4XFuEVgfcee5MwxWPAgP6ZV89X");
}

pub mod rent_for_sysvars {
    crate::declare_id!("BKCPBQQBZqggVnFso5nQ8rQ4RwwogYwjuUt9biBjxwNF");
}

pub mod libsecp256k1_0_5_upgrade_enabled {
    crate::declare_id!("DhsYfRjxfnh2g7HKJYSzT79r74Afa1wbHkAgHndrA1oy");
}

pub mod tx_wide_compute_cap {
    crate::declare_id!("5ekBxc8itEnPv4NzGJtr8BVVQLNMQuLMNQQj7pHoLNZ9");
}

pub mod spl_token_v2_set_authority_fix {
    crate::declare_id!("FToKNBYyiF4ky9s8WsmLBXHCht17Ek7RXaLZGHzzQhJ1");
}

pub mod merge_nonce_error_into_system_error {
    crate::declare_id!("21AWDosvp3pBamFW91KB35pNoaoZVTM7ess8nr2nt53B");
}

pub mod disable_fees_sysvar {
    crate::declare_id!("JAN1trEUEtZjgXYzNBYHU9DYd7GnThhXfFP7SzPXkPsG");
}

pub mod stake_merge_with_unmatched_credits_observed {
    crate::declare_id!("meRgp4ArRPhD3KtCY9c5yAf2med7mBLsjKTPeVUHqBL");
}

pub mod zk_token_sdk_enabled {
    crate::declare_id!("zk1snxsc6Fh3wsGNbbHAJNHiJoYgF29mMnTSusGx5EJ");
}

pub mod curve25519_syscall_enabled {
    crate::declare_id!("7rcw5UtqgDTBBv2EcynNfYckgdAaH1MAsCjKgXMkN7Ri");
}

pub mod versioned_tx_message_enabled {
    crate::declare_id!("3KZZ6Ks1885aGBQ45fwRcPXVBCtzUvxhUTkwKMR41Tca");
}

pub mod libsecp256k1_fail_on_bad_count {
    crate::declare_id!("8aXvSuopd1PUj7UhehfXJRg6619RHp8ZvwTyyJHdUYsj");
}

pub mod libsecp256k1_fail_on_bad_count2 {
    crate::declare_id!("54KAoNiUERNoWWUhTWWwXgym94gzoXFVnHyQwPA18V9A");
}

pub mod instructions_sysvar_owned_by_sysvar {
    crate::declare_id!("H3kBSaKdeiUsyHmeHqjJYNc27jesXZ6zWj3zWkowQbkV");
}

pub mod stake_program_advance_activating_credits_observed {
    crate::declare_id!("SAdVFw3RZvzbo6DvySbSdBnHN4gkzSTH9dSxesyKKPj");
}

pub mod credits_auto_rewind {
    crate::declare_id!("BUS12ciZ5gCoFafUHWW8qaFMMtwFQGVxjsDheWLdqBE2");
}

pub mod demote_program_write_locks {
    crate::declare_id!("3E3jV7v9VcdJL8iYZUMax9DiDno8j7EWUVbhm9RtShj2");
}

pub mod ed25519_program_enabled {
    crate::declare_id!("6ppMXNYLhVd7GcsZ5uV11wQEW7spppiMVfqQv5SXhDpX");
}

pub mod return_data_syscall_enabled {
    crate::declare_id!("DwScAzPUjuv65TMbDnFY7AgwmotzWy3xpEJMXM3hZFaB");
}

pub mod reduce_required_deploy_balance {
    crate::declare_id!("EBeznQDjcPG8491sFsKZYBi5S5jTVXMpAKNDJMQPS2kq");
}

pub mod sol_log_data_syscall_enabled {
    crate::declare_id!("6uaHcKPGUy4J7emLBgUTeufhJdiwhngW6a1R9B7c2ob9");
}

pub mod stakes_remove_delegation_if_inactive {
    crate::declare_id!("HFpdDDNQjvcXnXKec697HDDsyk6tFoWS2o8fkxuhQZpL");
}

pub mod do_support_realloc {
    crate::declare_id!("75m6ysz33AfLA5DDEzWM1obBrnPQRSsdVQ2nRmc8Vuu1");
}

pub mod prevent_calling_precompiles_as_programs {
    crate::declare_id!("4ApgRX3ud6p7LNMJmsuaAcZY5HWctGPr5obAsjB3A54d");
}

pub mod optimize_epoch_boundary_updates {
    crate::declare_id!("265hPS8k8xJ37ot82KEgjRunsUp5w4n4Q4VwwiN9i9ps");
}

pub mod remove_native_loader {
    crate::declare_id!("HTTgmruMYRZEntyL3EdCDdnS6e4D5wRq1FA7kQsb66qq");
}

pub mod send_to_tpu_vote_port {
    crate::declare_id!("C5fh68nJ7uyKAuYZg2x9sEQ5YrVf3dkW6oojNBSc3Jvo");
}

pub mod requestable_heap_size {
    crate::declare_id!("CCu4boMmfLuqcmfTLPHQiUo22ZdUsXjgzPAURYaWt1Bw");
}

pub mod disable_fee_calculator {
    crate::declare_id!("2jXx2yDmGysmBKfKYNgLj2DQyAQv6mMk2BPh4eSbyB4H");
}

pub mod add_compute_budget_program {
    crate::declare_id!("4d5AKtxoh93Dwm1vHXUU3iRATuMndx1c431KgT2td52r");
}

pub mod nonce_must_be_writable {
    crate::declare_id!("BiCU7M5w8ZCMykVSyhZ7Q3m2SWoR2qrEQ86ERcDX77ME");
}

pub mod spl_token_v3_3_0_release {
    crate::declare_id!("Ftok2jhqAqxUWEiCVRrfRs9DPppWP8cgTB7NQNKL88mS");
}

pub mod leave_nonce_on_success {
    crate::declare_id!("E8MkiWZNNPGU6n55jkGzyj8ghUmjCHRmDFdYYFYHxWhQ");
}

pub mod reject_empty_instruction_without_program {
    crate::declare_id!("9kdtFSrXHQg3hKkbXkQ6trJ3Ja1xpJ22CTFSNAciEwmL");
}

pub mod fixed_memcpy_nonoverlapping_check {
    crate::declare_id!("36PRUK2Dz6HWYdG9SpjeAsF5F3KxnFCakA2BZMbtMhSb");
}

pub mod reject_non_rent_exempt_vote_withdraws {
    crate::declare_id!("7txXZZD6Um59YoLMF7XUNimbMjsqsWhc7g2EniiTrmp1");
}

pub mod evict_invalid_stakes_cache_entries {
    crate::declare_id!("EMX9Q7TVFAmQ9V1CggAkhMzhXSg8ECp7fHrWQX2G1chf");
}

pub mod allow_votes_to_directly_update_vote_state {
    crate::declare_id!("Ff8b1fBeB86q8cjq47ZhsQLgv5EkHu3G1C99zjUfAzrq");
}

pub mod cap_accounts_data_len {
    crate::declare_id!("capRxUrBjNkkCpjrJxPGfPaWijB7q3JoDfsWXAnt46r");
}

pub mod max_tx_account_locks {
    crate::declare_id!("CBkDroRDqm8HwHe6ak9cguPjUomrASEkfmxEaZ5CNNxz");
}

pub mod require_rent_exempt_accounts {
    crate::declare_id!("BkFDxiJQWZXGTZaJQxH7wVEHkAmwCgSEVkrvswFfRJPD");
}

pub mod filter_votes_outside_slot_hashes {
    crate::declare_id!("3gtZPqvPpsbXZVCx6hceMfWxtsmrjMzmg8C7PLKSxS2d");
}

pub mod update_syscall_base_costs {
    crate::declare_id!("2h63t332mGCCsWK2nqqqHhN4U9ayyqhLVFvczznHDoTZ");
}

pub mod stake_deactivate_delinquent_instruction {
    crate::declare_id!("437r62HoAdUb63amq3D7ENnBLDhHT2xY8eFkLJYVKK4x");
}

pub mod stake_redelegate_instruction {
    crate::declare_id!("2KKG3C6RBnxQo9jVVrbzsoSh41TDXLK7gBc9gduyxSzW");
}

pub mod vote_withdraw_authority_may_change_authorized_voter {
    crate::declare_id!("AVZS3ZsN4gi6Rkx2QUibYuSJG3S6QHib7xCYhG6vGJxU");
}

pub mod spl_associated_token_account_v1_0_4 {
    crate::declare_id!("FaTa4SpiaSNH44PGC4z8bnGVTkSRYaWvrBs3KTu8XQQq");
}

pub mod reject_vote_account_close_unless_zero_credit_epoch {
    crate::declare_id!("ALBk3EWdeAg2WAGf6GPDUf1nynyNqCdEVmgouG7rpuCj");
}

pub mod add_get_processed_sibling_instruction_syscall {
    crate::declare_id!("CFK1hRCNy8JJuAAY8Pb2GjLFNdCThS2qwZNe3izzBMgn");
}

pub mod bank_transaction_count_fix {
    crate::declare_id!("Vo5siZ442SaZBKPXNocthiXysNviW4UYPwRFggmbgAp");
}

pub mod disable_bpf_deprecated_load_instructions {
    crate::declare_id!("3XgNukcZWf9o3HdA3fpJbm94XFc4qpvTXc8h1wxYwiPi");
}

pub mod disable_bpf_unresolved_symbols_at_runtime {
    crate::declare_id!("4yuaYAj2jGMGTh1sSmi4G2eFscsDq8qjugJXZoBN6YEa");
}

pub mod record_instruction_in_transaction_context_push {
    crate::declare_id!("3aJdcZqxoLpSBxgeYGjPwaYS1zzcByxUDqJkbzWAH1Zb");
}

pub mod syscall_saturated_math {
    crate::declare_id!("HyrbKftCdJ5CrUfEti6x26Cj7rZLNe32weugk7tLcWb8");
}

pub mod check_physical_overlapping {
    crate::declare_id!("nWBqjr3gpETbiaVj3CBJ3HFC5TMdnJDGt21hnvSTvVZ");
}

pub mod limit_secp256k1_recovery_id {
    crate::declare_id!("7g9EUwj4j7CS21Yx1wvgWLjSZeh5aPq8x9kpoPwXM8n8");
}

pub mod disable_deprecated_loader {
    crate::declare_id!("GTUMCZ8LTNxVfxdrw7ZsDFTxXb7TutYkzJnFwinpE6dg");
}

pub mod check_slice_translation_size {
    crate::declare_id!("GmC19j9qLn2RFk5NduX6QXaDhVpGncVVBzyM8e9WMz2F");
}

pub mod stake_split_uses_rent_sysvar {
    crate::declare_id!("FQnc7U4koHqWgRvFaBJjZnV8VPg6L6wWK33yJeDp4yvV");
}

pub mod add_get_minimum_delegation_instruction_to_stake_program {
    crate::declare_id!("St8k9dVXP97xT6faW24YmRSYConLbhsMJA4TJTBLmMT");
}

pub mod error_on_syscall_bpf_function_hash_collisions {
    crate::declare_id!("8199Q2gMD2kwgfopK5qqVWuDbegLgpuFUFHCcUJQDN8b");
}

pub mod reject_callx_r10 {
    crate::declare_id!("3NKRSwpySNwD3TvP5pHnRmkAQRsdkXWRr1WaQh8p4PWX");
}

pub mod drop_redundant_turbine_path {
    crate::declare_id!("4Di3y24QFLt5QEUPZtbnjyfQKfm6ZMTfa6Dw1psfoMKU");
}

pub mod executables_incur_cpi_data_cost {
    crate::declare_id!("7GUcYgq4tVtaqNCKT3dho9r4665Qp5TxCZ27Qgjx3829");
}

pub mod fix_recent_blockhashes {
    crate::declare_id!("6iyggb5MTcsvdcugX7bEKbHV8c6jdLbpHwkncrgLMhfo");
}

pub mod update_rewards_from_cached_accounts {
    crate::declare_id!("28s7i3htzhahXQKqmS2ExzbEoUypg9krwvtK2M9UWXh9");
}
pub mod enable_partitioned_epoch_reward {
    crate::declare_id!("HCnE3xQoZtDz9dSVm3jKwJXioTb6zMRbgwCmGg3PHHk8");
}

pub mod spl_token_v3_4_0 {
    crate::declare_id!("Ftok4njE8b7tDffYkC5bAbCaQv5sL6jispYrprzatUwN");
}

pub mod spl_associated_token_account_v1_1_0 {
    crate::declare_id!("FaTa17gVKoqbh38HcfiQonPsAaQViyDCCSg71AubYZw8");
}

pub mod default_units_per_instruction {
    crate::declare_id!("J2QdYx8crLbTVK8nur1jeLsmc3krDbfjoxoea2V1Uy5Q");
}

pub mod stake_allow_zero_undelegated_amount {
    crate::declare_id!("sTKz343FM8mqtyGvYWvbLpTThw3ixRM4Xk8QvZ985mw");
}

pub mod require_static_program_ids_in_transaction {
    crate::declare_id!("8FdwgyHFEjhAdjWfV2vfqk7wA1g9X3fQpKH7SBpEv3kC");
}

pub mod stake_raise_minimum_delegation_to_1_sol {
    // This is a feature-proposal *feature id*.  The feature keypair address is `GQXzC7YiSNkje6FFUk6sc2p53XRvKoaZ9VMktYzUMnpL`.
    crate::declare_id!("9onWzzvCzNC2jfhxxeqRgs5q7nFAAKpCUvkj6T6GJK9i");
}

pub mod stake_minimum_delegation_for_rewards {
    crate::declare_id!("ELjxSXwNsyXGfAh8TqX8ih22xeT8huF6UngQirbLKYKH");
}

pub mod add_set_compute_unit_price_ix {
    crate::declare_id!("98std1NSHqXi9WYvFShfVepRdCoq1qvsp8fsR2XZtG8g");
}

pub mod disable_deploy_of_alloc_free_syscall {
    crate::declare_id!("79HWsX9rpnnJBPcdNURVqygpMAfxdrAirzAGAVmf92im");
}

pub mod include_account_index_in_rent_error {
    crate::declare_id!("2R72wpcQ7qV7aTJWUumdn8u5wmmTyXbK7qzEy7YSAgyY");
}

pub mod add_shred_type_to_shred_seed {
    crate::declare_id!("Ds87KVeqhbv7Jw8W6avsS1mqz3Mw5J3pRTpPoDQ2QdiJ");
}

pub mod warp_timestamp_with_a_vengeance {
    crate::declare_id!("3BX6SBeEBibHaVQXywdkcgyUk6evfYZkHdztXiDtEpFS");
}

pub mod separate_nonce_from_blockhash {
    crate::declare_id!("Gea3ZkK2N4pHuVZVxWcnAtS6UEDdyumdYt4pFcKjA3ar");
}

pub mod enable_durable_nonce {
    crate::declare_id!("4EJQtF2pkRyawwcTVfQutzq4Sa5hRhibF6QAK1QXhtEX");
}

pub mod vote_state_update_credit_per_dequeue {
    crate::declare_id!("CveezY6FDLVBToHDcvJRmtMouqzsmj4UXYh5ths5G5Uv");
}

pub mod quick_bail_on_panic {
    crate::declare_id!("DpJREPyuMZ5nDfU6H3WTqSqUFSXAfw8u7xqmWtEwJDcP");
}

pub mod nonce_must_be_authorized {
    crate::declare_id!("HxrEu1gXuH7iD3Puua1ohd5n4iUKJyFNtNxk9DVJkvgr");
}

pub mod nonce_must_be_advanceable {
    crate::declare_id!("3u3Er5Vc2jVcwz4xr2GJeSAXT3fAj6ADHZ4BJMZiScFd");
}

pub mod vote_authorize_with_seed {
    crate::declare_id!("6tRxEYKuy2L5nnv5bgn7iT28MxUbYxp5h7F3Ncf1exrT");
}

pub mod cap_accounts_data_size_per_block {
    crate::declare_id!("qywiJyZmqTKspFg2LeuUHqcA5nNvBgobqb9UprywS9N");
}

pub mod preserve_rent_epoch_for_rent_exempt_accounts {
    crate::declare_id!("HH3MUYReL2BvqqA3oEcAa7txju5GY6G4nxJ51zvsEjEZ");
}

pub mod enable_bpf_loader_extend_program_ix {
    crate::declare_id!("8Zs9W7D9MpSEtUWSQdGniZk2cNmV22y6FLJwCx53asme");
}

pub mod enable_early_verification_of_account_modifications {
    crate::declare_id!("7Vced912WrRnfjaiKRiNBcbuFw7RrnLv3E3z95Y4GTNc");
}

pub mod skip_rent_rewrites {
    crate::declare_id!("CGB2jM8pwZkeeiXQ66kBMyBR6Np61mggL7XUsmLjVcrw");
}

pub mod prevent_crediting_accounts_that_end_rent_paying {
    crate::declare_id!("812kqX67odAp5NFwM8D2N24cku7WTm9CHUTFUXaDkWPn");
}

pub mod cap_bpf_program_instruction_accounts {
    crate::declare_id!("9k5ijzTbYPtjzu8wj2ErH9v45xecHzQ1x4PMYMMxFgdM");
}

pub mod loosen_cpi_size_restriction {
    crate::declare_id!("GDH5TVdbTPUpRnXaRyQqiKUa7uZAbZ28Q2N9bhbKoMLm");
}

pub mod use_default_units_in_fee_calculation {
    crate::declare_id!("8sKQrMQoUHtQSUP83SPG4ta2JDjSAiWs7t5aJ9uEd6To");
}

pub mod compact_vote_state_updates {
    crate::declare_id!("86HpNqzutEZwLcPxS6EHDcMNYWk6ikhteg9un7Y2PBKE");
}

pub mod incremental_snapshot_only_incremental_hash_calculation {
    crate::declare_id!("25vqsfjk7Nv1prsQJmA4Xu1bN61s8LXCBGUPp8Rfy1UF");
}

pub mod disable_cpi_setting_executable_and_rent_epoch {
    crate::declare_id!("B9cdB55u4jQsDNsdTK525yE9dmSc5Ga7YBaBrDFvEhM9");
}

pub mod on_load_preserve_rent_epoch_for_rent_exempt_accounts {
    crate::declare_id!("CpkdQmspsaZZ8FVAouQTtTWZkc8eeQ7V3uj7dWz543rZ");
}

pub mod account_hash_ignore_slot {
    crate::declare_id!("SVn36yVApPLYsa8koK3qUcy14zXDnqkNYWyUh1f4oK1");
}

pub mod set_exempt_rent_epoch_max {
    crate::declare_id!("5wAGiy15X1Jb2hkHnPDCM8oB9V42VNA9ftNVFK84dEgv");
}

pub mod relax_authority_signer_check_for_lookup_table_creation {
    crate::declare_id!("FKAcEvNgSY79RpqsPNUV5gDyumopH4cEHqUxyfm8b8Ap");
}

pub mod stop_sibling_instruction_search_at_parent {
    crate::declare_id!("EYVpEP7uzH1CoXzbD6PubGhYmnxRXPeq3PPsm1ba3gpo");
}

pub mod vote_state_update_root_fix {
    crate::declare_id!("G74BkWBzmsByZ1kxHy44H3wjwp5hp7JbrGRuDpco22tY");
}

pub mod cap_accounts_data_allocations_per_transaction {
    crate::declare_id!("9gxu85LYRAcZL38We8MYJ4A9AwgBBPtVBAqebMcT1241");
}

pub mod epoch_accounts_hash {
    crate::declare_id!("5GpmAKxaGsWWbPp4bNXFLJxZVvG92ctxf7jQnzTQjF3n");
}

pub mod remove_deprecated_request_unit_ix {
    crate::declare_id!("EfhYd3SafzGT472tYQDUc4dPd2xdEfKs5fwkowUgVt4W");
}

pub mod disable_rehash_for_rent_epoch {
    crate::declare_id!("DTVTkmw3JSofd8CJVJte8PXEbxNQ2yZijvVr3pe2APPj");
}

pub mod increase_tx_account_lock_limit {
    crate::declare_id!("9LZdXeKGeBV6hRLdxS1rHbHoEUsKqesCC2ZAPTPKJAbK");
}

pub mod limit_max_instruction_trace_length {
    crate::declare_id!("GQALDaC48fEhZGWRj9iL5Q889emJKcj3aCvHF7VCbbF4");
}

pub mod check_syscall_outputs_do_not_overlap {
    crate::declare_id!("3uRVPBpyEJRo1emLCrq38eLRFGcu6uKSpUXqGvU8T7SZ");
}

pub mod enable_bpf_loader_set_authority_checked_ix {
    crate::declare_id!("5x3825XS7M2A3Ekbn5VGGkvFoAg5qrRWkTrY4bARP1GL");
}

pub mod enable_alt_bn128_syscall {
    crate::declare_id!("A16q37opZdQMCbe5qJ6xpBB9usykfv8jZaMkxvZQi4GJ");
}
pub mod enable_alt_bn128_compression_syscall {
    crate::declare_id!("EJJewYSddEEtSZHiqugnvhQHiWyZKjkFDQASd7oKSagn");
}

pub mod enable_program_redeployment_cooldown {
    crate::declare_id!("J4HFT8usBxpcF63y46t1upYobJgChmKyZPm5uTBRg25Z");
}

pub mod commission_updates_only_allowed_in_first_half_of_epoch {
    crate::declare_id!("noRuG2kzACwgaY7TVmLRnUNPLKNVQE1fb7X55YWBehp");
}

pub mod enable_turbine_fanout_experiments {
    crate::declare_id!("D31EFnLgdiysi84Woo3of4JMu7VmasUS3Z7j9HYXCeLY");
}

pub mod disable_turbine_fanout_experiments {
    crate::declare_id!("Gz1aLrbeQ4Q6PTSafCZcGWZXz91yVRi7ASFzFEr1U4sa");
}

pub mod move_serialized_len_ptr_in_cpi {
    crate::declare_id!("74CoWuBmt3rUVUrCb2JiSTvh6nXyBWUsK4SaMj3CtE3T");
}

pub mod update_hashes_per_tick {
    crate::declare_id!("3uFHb9oKdGfgZGJK9EHaAXN4USvnQtAFC13Fh5gGFS5B");
}

pub mod enable_big_mod_exp_syscall {
    crate::declare_id!("EBq48m8irRKuE7ZnMTLvLg2UuGSqhe8s8oMqnmja1fJw");
}

pub mod disable_builtin_loader_ownership_chains {
    crate::declare_id!("4UDcAfQ6EcA6bdcadkeHpkarkhZGJ7Bpq7wTAiRMjkoi");
}

pub mod cap_transaction_accounts_data_size {
    crate::declare_id!("DdLwVYuvDz26JohmgSbA7mjpJFgX5zP2dkp8qsF2C33V");
}

pub mod remove_congestion_multiplier_from_fee_calculation {
    crate::declare_id!("A8xyMHZovGXFkorFqEmVH2PKGLiBip5JD7jt4zsUWo4H");
}

pub mod enable_request_heap_frame_ix {
    crate::declare_id!("Hr1nUA9b7NJ6eChS26o7Vi8gYYDDwWD3YeBfzJkTbU86");
}

pub mod prevent_rent_paying_rent_recipients {
    crate::declare_id!("Fab5oP3DmsLYCiQZXdjyqT3ukFFPrsmqhXU4WU1AWVVF");
}

pub mod delay_visibility_of_program_deployment {
    crate::declare_id!("GmuBvtFb2aHfSfMXpuFeWZGHyDeCLPS79s48fmCWCfM5");
}

pub mod apply_cost_tracker_during_replay {
    crate::declare_id!("2ry7ygxiYURULZCrypHhveanvP5tzZ4toRwVp89oCNSj");
}
pub mod bpf_account_data_direct_mapping {
    crate::declare_id!("EenyoWx9UMXYKpR8mW5Jmfmy2fRjzUtM7NduYMY8bx33");
}

pub mod add_set_tx_loaded_accounts_data_size_instruction {
    crate::declare_id!("G6vbf1UBok8MWb8m25ex86aoQHeKTzDKzuZADHkShqm6");
}

pub mod switch_to_new_elf_parser {
    crate::declare_id!("Cdkc8PPTeTNUPoZEfCY5AyetUrEdkZtNPMgz58nqyaHD");
}

pub mod round_up_heap_size {
    crate::declare_id!("CE2et8pqgyQMP2mQRg3CgvX8nJBKUArMu3wfiQiQKY1y");
}

pub mod remove_bpf_loader_incorrect_program_id {
    crate::declare_id!("2HmTkCj9tXuPE4ueHzdD7jPeMf9JGCoZh5AsyoATiWEe");
}

pub mod include_loaded_accounts_data_size_in_fee_calculation {
    crate::declare_id!("EaQpmC6GtRssaZ3PCUM5YksGqUdMLeZ46BQXYtHYakDS");
}

pub mod native_programs_consume_cu {
    crate::declare_id!("8pgXCMNXC8qyEFypuwpXyRxLXZdpM4Qo72gJ6k87A6wL");
}

pub mod simplify_writable_program_account_check {
    crate::declare_id!("5ZCcFAzJ1zsFKe1KSZa9K92jhx7gkcKj97ci2DBo1vwj");
}

pub mod stop_truncating_strings_in_syscalls {
    crate::declare_id!("16FMCmgLzCNNz6eTwGanbyN2ZxvTBSLuQ6DZhgeMshg");
}

pub mod clean_up_delegation_errors {
    crate::declare_id!("Bj2jmUsM2iRhfdLLDSTkhM5UQRQvQHm57HSmPibPtEyu");
}

pub mod vote_state_add_vote_latency {
    crate::declare_id!("7axKe5BTYBDD87ftzWbk5DfzWMGyRvqmWTduuo22Yaqy");
}

pub mod checked_arithmetic_in_fee_validation {
    crate::declare_id!("5Pecy6ie6XGm22pc9d4P9W5c31BugcFBuy6hsP2zkETv");
}

pub mod last_restart_slot_sysvar {
    crate::declare_id!("HooKD5NC9QNxk25QuzCssB8ecrEzGt6eXEPBUxWp1LaR");
}

pub mod reduce_stake_warmup_cooldown {
    crate::declare_id!("GwtDQBghCTBgmX2cpEGNPxTEBUTQRaDMGTr5qychdGMj");
}

pub mod revise_turbine_epoch_stakes {
    crate::declare_id!("BTWmtJC8U5ZLMbBUUA1k6As62sYjPEjAiNAT55xYGdJU");
}

pub mod enable_poseidon_syscall {
    crate::declare_id!("FL9RsQA6TVUoh5xJQ9d936RHSebA1NLQqe3Zv9sXZRpr");
}

pub mod timely_vote_credits {
    crate::declare_id!("2oXpeh141pPZCTCFHBsvCwG2BtaHZZAtrVhwaxSy6brS");
}

pub mod remaining_compute_units_syscall_enabled {
    crate::declare_id!("5TuppMutoyzhUSfuYdhgzD47F92GL1g89KpCZQKqedxP");
}

pub mod enable_program_runtime_v2_and_loader_v4 {
    crate::declare_id!("8oBxsYqnCvUTGzgEpxPcnVf7MLbWWPYddE33PftFeBBd");
}

pub mod require_rent_exempt_split_destination {
    crate::declare_id!("D2aip4BBr8NPWtU9vLrwrBvbuaQ8w1zV38zFLxx4pfBV");
}

pub mod better_error_codes_for_tx_lamport_check {
    crate::declare_id!("Ffswd3egL3tccB6Rv3XY6oqfdzn913vUcjCSnpvCKpfx");
}

pub mod update_hashes_per_tick2 {
    crate::declare_id!("EWme9uFqfy1ikK1jhJs8fM5hxWnK336QJpbscNtizkTU");
}

pub mod update_hashes_per_tick3 {
    crate::declare_id!("8C8MCtsab5SsfammbzvYz65HHauuUYdbY2DZ4sznH6h5");
}

pub mod update_hashes_per_tick4 {
    crate::declare_id!("8We4E7DPwF2WfAN8tRTtWQNhi98B99Qpuj7JoZ3Aikgg");
}

pub mod update_hashes_per_tick5 {
    crate::declare_id!("BsKLKAn1WM4HVhPRDsjosmqSg2J8Tq5xP2s2daDS6Ni4");
}

pub mod update_hashes_per_tick6 {
    crate::declare_id!("FKu1qYwLQSiehz644H6Si65U5ZQ2cp9GxsyFUfYcuADv");
}

pub mod validate_fee_collector_account {
    crate::declare_id!("prpFrMtgNmzaNzkPJg9o753fVvbHKqNrNTm76foJ2wm");
}

pub mod enable_zk_transfer_with_fee {
    crate::declare_id!("zkNLP7EQALfC1TYeB3biDU7akDckj8iPkvh9y2Mt2K3");
}

pub mod drop_legacy_shreds {
    solana_sdk::declare_id!("GV49KKQdBNaiv2pgqhS2Dy3GWYJGXMTVYbYkdk91orRy");
}

pub mod allow_commission_decrease_at_any_time {
    solana_sdk::declare_id!("decoMktMcnmiq6t3u7g5BfgcQu91nKZr6RvMYf9z1Jb");
}

pub mod consume_blockstore_duplicate_proofs {
    solana_sdk::declare_id!("6YsBCejwK96GZCkJ6mkZ4b68oP63z2PLoQmWjC7ggTqZ");
}

pub mod index_erasure_conflict_duplicate_proofs {
    solana_sdk::declare_id!("dupPajaLy2SSn8ko42aZz4mHANDNrLe8Nw8VQgFecLa");
}

pub mod is_feature_active_syscall_enabled {
    crate::declare_id!("DdJ7zTYxodTonmHJkQiTDbbETdzNceKyjEbLN36HDfUm");
}
