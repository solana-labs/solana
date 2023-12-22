//! Collection of all runtime features.
//!
//! Steps to add a new feature are outlined below. Note that these steps only cover
//! the process of getting a feature into the core Solana code.
//! - For features that are unambiguously good (ie bug fixes), these steps are sufficient.
//! - For features that should go up for community vote (ie fee structure changes), more
//!   information on the additional steps to follow can be found at:
//!   <https://spl.solana.com/feature-proposal#feature-proposal-life-cycle>
//!
//! 1. Generate a new keypair with `solana-keygen new --outfile feature.json --no-passphrase`
//!    - Keypairs should be held by core contributors only. If you're a non-core contributor going
//!      through these steps, the PR process will facilitate a keypair holder being picked. That
//!      person will generate the keypair, provide pubkey for PR, and ultimately enable the feature.
//! 2. Add a public module for the feature, specifying keypair pubkey as the id with
//!    `solana_sdk::declare_id!()` within the module.
//!    Additionally, add an entry to `FEATURE_NAMES` map.
//! 3. Add desired logic to check for and switch on feature availability.
//!
//! For more information on how features are picked up, see comments for `Feature`.

use {
    lazy_static::lazy_static,
    solana_program::{epoch_schedule::EpochSchedule, stake_history::Epoch},
    solana_sdk::{
        clock::Slot,
        hash::{Hash, Hasher},
        pubkey::Pubkey,
    },
    std::collections::{HashMap, HashSet},
};

pub mod deprecate_rewards_sysvar {
    solana_sdk::declare_id!("GaBtBJvmS4Arjj5W1NmFcyvPjsHN38UGYDq2MDwbs9Qu");
}

pub mod pico_inflation {
    solana_sdk::declare_id!("4RWNif6C2WCNiKVW7otP4G7dkmkHGyKQWRpuZ1pxKU5m");
}

pub mod full_inflation {
    pub mod devnet_and_testnet {
        solana_sdk::declare_id!("DT4n6ABDqs6w4bnfwrXT9rsprcPf6cdDga1egctaPkLC");
    }

    pub mod mainnet {
        pub mod certusone {
            pub mod vote {
                solana_sdk::declare_id!("BzBBveUDymEYoYzcMWNQCx3cd4jQs7puaVFHLtsbB6fm");
            }
            pub mod enable {
                solana_sdk::declare_id!("7XRJcS5Ud5vxGB54JbK9N2vBZVwnwdBNeJW1ibRgD9gx");
            }
        }
    }
}

pub mod secp256k1_program_enabled {
    solana_sdk::declare_id!("E3PHP7w8kB7np3CTQ1qQ2tW3KCtjRSXBQgW9vM2mWv2Y");
}

pub mod spl_token_v2_multisig_fix {
    solana_sdk::declare_id!("E5JiFDQCwyC6QfT9REFyMpfK2mHcmv1GUDySU1Ue7TYv");
}

pub mod no_overflow_rent_distribution {
    solana_sdk::declare_id!("4kpdyrcj5jS47CZb2oJGfVxjYbsMm2Kx97gFyZrxxwXz");
}

pub mod filter_stake_delegation_accounts {
    solana_sdk::declare_id!("GE7fRxmW46K6EmCD9AMZSbnaJ2e3LfqCZzdHi9hmYAgi");
}

pub mod require_custodian_for_locked_stake_authorize {
    solana_sdk::declare_id!("D4jsDcXaqdW8tDAWn8H4R25Cdns2YwLneujSL1zvjW6R");
}

pub mod spl_token_v2_self_transfer_fix {
    solana_sdk::declare_id!("BL99GYhdjjcv6ys22C9wPgn2aTVERDbPHHo4NbS3hgp7");
}

pub mod warp_timestamp_again {
    solana_sdk::declare_id!("GvDsGDkH5gyzwpDhxNixx8vtx1kwYHH13RiNAPw27zXb");
}

pub mod check_init_vote_data {
    solana_sdk::declare_id!("3ccR6QpxGYsAbWyfevEtBNGfWV4xBffxRj2tD6A9i39F");
}

pub mod secp256k1_recover_syscall_enabled {
    solana_sdk::declare_id!("6RvdSWHh8oh72Dp7wMTS2DBkf3fRPtChfNrAo3cZZoXJ");
}

pub mod system_transfer_zero_check {
    solana_sdk::declare_id!("BrTR9hzw4WBGFP65AJMbpAo64DcA3U6jdPSga9fMV5cS");
}

pub mod blake3_syscall_enabled {
    solana_sdk::declare_id!("HTW2pSyErTj4BV6KBM9NZ9VBUJVxt7sacNWcf76wtzb3");
}

pub mod dedupe_config_program_signers {
    solana_sdk::declare_id!("8kEuAshXLsgkUEdcFVLqrjCGGHVWFW99ZZpxvAzzMtBp");
}

pub mod verify_tx_signatures_len {
    solana_sdk::declare_id!("EVW9B5xD9FFK7vw1SBARwMA4s5eRo5eKJdKpsBikzKBz");
}

pub mod vote_stake_checked_instructions {
    solana_sdk::declare_id!("BcWknVcgvonN8sL4HE4XFuEVgfcee5MwxWPAgP6ZV89X");
}

pub mod rent_for_sysvars {
    solana_sdk::declare_id!("BKCPBQQBZqggVnFso5nQ8rQ4RwwogYwjuUt9biBjxwNF");
}

pub mod libsecp256k1_0_5_upgrade_enabled {
    solana_sdk::declare_id!("DhsYfRjxfnh2g7HKJYSzT79r74Afa1wbHkAgHndrA1oy");
}

pub mod tx_wide_compute_cap {
    solana_sdk::declare_id!("5ekBxc8itEnPv4NzGJtr8BVVQLNMQuLMNQQj7pHoLNZ9");
}

pub mod spl_token_v2_set_authority_fix {
    solana_sdk::declare_id!("FToKNBYyiF4ky9s8WsmLBXHCht17Ek7RXaLZGHzzQhJ1");
}

pub mod merge_nonce_error_into_system_error {
    solana_sdk::declare_id!("21AWDosvp3pBamFW91KB35pNoaoZVTM7ess8nr2nt53B");
}

pub mod disable_fees_sysvar {
    solana_sdk::declare_id!("JAN1trEUEtZjgXYzNBYHU9DYd7GnThhXfFP7SzPXkPsG");
}

pub mod stake_merge_with_unmatched_credits_observed {
    solana_sdk::declare_id!("meRgp4ArRPhD3KtCY9c5yAf2med7mBLsjKTPeVUHqBL");
}

pub mod zk_token_sdk_enabled {
    solana_sdk::declare_id!("zk1snxsc6Fh3wsGNbbHAJNHiJoYgF29mMnTSusGx5EJ");
}

pub mod curve25519_syscall_enabled {
    solana_sdk::declare_id!("7rcw5UtqgDTBBv2EcynNfYckgdAaH1MAsCjKgXMkN7Ri");
}

pub mod versioned_tx_message_enabled {
    solana_sdk::declare_id!("3KZZ6Ks1885aGBQ45fwRcPXVBCtzUvxhUTkwKMR41Tca");
}

pub mod libsecp256k1_fail_on_bad_count {
    solana_sdk::declare_id!("8aXvSuopd1PUj7UhehfXJRg6619RHp8ZvwTyyJHdUYsj");
}

pub mod libsecp256k1_fail_on_bad_count2 {
    solana_sdk::declare_id!("54KAoNiUERNoWWUhTWWwXgym94gzoXFVnHyQwPA18V9A");
}

