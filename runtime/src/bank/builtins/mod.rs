pub(crate) mod core_bpf_migration;
pub mod prototypes;

pub use prototypes::{BuiltinPrototype, StatelessBuiltinPrototype};
use {
    core_bpf_migration::CoreBpfMigrationConfig,
    solana_feature_set as feature_set,
    solana_sdk::{bpf_loader, bpf_loader_deprecated, bpf_loader_upgradeable},
};

macro_rules! testable_prototype {
    ($prototype:ident {
        core_bpf_migration_config: $core_bpf_migration_config:expr,
        name: $name:ident,
        $($field:ident : $value:expr),* $(,)?
    }) => {
        $prototype {
            core_bpf_migration_config: {
                #[cfg(not(test))]
                {
                    $core_bpf_migration_config
                }
                #[cfg(test)]
                {
                    Some( test_only::$name::CONFIG )
                }
            },
            name: stringify!($name),
            $($field: $value),*
        }
    };
}

pub static BUILTINS: &[BuiltinPrototype] = &[
    testable_prototype!(BuiltinPrototype {
        core_bpf_migration_config: None,
        name: system_program,
        enable_feature_id: None,
        program_id: solana_system_program::id(),
        entrypoint: solana_system_program::system_processor::Entrypoint::vm,
    }),
    testable_prototype!(BuiltinPrototype {
        core_bpf_migration_config: None,
        name: vote_program,
        enable_feature_id: None,
        program_id: solana_vote_program::id(),
        entrypoint: solana_vote_program::vote_processor::Entrypoint::vm,
    }),
    BuiltinPrototype {
        core_bpf_migration_config: Some(CoreBpfMigrationConfig {
            source_buffer_address: buffer_accounts::stake_program::id(),
            upgrade_authority_address: None,
            feature_id: solana_feature_set::migrate_stake_program_to_core_bpf::id(),
            migration_target: core_bpf_migration::CoreBpfMigrationTargetType::Builtin,
            datapoint_name: "migrate_builtin_to_core_bpf_stake_program",
        }),
        name: "stake_program",
        enable_feature_id: None,
        program_id: solana_stake_program::id(),
        entrypoint: solana_stake_program::stake_instruction::Entrypoint::vm,
    },
    BuiltinPrototype {
        core_bpf_migration_config: Some(CoreBpfMigrationConfig {
            source_buffer_address: buffer_accounts::config_program::id(),
            upgrade_authority_address: None,
            feature_id: solana_feature_set::migrate_config_program_to_core_bpf::id(),
            migration_target: core_bpf_migration::CoreBpfMigrationTargetType::Builtin,
            datapoint_name: "migrate_builtin_to_core_bpf_config_program",
        }),
        name: "config_program",
        enable_feature_id: None,
        program_id: solana_config_program::id(),
        entrypoint: solana_config_program::config_processor::Entrypoint::vm,
    },
    testable_prototype!(BuiltinPrototype {
        core_bpf_migration_config: None,
        name: solana_bpf_loader_deprecated_program,
        enable_feature_id: None,
        program_id: bpf_loader_deprecated::id(),
        entrypoint: solana_bpf_loader_program::Entrypoint::vm,
    }),
    testable_prototype!(BuiltinPrototype {
        core_bpf_migration_config: None,
        name: solana_bpf_loader_program,
        enable_feature_id: None,
        program_id: bpf_loader::id(),
        entrypoint: solana_bpf_loader_program::Entrypoint::vm,
    }),
    testable_prototype!(BuiltinPrototype {
        core_bpf_migration_config: None,
        name: solana_bpf_loader_upgradeable_program,
        enable_feature_id: None,
        program_id: bpf_loader_upgradeable::id(),
        entrypoint: solana_bpf_loader_program::Entrypoint::vm,
    }),
    testable_prototype!(BuiltinPrototype {
        core_bpf_migration_config: None,
        name: compute_budget_program,
        enable_feature_id: None,
        program_id: solana_sdk::compute_budget::id(),
        entrypoint: solana_compute_budget_program::Entrypoint::vm,
    }),
    BuiltinPrototype {
        core_bpf_migration_config: Some(CoreBpfMigrationConfig {
            source_buffer_address: buffer_accounts::address_lookup_table_program::id(),
            upgrade_authority_address: None,
            feature_id: solana_feature_set::migrate_address_lookup_table_program_to_core_bpf::id(),
            migration_target: core_bpf_migration::CoreBpfMigrationTargetType::Builtin,
            datapoint_name: "migrate_builtin_to_core_bpf_address_lookup_table_program",
        }),
        name: "address_lookup_table_program",
        enable_feature_id: None,
        program_id: solana_sdk::address_lookup_table::program::id(),
        entrypoint: solana_address_lookup_table_program::processor::Entrypoint::vm,
    },
    testable_prototype!(BuiltinPrototype {
        core_bpf_migration_config: None,
        name: zk_token_proof_program,
        enable_feature_id: Some(feature_set::zk_token_sdk_enabled::id()),
        program_id: solana_zk_token_sdk::zk_token_proof_program::id(),
        entrypoint: solana_zk_token_proof_program::Entrypoint::vm,
    }),
    testable_prototype!(BuiltinPrototype {
        core_bpf_migration_config: None,
        name: loader_v4,
        enable_feature_id: Some(feature_set::enable_program_runtime_v2_and_loader_v4::id()),
        program_id: solana_sdk::loader_v4::id(),
        entrypoint: solana_loader_v4_program::Entrypoint::vm,
    }),
    testable_prototype!(BuiltinPrototype {
        core_bpf_migration_config: None,
        name: zk_elgamal_proof_program,
        enable_feature_id: Some(feature_set::zk_elgamal_proof_program_enabled::id()),
        program_id: solana_zk_sdk::zk_elgamal_proof_program::id(),
        entrypoint: solana_zk_elgamal_proof_program::Entrypoint::vm,
    }),
];

