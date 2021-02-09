use solana_sdk::{account::Account, pubkey::Pubkey, rent::Rent};

mod spl_token {
    solana_sdk::declare_id!("HMGr16f8Ct1Zeb9TGPypt9rPgzCkmhCQB8Not8vwiPW1");
}
mod spl_memo {
    solana_sdk::declare_id!("4DDUJ1rA8Vd7e6SFWanf4V8JnsfapjCGNutQYw8Vtt45");
}
mod spl_associated_token_account {
    solana_sdk::declare_id!("CA6qXxzup4Fh4q4yRAoFcry4FHmgcHwA46r8D17AKQbn");
}

static SPL_PROGRAMS: &[(Pubkey, &[u8])] = &[
    (spl_token::ID, include_bytes!("programs/spl_token-2.0.6.so")),
    (spl_memo::ID, include_bytes!("programs/spl_memo-1.0.0.so")),
    (
        spl_associated_token_account::ID,
        include_bytes!("programs/spl_associated-token-account-1.0.1.so"),
    ),
];

pub fn spl_programs(rent: &Rent) -> Vec<(Pubkey, Account)> {
    SPL_PROGRAMS
        .iter()
        .map(|(program_id, elf)| {
            (
                *program_id,
                Account {
                    lamports: rent.minimum_balance(elf.len()).min(1),
                    data: elf.to_vec(),
                    owner: solana_program::bpf_loader::id(),
                    executable: true,
                    rent_epoch: 0,
                },
            )
        })
        .collect()
}
