#![no_std]

pub mod address_lookup_table {
    solana_pubkey::declare_id!("AddressLookupTab1e1111111111111111111111111");
}

pub mod bpf_loader {
    solana_pubkey::declare_id!("BPFLoader2111111111111111111111111111111111");
}

pub mod bpf_loader_deprecated {
    solana_pubkey::declare_id!("BPFLoader1111111111111111111111111111111111");
}

pub mod bpf_loader_upgradeable {
    solana_pubkey::declare_id!("BPFLoaderUpgradeab1e11111111111111111111111");
}

pub mod compute_budget {
    solana_pubkey::declare_id!("ComputeBudget111111111111111111111111111111");
}

pub mod config {
    solana_pubkey::declare_id!("Config1111111111111111111111111111111111111");
}

pub mod ed25519_program {
    solana_pubkey::declare_id!("Ed25519SigVerify111111111111111111111111111");
}

pub mod feature {
    solana_pubkey::declare_id!("Feature111111111111111111111111111111111111");
}

pub mod loader_v4 {
    solana_pubkey::declare_id!("LoaderV411111111111111111111111111111111111");
}

pub mod native_loader {
    solana_pubkey::declare_id!("NativeLoader1111111111111111111111111111111");
}

pub mod secp256k1_program {
    solana_pubkey::declare_id!("KeccakSecp256k11111111111111111111111111111");
}

pub mod secp256r1_program {
    solana_pubkey::declare_id!("Secp256r1SigVerify1111111111111111111111111");
}

pub mod stake {
    pub mod config {
        solana_pubkey::declare_deprecated_id!("StakeConfig11111111111111111111111111111111");
    }
    solana_pubkey::declare_id!("Stake11111111111111111111111111111111111111");
}

pub mod system_program {
    solana_pubkey::declare_id!("11111111111111111111111111111111");
}

pub mod vote {
    solana_pubkey::declare_id!("Vote111111111111111111111111111111111111111");
}

pub mod sysvar {
    // Owner pubkey for sysvar accounts
    solana_pubkey::declare_id!("Sysvar1111111111111111111111111111111111111");
    pub mod clock {
        solana_pubkey::declare_id!("SysvarC1ock11111111111111111111111111111111");
    }
    pub mod epoch_rewards {
        solana_pubkey::declare_id!("SysvarEpochRewards1111111111111111111111111");
    }
    pub mod epoch_schedule {
        solana_pubkey::declare_id!("SysvarEpochSchedu1e111111111111111111111111");
    }
    pub mod fees {
        solana_pubkey::declare_id!("SysvarFees111111111111111111111111111111111");
    }
    pub mod instructions {
        solana_pubkey::declare_id!("Sysvar1nstructions1111111111111111111111111");
    }
    pub mod last_restart_slot {
        solana_pubkey::declare_id!("SysvarLastRestartS1ot1111111111111111111111");
    }
    pub mod recent_blockhashes {
        solana_pubkey::declare_id!("SysvarRecentB1ockHashes11111111111111111111");
    }
    pub mod rent {
        solana_pubkey::declare_id!("SysvarRent111111111111111111111111111111111");
    }
    pub mod rewards {
        solana_pubkey::declare_id!("SysvarRewards111111111111111111111111111111");
    }
    pub mod slot_hashes {
        solana_pubkey::declare_id!("SysvarS1otHashes111111111111111111111111111");
    }
    pub mod slot_history {
        solana_pubkey::declare_id!("SysvarS1otHistory11111111111111111111111111");
    }
    pub mod stake_history {
        solana_pubkey::declare_id!("SysvarStakeHistory1111111111111111111111111");
    }
}

pub mod zk_token_proof_program {
    solana_pubkey::declare_id!("ZkTokenProof1111111111111111111111111111111");
}

pub mod zk_elgamal_proof_program {
    solana_pubkey::declare_id!("ZkE1Gama1Proof11111111111111111111111111111");
}