pub static STATELESS_BUILTINS: &[StatelessBuiltinPrototype] = &[StatelessBuiltinPrototype {
    core_bpf_migration_config: Some(CoreBpfMigrationConfig {
        source_buffer_address: buffer_accounts::feature_gate_program::id(),
        upgrade_authority_address: None,
        feature_id: solana_feature_set::migrate_feature_gate_program_to_core_bpf::id(),
        migration_target: core_bpf_migration::CoreBpfMigrationTargetType::Stateless,
        datapoint_name: "migrate_stateless_to_core_bpf_feature_gate_program",
    }),
    name: "feature_gate_program",
    program_id: solana_sdk::feature::id(),
}];

/// Live source buffer accounts for builtin migrations.
mod buffer_accounts {
    pub mod address_lookup_table_program {
        solana_sdk::declare_id!("AhXWrD9BBUYcKjtpA3zuiiZG4ysbo6C6wjHo1QhERk6A");
    }
    pub mod config_program {
        solana_sdk::declare_id!("BuafH9fBv62u6XjzrzS4ZjAE8963ejqF5rt1f8Uga4Q3");
    }
    pub mod feature_gate_program {
        solana_sdk::declare_id!("3D3ydPWvmEszrSjrickCtnyRSJm1rzbbSsZog8Ub6vLh");
    }
    pub mod stake_program {
        solana_sdk::declare_id!("8t3vv6v99tQA6Gp7fVdsBH66hQMaswH5qsJVqJqo8xvG");
    }
}

// This module contains a number of arbitrary addresses used for testing Core
// BPF migrations.
// Since the list of builtins is static, using `declare_id!` with constant
// values is arguably the least-overhead approach to injecting static addresses
// into the builtins list for both the feature ID and the source program ID.
// These arbitrary IDs can then be used to configure feature-activation runtime
// tests.
#[cfg(test)]
mod test_only {
    use super::core_bpf_migration::{CoreBpfMigrationConfig, CoreBpfMigrationTargetType};
    pub mod system_program {
        pub mod feature {
            solana_sdk::declare_id!("AnjsdWg7LXFbjDdy78wncCJs9PyTdWpKkFmHAwQU1mQ6");
        }
        pub mod source_buffer {
            solana_sdk::declare_id!("EDEhzg1Jk79Wrk4mwpRa7txjgRxcE6igXwd6egFDVhuz");
        }
        pub mod upgrade_authority {
            solana_sdk::declare_id!("4d14UK2o1FKKoecEBWhVDZrBBbRuhug75G1j9XYCawC2");
        }
        pub const CONFIG: super::CoreBpfMigrationConfig = super::CoreBpfMigrationConfig {
            source_buffer_address: source_buffer::id(),
            upgrade_authority_address: Some(upgrade_authority::id()),
            feature_id: feature::id(),
            migration_target: super::CoreBpfMigrationTargetType::Builtin,
            datapoint_name: "migrate_builtin_to_core_bpf_system_program",
        };
    }

    pub mod vote_program {
        pub mod feature {
            solana_sdk::declare_id!("5wDLHMasPmtrcpfRZX67RVkBXBbSTQ9S4C8EJomD3yAk");
        }
        pub mod source_buffer {
            solana_sdk::declare_id!("6T9s4PTcHnpq2AVAqoCbJd4FuHsdD99MjSUEbS7qb1tT");
        }
        pub mod upgrade_authority {
            solana_sdk::declare_id!("2N4JfyYub6cWUP9R4JrsFHv6FYKT7JnoRX8GQUH9MdT3");
        }
        pub const CONFIG: super::CoreBpfMigrationConfig = super::CoreBpfMigrationConfig {
            source_buffer_address: source_buffer::id(),
            upgrade_authority_address: Some(upgrade_authority::id()),
            feature_id: feature::id(),
            migration_target: super::CoreBpfMigrationTargetType::Builtin,
            datapoint_name: "migrate_builtin_to_core_bpf_vote_program",
        };
    }