pub mod instructions_sysvar_owned_by_sysvar {
    solana_sdk::declare_id!("H3kBSaKdeiUsyHmeHqjJYNc27jesXZ6zWj3zWkowQbkV");
}

pub mod stake_program_advance_activating_credits_observed {
    solana_sdk::declare_id!("SAdVFw3RZvzbo6DvySbSdBnHN4gkzSTH9dSxesyKKPj");
}

pub mod credits_auto_rewind {
    solana_sdk::declare_id!("BUS12ciZ5gCoFafUHWW8qaFMMtwFQGVxjsDheWLdqBE2");
}

pub mod demote_program_write_locks {
    solana_sdk::declare_id!("3E3jV7v9VcdJL8iYZUMax9DiDno8j7EWUVbhm9RtShj2");
}

pub mod ed25519_program_enabled {
    solana_sdk::declare_id!("6ppMXNYLhVd7GcsZ5uV11wQEW7spppiMVfqQv5SXhDpX");
}

pub mod return_data_syscall_enabled {
    solana_sdk::declare_id!("DwScAzPUjuv65TMbDnFY7AgwmotzWy3xpEJMXM3hZFaB");
}

pub mod reduce_required_deploy_balance {
    solana_sdk::declare_id!("EBeznQDjcPG8491sFsKZYBi5S5jTVXMpAKNDJMQPS2kq");
}

pub mod sol_log_data_syscall_enabled {
    solana_sdk::declare_id!("6uaHcKPGUy4J7emLBgUTeufhJdiwhngW6a1R9B7c2ob9");
}

pub mod stakes_remove_delegation_if_inactive {
    solana_sdk::declare_id!("HFpdDDNQjvcXnXKec697HDDsyk6tFoWS2o8fkxuhQZpL");
}

pub mod do_support_realloc {
    solana_sdk::declare_id!("75m6ysz33AfLA5DDEzWM1obBrnPQRSsdVQ2nRmc8Vuu1");
}

pub mod prevent_calling_precompiles_as_programs {
    solana_sdk::declare_id!("4ApgRX3ud6p7LNMJmsuaAcZY5HWctGPr5obAsjB3A54d");
}

pub mod optimize_epoch_boundary_updates {
    solana_sdk::declare_id!("265hPS8k8xJ37ot82KEgjRunsUp5w4n4Q4VwwiN9i9ps");
}

pub mod remove_native_loader {
    solana_sdk::declare_id!("HTTgmruMYRZEntyL3EdCDdnS6e4D5wRq1FA7kQsb66qq");
}

pub mod send_to_tpu_vote_port {
    solana_sdk::declare_id!("C5fh68nJ7uyKAuYZg2x9sEQ5YrVf3dkW6oojNBSc3Jvo");
}

pub mod requestable_heap_size {
    solana_sdk::declare_id!("CCu4boMmfLuqcmfTLPHQiUo22ZdUsXjgzPAURYaWt1Bw");
}

pub mod disable_fee_calculator {
    solana_sdk::declare_id!("2jXx2yDmGysmBKfKYNgLj2DQyAQv6mMk2BPh4eSbyB4H");
}

pub mod add_compute_budget_program {
    solana_sdk::declare_id!("4d5AKtxoh93Dwm1vHXUU3iRATuMndx1c431KgT2td52r");
}

pub mod nonce_must_be_writable {
    solana_sdk::declare_id!("BiCU7M5w8ZCMykVSyhZ7Q3m2SWoR2qrEQ86ERcDX77ME");
}

pub mod spl_token_v3_3_0_release {
    solana_sdk::declare_id!("Ftok2jhqAqxUWEiCVRrfRs9DPppWP8cgTB7NQNKL88mS");
}

pub mod leave_nonce_on_success {
    solana_sdk::declare_id!("E8MkiWZNNPGU6n55jkGzyj8ghUmjCHRmDFdYYFYHxWhQ");
}

pub mod reject_empty_instruction_without_program {
    solana_sdk::declare_id!("9kdtFSrXHQg3hKkbXkQ6trJ3Ja1xpJ22CTFSNAciEwmL");
}

pub mod fixed_memcpy_nonoverlapping_check {
    solana_sdk::declare_id!("36PRUK2Dz6HWYdG9SpjeAsF5F3KxnFCakA2BZMbtMhSb");
}

pub mod reject_non_rent_exempt_vote_withdraws {
    solana_sdk::declare_id!("7txXZZD6Um59YoLMF7XUNimbMjsqsWhc7g2EniiTrmp1");
}

pub mod evict_invalid_stakes_cache_entries {
    solana_sdk::declare_id!("EMX9Q7TVFAmQ9V1CggAkhMzhXSg8ECp7fHrWQX2G1chf");
}

pub mod allow_votes_to_directly_update_vote_state {
    solana_sdk::declare_id!("Ff8b1fBeB86q8cjq47ZhsQLgv5EkHu3G1C99zjUfAzrq");
}

pub mod cap_accounts_data_len {
    solana_sdk::declare_id!("capRxUrBjNkkCpjrJxPGfPaWijB7q3JoDfsWXAnt46r");
}

pub mod max_tx_account_locks {
    solana_sdk::declare_id!("CBkDroRDqm8HwHe6ak9cguPjUomrASEkfmxEaZ5CNNxz");
}

pub mod require_rent_exempt_accounts {
    solana_sdk::declare_id!("BkFDxiJQWZXGTZaJQxH7wVEHkAmwCgSEVkrvswFfRJPD");
}

pub mod filter_votes_outside_slot_hashes {
    solana_sdk::declare_id!("3gtZPqvPpsbXZVCx6hceMfWxtsmrjMzmg8C7PLKSxS2d");
}

pub mod update_syscall_base_costs {
    solana_sdk::declare_id!("2h63t332mGCCsWK2nqqqHhN4U9ayyqhLVFvczznHDoTZ");
}

pub mod stake_deactivate_delinquent_instruction {
    solana_sdk::declare_id!("437r62HoAdUb63amq3D7ENnBLDhHT2xY8eFkLJYVKK4x");
}

pub mod stake_redelegate_instruction {
    solana_sdk::declare_id!("2KKG3C6RBnxQo9jVVrbzsoSh41TDXLK7gBc9gduyxSzW");
}

pub mod vote_withdraw_authority_may_change_authorized_voter {
    solana_sdk::declare_id!("AVZS3ZsN4gi6Rkx2QUibYuSJG3S6QHib7xCYhG6vGJxU");
}

pub mod spl_associated_token_account_v1_0_4 {
    solana_sdk::declare_id!("FaTa4SpiaSNH44PGC4z8bnGVTkSRYaWvrBs3KTu8XQQq");
}

pub mod reject_vote_account_close_unless_zero_credit_epoch {
    solana_sdk::declare_id!("ALBk3EWdeAg2WAGf6GPDUf1nynyNqCdEVmgouG7rpuCj");
}

pub mod add_get_processed_sibling_instruction_syscall {
    solana_sdk::declare_id!("CFK1hRCNy8JJuAAY8Pb2GjLFNdCThS2qwZNe3izzBMgn");
}

pub mod bank_transaction_count_fix {
    solana_sdk::declare_id!("Vo5siZ442SaZBKPXNocthiXysNviW4UYPwRFggmbgAp");
}

pub mod disable_bpf_deprecated_load_instructions {
    solana_sdk::declare_id!("3XgNukcZWf9o3HdA3fpJbm94XFc4qpvTXc8h1wxYwiPi");
}