    pub mod solana_bpf_loader_deprecated_program {
        pub mod feature {
            solana_sdk::declare_id!("8gpakCv5Pk5PZGv9RUjzdkk2GVQPGx12cNRUDMQ3bP86");
        }
        pub mod source_buffer {
            solana_sdk::declare_id!("DveUYB5m9G3ce4zpV3fxg9pCNkvH1wDsyd8XberZ47JL");
        }
        pub mod upgrade_authority {
            solana_sdk::declare_id!("8Y5VTHdadnz4rZZWdUA4Qq2m2zWoCwwtb38spPZCXuGU");
        }
        pub const CONFIG: super::CoreBpfMigrationConfig = super::CoreBpfMigrationConfig {
            source_buffer_address: source_buffer::id(),
            upgrade_authority_address: Some(upgrade_authority::id()),
            feature_id: feature::id(),
            migration_target: super::CoreBpfMigrationTargetType::Builtin,
            datapoint_name: "migrate_builtin_to_core_bpf_bpf_loader_deprecated_program",
        };
    }

    pub mod solana_bpf_loader_program {
        pub mod feature {
            solana_sdk::declare_id!("8yEdUm4SaP1yNq2MczEVdrM48SucvZCTDSqjcAKfYfL6");
        }
        pub mod source_buffer {
            solana_sdk::declare_id!("2EWMYGJPuGLW4TexLLEMeXP2BkB1PXEKBFb698yw6LhT");
        }
        pub mod upgrade_authority {
            solana_sdk::declare_id!("3sQ9VZ1Lvuvs6NpFXFV3ByFAf52ajPPdXwuhYERJR3iJ");
        }
        pub const CONFIG: super::CoreBpfMigrationConfig = super::CoreBpfMigrationConfig {
            source_buffer_address: source_buffer::id(),
            upgrade_authority_address: Some(upgrade_authority::id()),
            feature_id: feature::id(),
            migration_target: super::CoreBpfMigrationTargetType::Builtin,
            datapoint_name: "migrate_builtin_to_core_bpf_bpf_loader_program",
        };
    }

    pub mod solana_bpf_loader_upgradeable_program {
        pub mod feature {
            solana_sdk::declare_id!("oPQbVjgoQ7SaQmzZiiHW4xqHbh4BJqqrFhxEJZiMiwY");
        }
        pub mod source_buffer {
            solana_sdk::declare_id!("6bTmA9iefD57GDoQ9wUjG8SeYkSpRw3EkKzxZCbhkavq");
        }
        pub mod upgrade_authority {
            solana_sdk::declare_id!("CuJvJY1K2wx82oLrQGSSWtw4AF7nVifEHupzSC2KEcq5");
        }
        pub const CONFIG: super::CoreBpfMigrationConfig = super::CoreBpfMigrationConfig {
            source_buffer_address: source_buffer::id(),
            upgrade_authority_address: Some(upgrade_authority::id()),
            feature_id: feature::id(),
            migration_target: super::CoreBpfMigrationTargetType::Builtin,
            datapoint_name: "migrate_builtin_to_core_bpf_bpf_loader_upgradeable_program",
        };
    }

    pub mod compute_budget_program {
        pub mod feature {
            solana_sdk::declare_id!("D39vUspVfhjPVD7EtMJZrA5j1TSMp4LXfb43nxumGdHT");
        }
        pub mod source_buffer {
            solana_sdk::declare_id!("KfX1oLpFC5CwmFeSgXrNcXaouKjFkPuSJ4UsKb3zKMX");
        }
        pub mod upgrade_authority {
            solana_sdk::declare_id!("HGTbQhaCXNTbpgpLb2KNjqWSwpJyb2dqDB66Lc3Ph4aN");
        }
        pub const CONFIG: super::CoreBpfMigrationConfig = super::CoreBpfMigrationConfig {
            source_buffer_address: source_buffer::id(),
            upgrade_authority_address: Some(upgrade_authority::id()),
            feature_id: feature::id(),
            migration_target: super::CoreBpfMigrationTargetType::Builtin,
            datapoint_name: "migrate_builtin_to_core_bpf_compute_budget_program",
        };
    }