pub mod disable_bpf_unresolved_symbols_at_runtime {
    solana_sdk::declare_id!("4yuaYAj2jGMGTh1sSmi4G2eFscsDq8qjugJXZoBN6YEa");
}

pub mod record_instruction_in_transaction_context_push {
    solana_sdk::declare_id!("3aJdcZqxoLpSBxgeYGjPwaYS1zzcByxUDqJkbzWAH1Zb");
}

pub mod syscall_saturated_math {
    solana_sdk::declare_id!("HyrbKftCdJ5CrUfEti6x26Cj7rZLNe32weugk7tLcWb8");
}

pub mod check_physical_overlapping {
    solana_sdk::declare_id!("nWBqjr3gpETbiaVj3CBJ3HFC5TMdnJDGt21hnvSTvVZ");
}

pub mod limit_secp256k1_recovery_id {
    solana_sdk::declare_id!("7g9EUwj4j7CS21Yx1wvgWLjSZeh5aPq8x9kpoPwXM8n8");
}

pub mod disable_deprecated_loader {
    solana_sdk::declare_id!("GTUMCZ8LTNxVfxdrw7ZsDFTxXb7TutYkzJnFwinpE6dg");
}

pub mod check_slice_translation_size {
    solana_sdk::declare_id!("GmC19j9qLn2RFk5NduX6QXaDhVpGncVVBzyM8e9WMz2F");
}

pub mod stake_split_uses_rent_sysvar {
    solana_sdk::declare_id!("FQnc7U4koHqWgRvFaBJjZnV8VPg6L6wWK33yJeDp4yvV");
}

pub mod add_get_minimum_delegation_instruction_to_stake_program {
    solana_sdk::declare_id!("St8k9dVXP97xT6faW24YmRSYConLbhsMJA4TJTBLmMT");
}

pub mod error_on_syscall_bpf_function_hash_collisions {
    solana_sdk::declare_id!("8199Q2gMD2kwgfopK5qqVWuDbegLgpuFUFHCcUJQDN8b");
}

pub mod reject_callx_r10 {
    solana_sdk::declare_id!("3NKRSwpySNwD3TvP5pHnRmkAQRsdkXWRr1WaQh8p4PWX");
}

pub mod drop_redundant_turbine_path {
    solana_sdk::declare_id!("4Di3y24QFLt5QEUPZtbnjyfQKfm6ZMTfa6Dw1psfoMKU");
}

pub mod executables_incur_cpi_data_cost {
    solana_sdk::declare_id!("7GUcYgq4tVtaqNCKT3dho9r4665Qp5TxCZ27Qgjx3829");
}

pub mod fix_recent_blockhashes {
    solana_sdk::declare_id!("6iyggb5MTcsvdcugX7bEKbHV8c6jdLbpHwkncrgLMhfo");
}

pub mod update_rewards_from_cached_accounts {
    solana_sdk::declare_id!("28s7i3htzhahXQKqmS2ExzbEoUypg9krwvtK2M9UWXh9");
}
pub mod enable_partitioned_epoch_reward {
    solana_sdk::declare_id!("41tVp5qR1XwWRt5WifvtSQyuxtqQWJgEK8w91AtBqSwP");
}

pub mod spl_token_v3_4_0 {
    solana_sdk::declare_id!("Ftok4njE8b7tDffYkC5bAbCaQv5sL6jispYrprzatUwN");
}

pub mod spl_associated_token_account_v1_1_0 {
    solana_sdk::declare_id!("FaTa17gVKoqbh38HcfiQonPsAaQViyDCCSg71AubYZw8");
}

pub mod default_units_per_instruction {
    solana_sdk::declare_id!("J2QdYx8crLbTVK8nur1jeLsmc3krDbfjoxoea2V1Uy5Q");
}

pub mod stake_allow_zero_undelegated_amount {
    solana_sdk::declare_id!("sTKz343FM8mqtyGvYWvbLpTThw3ixRM4Xk8QvZ985mw");
}

pub mod require_static_program_ids_in_transaction {
    solana_sdk::declare_id!("8FdwgyHFEjhAdjWfV2vfqk7wA1g9X3fQpKH7SBpEv3kC");
}

pub mod stake_raise_minimum_delegation_to_1_sol {
    // This is a feature-proposal *feature id*.  The feature keypair address is `GQXzC7YiSNkje6FFUk6sc2p53XRvKoaZ9VMktYzUMnpL`.
    solana_sdk::declare_id!("9onWzzvCzNC2jfhxxeqRgs5q7nFAAKpCUvkj6T6GJK9i");
}

pub mod stake_minimum_delegation_for_rewards {
    solana_sdk::declare_id!("G6ANXD6ptCSyNd9znZm7j4dEczAJCfx7Cy43oBx3rKHJ");
}

pub mod add_set_compute_unit_price_ix {
    solana_sdk::declare_id!("98std1NSHqXi9WYvFShfVepRdCoq1qvsp8fsR2XZtG8g");
}

pub mod disable_deploy_of_alloc_free_syscall {
    solana_sdk::declare_id!("79HWsX9rpnnJBPcdNURVqygpMAfxdrAirzAGAVmf92im");
}

pub mod include_account_index_in_rent_error {
    solana_sdk::declare_id!("2R72wpcQ7qV7aTJWUumdn8u5wmmTyXbK7qzEy7YSAgyY");
}

pub mod add_shred_type_to_shred_seed {
    solana_sdk::declare_id!("Ds87KVeqhbv7Jw8W6avsS1mqz3Mw5J3pRTpPoDQ2QdiJ");
}

pub mod warp_timestamp_with_a_vengeance {
    solana_sdk::declare_id!("3BX6SBeEBibHaVQXywdkcgyUk6evfYZkHdztXiDtEpFS");
}

pub mod separate_nonce_from_blockhash {
    solana_sdk::declare_id!("Gea3ZkK2N4pHuVZVxWcnAtS6UEDdyumdYt4pFcKjA3ar");
}

pub mod enable_durable_nonce {
    solana_sdk::declare_id!("4EJQtF2pkRyawwcTVfQutzq4Sa5hRhibF6QAK1QXhtEX");
}

pub mod vote_state_update_credit_per_dequeue {
    solana_sdk::declare_id!("CveezY6FDLVBToHDcvJRmtMouqzsmj4UXYh5ths5G5Uv");
}

pub mod quick_bail_on_panic {
    solana_sdk::declare_id!("DpJREPyuMZ5nDfU6H3WTqSqUFSXAfw8u7xqmWtEwJDcP");
}

pub mod nonce_must_be_authorized {
    solana_sdk::declare_id!("HxrEu1gXuH7iD3Puua1ohd5n4iUKJyFNtNxk9DVJkvgr");
}

pub mod nonce_must_be_advanceable {
    solana_sdk::declare_id!("3u3Er5Vc2jVcwz4xr2GJeSAXT3fAj6ADHZ4BJMZiScFd");
}

pub mod vote_authorize_with_seed {
    solana_sdk::declare_id!("6tRxEYKuy2L5nnv5bgn7iT28MxUbYxp5h7F3Ncf1exrT");
}

pub mod cap_accounts_data_size_per_block {
    solana_sdk::declare_id!("qywiJyZmqTKspFg2LeuUHqcA5nNvBgobqb9UprywS9N");
}

pub mod preserve_rent_epoch_for_rent_exempt_accounts {
    solana_sdk::declare_id!("HH3MUYReL2BvqqA3oEcAa7txju5GY6G4nxJ51zvsEjEZ");
}

pub mod enable_bpf_loader_extend_program_ix {
    solana_sdk::declare_id!("8Zs9W7D9MpSEtUWSQdGniZk2cNmV22y6FLJwCx53asme");
}

pub mod enable_early_verification_of_account_modifications {
    solana_sdk::declare_id!("7Vced912WrRnfjaiKRiNBcbuFw7RrnLv3E3z95Y4GTNc");
}

pub mod skip_rent_rewrites {
    solana_sdk::declare_id!("CGB2jM8pwZkeeiXQ66kBMyBR6Np61mggL7XUsmLjVcrw");
}

pub mod prevent_crediting_accounts_that_end_rent_paying {
    solana_sdk::declare_id!("812kqX67odAp5NFwM8D2N24cku7WTm9CHUTFUXaDkWPn");
}

pub mod cap_bpf_program_instruction_accounts {
    solana_sdk::declare_id!("9k5ijzTbYPtjzu8wj2ErH9v45xecHzQ1x4PMYMMxFgdM");
}

pub mod loosen_cpi_size_restriction {
    solana_sdk::declare_id!("GDH5TVdbTPUpRnXaRyQqiKUa7uZAbZ28Q2N9bhbKoMLm");
}

pub mod use_default_units_in_fee_calculation {
    solana_sdk::declare_id!("8sKQrMQoUHtQSUP83SPG4ta2JDjSAiWs7t5aJ9uEd6To");
}

pub mod compact_vote_state_updates {
    solana_sdk::declare_id!("86HpNqzutEZwLcPxS6EHDcMNYWk6ikhteg9un7Y2PBKE");
}

pub mod incremental_snapshot_only_incremental_hash_calculation {
    solana_sdk::declare_id!("25vqsfjk7Nv1prsQJmA4Xu1bN61s8LXCBGUPp8Rfy1UF");
}

pub mod disable_cpi_setting_executable_and_rent_epoch {
    solana_sdk::declare_id!("B9cdB55u4jQsDNsdTK525yE9dmSc5Ga7YBaBrDFvEhM9");
}

pub mod on_load_preserve_rent_epoch_for_rent_exempt_accounts {
    solana_sdk::declare_id!("CpkdQmspsaZZ8FVAouQTtTWZkc8eeQ7V3uj7dWz543rZ");
}

pub mod account_hash_ignore_slot {
    solana_sdk::declare_id!("SVn36yVApPLYsa8koK3qUcy14zXDnqkNYWyUh1f4oK1");
}

pub mod set_exempt_rent_epoch_max {
    solana_sdk::declare_id!("5wAGiy15X1Jb2hkHnPDCM8oB9V42VNA9ftNVFK84dEgv");
}

pub mod relax_authority_signer_check_for_lookup_table_creation {
    solana_sdk::declare_id!("FKAcEvNgSY79RpqsPNUV5gDyumopH4cEHqUxyfm8b8Ap");
}

pub mod stop_sibling_instruction_search_at_parent {
    solana_sdk::declare_id!("EYVpEP7uzH1CoXzbD6PubGhYmnxRXPeq3PPsm1ba3gpo");
}

pub mod vote_state_update_root_fix {
    solana_sdk::declare_id!("G74BkWBzmsByZ1kxHy44H3wjwp5hp7JbrGRuDpco22tY");
}

pub mod cap_accounts_data_allocations_per_transaction {
    solana_sdk::declare_id!("9gxu85LYRAcZL38We8MYJ4A9AwgBBPtVBAqebMcT1241");
}

pub mod epoch_accounts_hash {
    solana_sdk::declare_id!("5GpmAKxaGsWWbPp4bNXFLJxZVvG92ctxf7jQnzTQjF3n");
}

pub mod remove_deprecated_request_unit_ix {
    solana_sdk::declare_id!("EfhYd3SafzGT472tYQDUc4dPd2xdEfKs5fwkowUgVt4W");
}

pub mod disable_rehash_for_rent_epoch {
    solana_sdk::declare_id!("DTVTkmw3JSofd8CJVJte8PXEbxNQ2yZijvVr3pe2APPj");
}

pub mod increase_tx_account_lock_limit {
    solana_sdk::declare_id!("9LZdXeKGeBV6hRLdxS1rHbHoEUsKqesCC2ZAPTPKJAbK");
}

pub mod limit_max_instruction_trace_length {
    solana_sdk::declare_id!("GQALDaC48fEhZGWRj9iL5Q889emJKcj3aCvHF7VCbbF4");
}

pub mod check_syscall_outputs_do_not_overlap {
    solana_sdk::declare_id!("3uRVPBpyEJRo1emLCrq38eLRFGcu6uKSpUXqGvU8T7SZ");
}

pub mod enable_bpf_loader_set_authority_checked_ix {
    solana_sdk::declare_id!("5x3825XS7M2A3Ekbn5VGGkvFoAg5qrRWkTrY4bARP1GL");
}

pub mod enable_alt_bn128_syscall {
    solana_sdk::declare_id!("A16q37opZdQMCbe5qJ6xpBB9usykfv8jZaMkxvZQi4GJ");
}
pub mod enable_alt_bn128_compression_syscall {
    solana_sdk::declare_id!("EJJewYSddEEtSZHiqugnvhQHiWyZKjkFDQASd7oKSagn");
}

pub mod enable_program_redeployment_cooldown {
    solana_sdk::declare_id!("J4HFT8usBxpcF63y46t1upYobJgChmKyZPm5uTBRg25Z");
}

pub mod commission_updates_only_allowed_in_first_half_of_epoch {
    solana_sdk::declare_id!("noRuG2kzACwgaY7TVmLRnUNPLKNVQE1fb7X55YWBehp");
}

pub mod enable_turbine_fanout_experiments {
    solana_sdk::declare_id!("D31EFnLgdiysi84Woo3of4JMu7VmasUS3Z7j9HYXCeLY");
}

pub mod disable_turbine_fanout_experiments {
    solana_sdk::declare_id!("Gz1aLrbeQ4Q6PTSafCZcGWZXz91yVRi7ASFzFEr1U4sa");
}

pub mod move_serialized_len_ptr_in_cpi {
    solana_sdk::declare_id!("74CoWuBmt3rUVUrCb2JiSTvh6nXyBWUsK4SaMj3CtE3T");
}

pub mod update_hashes_per_tick {
    solana_sdk::declare_id!("3uFHb9oKdGfgZGJK9EHaAXN4USvnQtAFC13Fh5gGFS5B");
}

pub mod enable_big_mod_exp_syscall {
    solana_sdk::declare_id!("EBq48m8irRKuE7ZnMTLvLg2UuGSqhe8s8oMqnmja1fJw");
}

pub mod disable_builtin_loader_ownership_chains {
    solana_sdk::declare_id!("4UDcAfQ6EcA6bdcadkeHpkarkhZGJ7Bpq7wTAiRMjkoi");
}

pub mod cap_transaction_accounts_data_size {
    solana_sdk::declare_id!("DdLwVYuvDz26JohmgSbA7mjpJFgX5zP2dkp8qsF2C33V");
}

pub mod remove_congestion_multiplier_from_fee_calculation {
    solana_sdk::declare_id!("A8xyMHZovGXFkorFqEmVH2PKGLiBip5JD7jt4zsUWo4H");
}

pub mod enable_request_heap_frame_ix {
    solana_sdk::declare_id!("Hr1nUA9b7NJ6eChS26o7Vi8gYYDDwWD3YeBfzJkTbU86");
}