    pub mod zk_token_proof_program {
        pub mod feature {
            solana_sdk::declare_id!("GfeFwUzKP9NmaP5u4VfnFgEvQoeQc2wPgnBFrUZhpib5");
        }
        pub mod source_buffer {
            solana_sdk::declare_id!("Ffe9gL8vXraBkiv3HqbLvBqY7i9V4qtZxjH83jYYDe1V");
        }
        pub mod upgrade_authority {
            solana_sdk::declare_id!("6zkXWHR8YeCvfMqHwyiz2n7g6hMUKCFhrVccZZTDk4ei");
        }
        pub const CONFIG: super::CoreBpfMigrationConfig = super::CoreBpfMigrationConfig {
            source_buffer_address: source_buffer::id(),
            upgrade_authority_address: Some(upgrade_authority::id()),
            feature_id: feature::id(),
            migration_target: super::CoreBpfMigrationTargetType::Builtin,
            datapoint_name: "migrate_builtin_to_core_bpf_zk_token_proof_program",
        };
    }

    pub mod loader_v4 {
        pub mod feature {
            solana_sdk::declare_id!("Cz5JthYp27KR3rwTCtVJhbRgwHCurbwcYX46D8setL22");
        }
        pub mod source_buffer {
            solana_sdk::declare_id!("EH45pKy1kzjifB93wEJi91js3S4HETdsteywR7ZCNPn5");
        }
        pub mod upgrade_authority {
            solana_sdk::declare_id!("AWbiYRbFts9GVX5uwUkwV46hTFP85PxCAM8e8ir8Hqtq");
        }
        pub const CONFIG: super::CoreBpfMigrationConfig = super::CoreBpfMigrationConfig {
            source_buffer_address: source_buffer::id(),
            upgrade_authority_address: Some(upgrade_authority::id()),
            feature_id: feature::id(),
            migration_target: super::CoreBpfMigrationTargetType::Builtin,
            datapoint_name: "migrate_builtin_to_core_bpf_loader_v4_program",
        };
    }

    pub mod zk_elgamal_proof_program {
        pub mod feature {
            solana_sdk::declare_id!("EYtuxScWqGWmcPEDmeUsEt3iPkvWE26EWLfSxUvWP2WN");
        }
        pub mod source_buffer {
            solana_sdk::declare_id!("AaVrLPurAUmjw6XRNGr6ezQfHaJWpBGHhcRSJmNjoVpQ");
        }
        pub mod upgrade_authority {
            solana_sdk::declare_id!("EyGkQYHgynUdvdNPNiWbJQk9roFCexgdJQMNcWbuvp78");
        }
        pub const CONFIG: super::CoreBpfMigrationConfig = super::CoreBpfMigrationConfig {
            source_buffer_address: source_buffer::id(),
            upgrade_authority_address: Some(upgrade_authority::id()),
            feature_id: feature::id(),
            migration_target: super::CoreBpfMigrationTargetType::Builtin,
            datapoint_name: "migrate_builtin_to_core_bpf_zk_elgamal_proof_program",
        };
    }
}

#[cfg(test)]
mod tests {
    // Since a macro is used to initialize the test IDs from the `test_only`
    // module, best to ensure the lists have the expected values within a test
    // context.
    #[test]
    fn test_testable_prototypes() {
        assert_eq!(
            &super::BUILTINS[0].core_bpf_migration_config,
            &Some(super::test_only::system_program::CONFIG)
        );
        assert_eq!(
            &super::BUILTINS[1].core_bpf_migration_config,
            &Some(super::test_only::vote_program::CONFIG)
        );
        // Stake has a live migration config, so it has no test-only configs
        // to test here.
        // Config has a live migration config, so it has no test-only configs
        // to test here.
        assert_eq!(
            &super::BUILTINS[4].core_bpf_migration_config,
            &Some(super::test_only::solana_bpf_loader_deprecated_program::CONFIG)
        );
        assert_eq!(
            &super::BUILTINS[5].core_bpf_migration_config,
            &Some(super::test_only::solana_bpf_loader_program::CONFIG)
        );
        assert_eq!(
            &super::BUILTINS[6].core_bpf_migration_config,
            &Some(super::test_only::solana_bpf_loader_upgradeable_program::CONFIG)
        );
        assert_eq!(
            &super::BUILTINS[7].core_bpf_migration_config,
            &Some(super::test_only::compute_budget_program::CONFIG)
        );
        // Address Lookup Table has a live migration config, so it has no
        // test-only configs to test here.
        assert_eq!(
            &super::BUILTINS[9].core_bpf_migration_config,
            &Some(super::test_only::zk_token_proof_program::CONFIG)
        );
        assert_eq!(
            &super::BUILTINS[10].core_bpf_migration_config,
            &Some(super::test_only::loader_v4::CONFIG)
        );
        assert_eq!(
            &super::BUILTINS[11].core_bpf_migration_config,
            &Some(super::test_only::zk_elgamal_proof_program::CONFIG)
        );
        // Feature Gate has a live migration config, so it has no test-only
        // configs to test here.
    }
}