pub mod prevent_rent_paying_rent_recipients {
    solana_sdk::declare_id!("Fab5oP3DmsLYCiQZXdjyqT3ukFFPrsmqhXU4WU1AWVVF");
}

pub mod delay_visibility_of_program_deployment {
    solana_sdk::declare_id!("GmuBvtFb2aHfSfMXpuFeWZGHyDeCLPS79s48fmCWCfM5");
}

pub mod apply_cost_tracker_during_replay {
    solana_sdk::declare_id!("2ry7ygxiYURULZCrypHhveanvP5tzZ4toRwVp89oCNSj");
}
pub mod bpf_account_data_direct_mapping {
    solana_sdk::declare_id!("EenyoWx9UMXYKpR8mW5Jmfmy2fRjzUtM7NduYMY8bx33");
}

pub mod add_set_tx_loaded_accounts_data_size_instruction {
    solana_sdk::declare_id!("G6vbf1UBok8MWb8m25ex86aoQHeKTzDKzuZADHkShqm6");
}

pub mod switch_to_new_elf_parser {
    solana_sdk::declare_id!("Cdkc8PPTeTNUPoZEfCY5AyetUrEdkZtNPMgz58nqyaHD");
}

pub mod round_up_heap_size {
    solana_sdk::declare_id!("CE2et8pqgyQMP2mQRg3CgvX8nJBKUArMu3wfiQiQKY1y");
}

pub mod remove_bpf_loader_incorrect_program_id {
    solana_sdk::declare_id!("2HmTkCj9tXuPE4ueHzdD7jPeMf9JGCoZh5AsyoATiWEe");
}

pub mod include_loaded_accounts_data_size_in_fee_calculation {
    solana_sdk::declare_id!("EaQpmC6GtRssaZ3PCUM5YksGqUdMLeZ46BQXYtHYakDS");
}

pub mod native_programs_consume_cu {
    solana_sdk::declare_id!("8pgXCMNXC8qyEFypuwpXyRxLXZdpM4Qo72gJ6k87A6wL");
}

pub mod simplify_writable_program_account_check {
    solana_sdk::declare_id!("5ZCcFAzJ1zsFKe1KSZa9K92jhx7gkcKj97ci2DBo1vwj");
}

pub mod stop_truncating_strings_in_syscalls {
    solana_sdk::declare_id!("16FMCmgLzCNNz6eTwGanbyN2ZxvTBSLuQ6DZhgeMshg");
}

pub mod clean_up_delegation_errors {
    solana_sdk::declare_id!("Bj2jmUsM2iRhfdLLDSTkhM5UQRQvQHm57HSmPibPtEyu");
}

pub mod vote_state_add_vote_latency {
    solana_sdk::declare_id!("7axKe5BTYBDD87ftzWbk5DfzWMGyRvqmWTduuo22Yaqy");
}

pub mod checked_arithmetic_in_fee_validation {
    solana_sdk::declare_id!("5Pecy6ie6XGm22pc9d4P9W5c31BugcFBuy6hsP2zkETv");
}

pub mod last_restart_slot_sysvar {
    solana_sdk::declare_id!("HooKD5NC9QNxk25QuzCssB8ecrEzGt6eXEPBUxWp1LaR");
}

pub mod reduce_stake_warmup_cooldown {
    solana_sdk::declare_id!("GwtDQBghCTBgmX2cpEGNPxTEBUTQRaDMGTr5qychdGMj");
}

pub mod revise_turbine_epoch_stakes {
    solana_sdk::declare_id!("BTWmtJC8U5ZLMbBUUA1k6As62sYjPEjAiNAT55xYGdJU");
}

pub mod enable_poseidon_syscall {
    solana_sdk::declare_id!("FL9RsQA6TVUoh5xJQ9d936RHSebA1NLQqe3Zv9sXZRpr");
}

pub mod timely_vote_credits {
    solana_sdk::declare_id!("2oXpeh141pPZCTCFHBsvCwG2BtaHZZAtrVhwaxSy6brS");
}

pub mod remaining_compute_units_syscall_enabled {
    solana_sdk::declare_id!("5TuppMutoyzhUSfuYdhgzD47F92GL1g89KpCZQKqedxP");
}

pub mod enable_program_runtime_v2_and_loader_v4 {
    solana_sdk::declare_id!("8oBxsYqnCvUTGzgEpxPcnVf7MLbWWPYddE33PftFeBBd");
}

pub mod require_rent_exempt_split_destination {
    solana_sdk::declare_id!("D2aip4BBr8NPWtU9vLrwrBvbuaQ8w1zV38zFLxx4pfBV");
}

pub mod better_error_codes_for_tx_lamport_check {
    solana_sdk::declare_id!("Ffswd3egL3tccB6Rv3XY6oqfdzn913vUcjCSnpvCKpfx");
}

pub mod update_hashes_per_tick2 {
    solana_sdk::declare_id!("EWme9uFqfy1ikK1jhJs8fM5hxWnK336QJpbscNtizkTU");
}

pub mod update_hashes_per_tick3 {
    solana_sdk::declare_id!("8C8MCtsab5SsfammbzvYz65HHauuUYdbY2DZ4sznH6h5");
}

pub mod update_hashes_per_tick4 {
    solana_sdk::declare_id!("8We4E7DPwF2WfAN8tRTtWQNhi98B99Qpuj7JoZ3Aikgg");
}

pub mod update_hashes_per_tick5 {
    solana_sdk::declare_id!("BsKLKAn1WM4HVhPRDsjosmqSg2J8Tq5xP2s2daDS6Ni4");
}

pub mod update_hashes_per_tick6 {
    solana_sdk::declare_id!("FKu1qYwLQSiehz644H6Si65U5ZQ2cp9GxsyFUfYcuADv");
}

pub mod validate_fee_collector_account {
    solana_sdk::declare_id!("prpFrMtgNmzaNzkPJg9o753fVvbHKqNrNTm76foJ2wm");
}

pub mod enable_zk_transfer_with_fee {
    solana_sdk::declare_id!("zkNLP7EQALfC1TYeB3biDU7akDckj8iPkvh9y2Mt2K3");
}

pub mod drop_legacy_shreds {
    solana_sdk::declare_id!("GV49KKQdBNaiv2pgqhS2Dy3GWYJGXMTVYbYkdk91orRy");
}

pub mod consume_blockstore_duplicate_proofs {
    solana_sdk::declare_id!("6YsBCejwK96GZCkJ6mkZ4b68oP63z2PLoQmWjC7ggTqZ");
}

pub mod index_erasure_conflict_duplicate_proofs {
    solana_sdk::declare_id!("dupPajaLy2SSn8ko42aZz4mHANDNrLe8Nw8VQgFecLa");
}

lazy_static! {
    /// Map of feature identifiers to user-visible description
    pub static ref FEATURE_NAMES: HashMap<Pubkey, &'static str> = [
        (secp256k1_program_enabled::id(), "secp256k1 program"),
        (deprecate_rewards_sysvar::id(), "deprecate unused rewards sysvar"),
        (pico_inflation::id(), "pico inflation"),
        (full_inflation::devnet_and_testnet::id(), "full inflation on devnet and testnet"),
        (spl_token_v2_multisig_fix::id(), "spl-token multisig fix"),
        (no_overflow_rent_distribution::id(), "no overflow rent distribution"),
        (filter_stake_delegation_accounts::id(), "filter stake_delegation_accounts #14062"),
        (require_custodian_for_locked_stake_authorize::id(), "require custodian to authorize withdrawer change for locked stake"),
        (spl_token_v2_self_transfer_fix::id(), "spl-token self-transfer fix"),
        (full_inflation::mainnet::certusone::enable::id(), "full inflation enabled by Certus One"),
        (full_inflation::mainnet::certusone::vote::id(), "community vote allowing Certus One to enable full inflation"),
        (warp_timestamp_again::id(), "warp timestamp again, adjust bounding to 25% fast 80% slow #15204"),
        (check_init_vote_data::id(), "check initialized Vote data"),
        (secp256k1_recover_syscall_enabled::id(), "secp256k1_recover syscall"),
        (system_transfer_zero_check::id(), "perform all checks for transfers of 0 lamports"),
        (blake3_syscall_enabled::id(), "blake3 syscall"),
        (dedupe_config_program_signers::id(), "dedupe config program signers"),
        (verify_tx_signatures_len::id(), "prohibit extra transaction signatures"),
        (vote_stake_checked_instructions::id(), "vote/state program checked instructions #18345"),
        (rent_for_sysvars::id(), "collect rent from accounts owned by sysvars"),
        (libsecp256k1_0_5_upgrade_enabled::id(), "upgrade libsecp256k1 to v0.5.0"),
        (tx_wide_compute_cap::id(), "transaction wide compute cap"),
        (spl_token_v2_set_authority_fix::id(), "spl-token set_authority fix"),
        (merge_nonce_error_into_system_error::id(), "merge NonceError into SystemError"),
        (disable_fees_sysvar::id(), "disable fees sysvar"),
        (stake_merge_with_unmatched_credits_observed::id(), "allow merging active stakes with unmatched credits_observed #18985"),
        (zk_token_sdk_enabled::id(), "enable Zk Token proof program and syscalls"),
        (curve25519_syscall_enabled::id(), "enable curve25519 syscalls"),
        (versioned_tx_message_enabled::id(), "enable versioned transaction message processing"),
        (libsecp256k1_fail_on_bad_count::id(), "fail libsec256k1_verify if count appears wrong"),
        (libsecp256k1_fail_on_bad_count2::id(), "fail libsec256k1_verify if count appears wrong"),
        (instructions_sysvar_owned_by_sysvar::id(), "fix owner for instructions sysvar"),
        (stake_program_advance_activating_credits_observed::id(), "Enable advancing credits observed for activation epoch #19309"),
        (credits_auto_rewind::id(), "Auto rewind stake's credits_observed if (accidental) vote recreation is detected #22546"),
        (demote_program_write_locks::id(), "demote program write locks to readonly, except when upgradeable loader present #19593 #20265"),
        (ed25519_program_enabled::id(), "enable builtin ed25519 signature verify program"),
        (return_data_syscall_enabled::id(), "enable sol_{set,get}_return_data syscall"),
        (reduce_required_deploy_balance::id(), "reduce required payer balance for program deploys"),
        (sol_log_data_syscall_enabled::id(), "enable sol_log_data syscall"),
        (stakes_remove_delegation_if_inactive::id(), "remove delegations from stakes cache when inactive"),
        (do_support_realloc::id(), "support account data reallocation"),
        (prevent_calling_precompiles_as_programs::id(), "prevent calling precompiles as programs"),
        (optimize_epoch_boundary_updates::id(), "optimize epoch boundary updates"),
        (remove_native_loader::id(), "remove support for the native loader"),
        (send_to_tpu_vote_port::id(), "send votes to the tpu vote port"),
        (requestable_heap_size::id(), "Requestable heap frame size"),
        (disable_fee_calculator::id(), "deprecate fee calculator"),
        (add_compute_budget_program::id(), "Add compute_budget_program"),
        (nonce_must_be_writable::id(), "nonce must be writable"),
        (spl_token_v3_3_0_release::id(), "spl-token v3.3.0 release"),
        (leave_nonce_on_success::id(), "leave nonce as is on success"),
        (reject_empty_instruction_without_program::id(), "fail instructions which have native_loader as program_id directly"),
        (fixed_memcpy_nonoverlapping_check::id(), "use correct check for nonoverlapping regions in memcpy syscall"),
        (reject_non_rent_exempt_vote_withdraws::id(), "fail vote withdraw instructions which leave the account non-rent-exempt"),
        (evict_invalid_stakes_cache_entries::id(), "evict invalid stakes cache entries on epoch boundaries"),
        (allow_votes_to_directly_update_vote_state::id(), "enable direct vote state update"),
        (cap_accounts_data_len::id(), "cap the accounts data len"),
        (max_tx_account_locks::id(), "enforce max number of locked accounts per transaction"),
        (require_rent_exempt_accounts::id(), "require all new transaction accounts with data to be rent-exempt"),
        (filter_votes_outside_slot_hashes::id(), "filter vote slots older than the slot hashes history"),
        (update_syscall_base_costs::id(), "update syscall base costs"),
        (stake_deactivate_delinquent_instruction::id(), "enable the deactivate delinquent stake instruction #23932"),
        (vote_withdraw_authority_may_change_authorized_voter::id(), "vote account withdraw authority may change the authorized voter #22521"),
        (spl_associated_token_account_v1_0_4::id(), "SPL Associated Token Account Program release version 1.0.4, tied to token 3.3.0 #22648"),
        (reject_vote_account_close_unless_zero_credit_epoch::id(), "fail vote account withdraw to 0 unless account earned 0 credits in last completed epoch"),
        (add_get_processed_sibling_instruction_syscall::id(), "add add_get_processed_sibling_instruction_syscall"),
        (bank_transaction_count_fix::id(), "fixes Bank::transaction_count to include all committed transactions, not just successful ones"),
        (disable_bpf_deprecated_load_instructions::id(), "disable ldabs* and ldind* SBF instructions"),
        (disable_bpf_unresolved_symbols_at_runtime::id(), "disable reporting of unresolved SBF symbols at runtime"),
        (record_instruction_in_transaction_context_push::id(), "move the CPI stack overflow check to the end of push"),
        (syscall_saturated_math::id(), "syscalls use saturated math"),
        (check_physical_overlapping::id(), "check physical overlapping regions"),
        (limit_secp256k1_recovery_id::id(), "limit secp256k1 recovery id"),
        (disable_deprecated_loader::id(), "disable the deprecated BPF loader"),
        (check_slice_translation_size::id(), "check size when translating slices"),
        (stake_split_uses_rent_sysvar::id(), "stake split instruction uses rent sysvar"),
        (add_get_minimum_delegation_instruction_to_stake_program::id(), "add GetMinimumDelegation instruction to stake program"),
        (error_on_syscall_bpf_function_hash_collisions::id(), "error on bpf function hash collisions"),
        (reject_callx_r10::id(), "Reject bpf callx r10 instructions"),
        (drop_redundant_turbine_path::id(), "drop redundant turbine path"),
        (executables_incur_cpi_data_cost::id(), "Executables incur CPI data costs"),
        (fix_recent_blockhashes::id(), "stop adding hashes for skipped slots to recent blockhashes"),
        (update_rewards_from_cached_accounts::id(), "update rewards from cached accounts"),
        (enable_partitioned_epoch_reward::id(), "enable partitioned rewards at epoch boundary #32166"),
        (spl_token_v3_4_0::id(), "SPL Token Program version 3.4.0 release #24740"),
        (spl_associated_token_account_v1_1_0::id(), "SPL Associated Token Account Program version 1.1.0 release #24741"),
        (default_units_per_instruction::id(), "Default max tx-wide compute units calculated per instruction"),
        (stake_allow_zero_undelegated_amount::id(), "Allow zero-lamport undelegated amount for initialized stakes #24670"),
        (require_static_program_ids_in_transaction::id(), "require static program ids in versioned transactions"),
        (stake_raise_minimum_delegation_to_1_sol::id(), "Raise minimum stake delegation to 1.0 SOL #24357"),
        (stake_minimum_delegation_for_rewards::id(), "stakes must be at least the minimum delegation to earn rewards"),
        (add_set_compute_unit_price_ix::id(), "add compute budget ix for setting a compute unit price"),
        (disable_deploy_of_alloc_free_syscall::id(), "disable new deployments of deprecated sol_alloc_free_ syscall"),
        (include_account_index_in_rent_error::id(), "include account index in rent tx error #25190"),
        (add_shred_type_to_shred_seed::id(), "add shred-type to shred seed #25556"),
        (warp_timestamp_with_a_vengeance::id(), "warp timestamp again, adjust bounding to 150% slow #25666"),
        (separate_nonce_from_blockhash::id(), "separate durable nonce and blockhash domains #25744"),
        (enable_durable_nonce::id(), "enable durable nonce #25744"),
        (vote_state_update_credit_per_dequeue::id(), "Calculate vote credits for VoteStateUpdate per vote dequeue to match credit awards for Vote instruction"),
        (quick_bail_on_panic::id(), "quick bail on panic"),
        (nonce_must_be_authorized::id(), "nonce must be authorized"),
        (nonce_must_be_advanceable::id(), "durable nonces must be advanceable"),
        (vote_authorize_with_seed::id(), "An instruction you can use to change a vote accounts authority when the current authority is a derived key #25860"),
        (cap_accounts_data_size_per_block::id(), "cap the accounts data size per block #25517"),
        (stake_redelegate_instruction::id(), "enable the redelegate stake instruction #26294"),
        (preserve_rent_epoch_for_rent_exempt_accounts::id(), "preserve rent epoch for rent exempt accounts #26479"),
        (enable_bpf_loader_extend_program_ix::id(), "enable bpf upgradeable loader ExtendProgram instruction #25234"),
        (skip_rent_rewrites::id(), "skip rewriting rent exempt accounts during rent collection #26491"),
        (enable_early_verification_of_account_modifications::id(), "enable early verification of account modifications #25899"),
        (disable_rehash_for_rent_epoch::id(), "on accounts hash calculation, do not try to rehash accounts #28934"),
        (account_hash_ignore_slot::id(), "ignore slot when calculating an account hash #28420"),
        (set_exempt_rent_epoch_max::id(), "set rent epoch to Epoch::MAX for rent-exempt accounts #28683"),
        (on_load_preserve_rent_epoch_for_rent_exempt_accounts::id(), "on bank load account, do not try to fix up rent_epoch #28541"),
        (prevent_crediting_accounts_that_end_rent_paying::id(), "prevent crediting rent paying accounts #26606"),
        (cap_bpf_program_instruction_accounts::id(), "enforce max number of accounts per bpf program instruction #26628"),
        (loosen_cpi_size_restriction::id(), "loosen cpi size restrictions #26641"),
        (use_default_units_in_fee_calculation::id(), "use default units per instruction in fee calculation #26785"),
        (compact_vote_state_updates::id(), "Compact vote state updates to lower block size"),
        (incremental_snapshot_only_incremental_hash_calculation::id(), "only hash accounts in incremental snapshot during incremental snapshot creation #26799"),
        (disable_cpi_setting_executable_and_rent_epoch::id(), "disable setting is_executable and_rent_epoch in CPI #26987"),
        (relax_authority_signer_check_for_lookup_table_creation::id(), "relax authority signer check for lookup table creation #27205"),
        (stop_sibling_instruction_search_at_parent::id(), "stop the search in get_processed_sibling_instruction when the parent instruction is reached #27289"),
        (vote_state_update_root_fix::id(), "fix root in vote state updates #27361"),
        (cap_accounts_data_allocations_per_transaction::id(), "cap accounts data allocations per transaction #27375"),
        (epoch_accounts_hash::id(), "enable epoch accounts hash calculation #27539"),
        (remove_deprecated_request_unit_ix::id(), "remove support for RequestUnitsDeprecated instruction #27500"),
        (increase_tx_account_lock_limit::id(), "increase tx account lock limit to 128 #27241"),
        (limit_max_instruction_trace_length::id(), "limit max instruction trace length #27939"),
        (check_syscall_outputs_do_not_overlap::id(), "check syscall outputs do_not overlap #28600"),
        (enable_bpf_loader_set_authority_checked_ix::id(), "enable bpf upgradeable loader SetAuthorityChecked instruction #28424"),
        (enable_alt_bn128_syscall::id(), "add alt_bn128 syscalls #27961"),
        (enable_program_redeployment_cooldown::id(), "enable program redeployment cooldown #29135"),
        (commission_updates_only_allowed_in_first_half_of_epoch::id(), "validator commission updates are only allowed in the first half of an epoch #29362"),
        (enable_turbine_fanout_experiments::id(), "enable turbine fanout experiments #29393"),
        (disable_turbine_fanout_experiments::id(), "disable turbine fanout experiments #29393"),
        (move_serialized_len_ptr_in_cpi::id(), "cpi ignore serialized_len_ptr #29592"),
        (update_hashes_per_tick::id(), "Update desired hashes per tick on epoch boundary"),
        (enable_big_mod_exp_syscall::id(), "add big_mod_exp syscall #28503"),
        (disable_builtin_loader_ownership_chains::id(), "disable builtin loader ownership chains #29956"),
        (cap_transaction_accounts_data_size::id(), "cap transaction accounts data size up to a limit #27839"),
        (remove_congestion_multiplier_from_fee_calculation::id(), "Remove congestion multiplier from transaction fee calculation #29881"),
        (enable_request_heap_frame_ix::id(), "Enable transaction to request heap frame using compute budget instruction #30076"),
        (prevent_rent_paying_rent_recipients::id(), "prevent recipients of rent rewards from ending in rent-paying state #30151"),
        (delay_visibility_of_program_deployment::id(), "delay visibility of program upgrades #30085"),
        (apply_cost_tracker_during_replay::id(), "apply cost tracker to blocks during replay #29595"),
        (add_set_tx_loaded_accounts_data_size_instruction::id(), "add compute budget instruction for setting account data size per transaction #30366"),
        (switch_to_new_elf_parser::id(), "switch to new ELF parser #30497"),
        (round_up_heap_size::id(), "round up heap size when calculating heap cost #30679"),
        (remove_bpf_loader_incorrect_program_id::id(), "stop incorrectly throwing IncorrectProgramId in bpf_loader #30747"),
        (include_loaded_accounts_data_size_in_fee_calculation::id(), "include transaction loaded accounts data size in base fee calculation #30657"),
        (native_programs_consume_cu::id(), "Native program should consume compute units #30620"),
        (simplify_writable_program_account_check::id(), "Simplify checks performed for writable upgradeable program accounts #30559"),
        (stop_truncating_strings_in_syscalls::id(), "Stop truncating strings in syscalls #31029"),
        (clean_up_delegation_errors::id(), "Return InsufficientDelegation instead of InsufficientFunds or InsufficientStake where applicable #31206"),
        (vote_state_add_vote_latency::id(), "replace Lockout with LandedVote (including vote latency) in vote state #31264"),
        (checked_arithmetic_in_fee_validation::id(), "checked arithmetic in fee validation #31273"),
        (bpf_account_data_direct_mapping::id(), "use memory regions to map account data into the rbpf vm instead of copying the data"),
        (last_restart_slot_sysvar::id(), "enable new sysvar last_restart_slot"),
        (reduce_stake_warmup_cooldown::id(), "reduce stake warmup cooldown from 25% to 9%"),
        (revise_turbine_epoch_stakes::id(), "revise turbine epoch stakes"),
        (enable_poseidon_syscall::id(), "Enable Poseidon syscall"),
        (timely_vote_credits::id(), "use timeliness of votes in determining credits to award"),
        (remaining_compute_units_syscall_enabled::id(), "enable the remaining_compute_units syscall"),
        (enable_program_runtime_v2_and_loader_v4::id(), "Enable Program-Runtime-v2 and Loader-v4 #33293"),
        (require_rent_exempt_split_destination::id(), "Require stake split destination account to be rent exempt"),
        (better_error_codes_for_tx_lamport_check::id(), "better error codes for tx lamport check #33353"),
        (enable_alt_bn128_compression_syscall::id(), "add alt_bn128 compression syscalls"),
        (update_hashes_per_tick2::id(), "Update desired hashes per tick to 2.8M"),
        (update_hashes_per_tick3::id(), "Update desired hashes per tick to 4.4M"),
        (update_hashes_per_tick4::id(), "Update desired hashes per tick to 7.6M"),
        (update_hashes_per_tick5::id(), "Update desired hashes per tick to 9.2M"),
        (update_hashes_per_tick6::id(), "Update desired hashes per tick to 10M"),
        (validate_fee_collector_account::id(), "validate fee collector account #33888"),
        (enable_zk_transfer_with_fee::id(), "enable Zk Token proof program transfer with fee"),
        (drop_legacy_shreds::id(), "drops legacy shreds #34328"),
        (consume_blockstore_duplicate_proofs::id(), "consume duplicate proofs from blockstore in consensus #34372"),
        (index_erasure_conflict_duplicate_proofs::id(), "generate duplicate proofs for index and erasure conflicts #34360"),
        /*************** ADD NEW FEATURES HERE ***************/
    ]
    .iter()
    .cloned()
    .collect();

    /// Unique identifier of the current software's feature set
    pub static ref ID: Hash = {
        let mut hasher = Hasher::default();
        let mut feature_ids = FEATURE_NAMES.keys().collect::<Vec<_>>();
        feature_ids.sort();
        for feature in feature_ids {
            hasher.hash(feature.as_ref());
        }
        hasher.result()
    };
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct FullInflationFeaturePair {
    pub vote_id: Pubkey, // Feature that grants the candidate the ability to enable full inflation
    pub enable_id: Pubkey, // Feature to enable full inflation by the candidate
}

lazy_static! {
    /// Set of feature pairs that once enabled will trigger full inflation
    pub static ref FULL_INFLATION_FEATURE_PAIRS: HashSet<FullInflationFeaturePair> = [
        FullInflationFeaturePair {
            vote_id: full_inflation::mainnet::certusone::vote::id(),
            enable_id: full_inflation::mainnet::certusone::enable::id(),
        },
    ]
    .iter()
    .cloned()
    .collect();
}

/// `FeatureSet` holds the set of currently active/inactive runtime features
#[derive(AbiExample, Debug, Clone, Eq, PartialEq)]
pub struct FeatureSet {
    pub active: HashMap<Pubkey, Slot>,
    pub inactive: HashSet<Pubkey>,
}
impl Default for FeatureSet {
    fn default() -> Self {
        // All features disabled
        Self {
            active: HashMap::new(),
            inactive: FEATURE_NAMES.keys().cloned().collect(),
        }
    }
}
impl FeatureSet {
    pub fn is_active(&self, feature_id: &Pubkey) -> bool {
        self.active.contains_key(feature_id)
    }

    pub fn activated_slot(&self, feature_id: &Pubkey) -> Option<Slot> {
        self.active.get(feature_id).copied()
    }

    /// List of enabled features that trigger full inflation
    pub fn full_inflation_features_enabled(&self) -> HashSet<Pubkey> {
        let mut hash_set = FULL_INFLATION_FEATURE_PAIRS
            .iter()
            .filter_map(|pair| {
                if self.is_active(&pair.vote_id) && self.is_active(&pair.enable_id) {
                    Some(pair.enable_id)
                } else {
                    None
                }
            })
            .collect::<HashSet<_>>();

        if self.is_active(&full_inflation::devnet_and_testnet::id()) {
            hash_set.insert(full_inflation::devnet_and_testnet::id());
        }
        hash_set
    }

    /// All features enabled, useful for testing
    pub fn all_enabled() -> Self {
        Self {
            active: FEATURE_NAMES.keys().cloned().map(|key| (key, 0)).collect(),
            inactive: HashSet::new(),
        }
    }

    /// Activate a feature
    pub fn activate(&mut self, feature_id: &Pubkey, slot: u64) {
        self.inactive.remove(feature_id);
        self.active.insert(*feature_id, slot);
    }

    /// Deactivate a feature
    pub fn deactivate(&mut self, feature_id: &Pubkey) {
        self.active.remove(feature_id);
        self.inactive.insert(*feature_id);
    }

    pub fn new_warmup_cooldown_rate_epoch(&self, epoch_schedule: &EpochSchedule) -> Option<Epoch> {
        self.activated_slot(&reduce_stake_warmup_cooldown::id())
            .map(|slot| epoch_schedule.get_epoch(slot))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_full_inflation_features_enabled_devnet_and_testnet() {
        let mut feature_set = FeatureSet::default();
        assert!(feature_set.full_inflation_features_enabled().is_empty());
        feature_set
            .active
            .insert(full_inflation::devnet_and_testnet::id(), 42);
        assert_eq!(
            feature_set.full_inflation_features_enabled(),
            [full_inflation::devnet_and_testnet::id()]
                .iter()
                .cloned()
                .collect()
        );
    }

    #[test]
    fn test_full_inflation_features_enabled() {
        // Normal sequence: vote_id then enable_id
        let mut feature_set = FeatureSet::default();
        assert!(feature_set.full_inflation_features_enabled().is_empty());
        feature_set
            .active
            .insert(full_inflation::mainnet::certusone::vote::id(), 42);
        assert!(feature_set.full_inflation_features_enabled().is_empty());
        feature_set
            .active
            .insert(full_inflation::mainnet::certusone::enable::id(), 42);
        assert_eq!(
            feature_set.full_inflation_features_enabled(),
            [full_inflation::mainnet::certusone::enable::id()]
                .iter()
                .cloned()
                .collect()
        );

        // Backwards sequence: enable_id and then vote_id
        let mut feature_set = FeatureSet::default();
        assert!(feature_set.full_inflation_features_enabled().is_empty());
        feature_set
            .active
            .insert(full_inflation::mainnet::certusone::enable::id(), 42);
        assert!(feature_set.full_inflation_features_enabled().is_empty());
        feature_set
            .active
            .insert(full_inflation::mainnet::certusone::vote::id(), 42);
        assert_eq!(
            feature_set.full_inflation_features_enabled(),
            [full_inflation::mainnet::certusone::enable::id()]
                .iter()
                .cloned()
                .collect()
        );
    }

    #[test]
    fn test_feature_set_activate_deactivate() {
        let mut feature_set = FeatureSet::default();

        let feature = Pubkey::new_unique();
        assert!(!feature_set.is_active(&feature));
        feature_set.activate(&feature, 0);
        assert!(feature_set.is_active(&feature));
        feature_set.deactivate(&feature);
        assert!(!feature_set.is_active(&feature));
    }
}
