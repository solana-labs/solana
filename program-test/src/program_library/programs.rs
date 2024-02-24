//! Actual library of known program.

use {
    super::{KnownPrograms, ProgramElfSource, ProgramInfoSource},
    solana_sdk::{self, bpf_loader, bpf_loader_upgradeable},
    std::{
        collections::HashMap,
        sync::{Mutex, MutexGuard, OnceLock},
    },
};

mod spl_token {
    solana_sdk::declare_id!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
}
mod spl_token_2022 {
    solana_sdk::declare_id!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
}
mod spl_memo_1_0 {
    solana_sdk::declare_id!("Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo");
}
mod spl_memo_3_0 {
    solana_sdk::declare_id!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
}
mod spl_associated_token_account {
    solana_sdk::declare_id!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
}

macro_rules! compiled_in {
    ($path:literal) => {
        ProgramElfSource::CompiledIn(include_bytes!($path))
    };
}

pub(crate) type Library = HashMap<KnownPrograms, ProgramInfoSource>;

// All the program in the library.
//
// In order to load larger programs from files at runtime, we wrap it in a mutex.
// Every call returns a new locked guard.
pub(crate) fn library() -> MutexGuard<'static, Library> {
    static LIBRARY: OnceLock<Mutex<Library>> = OnceLock::new();

    LIBRARY
        .get_or_init(|| {
            Mutex::new(HashMap::from([
                (
                    KnownPrograms::Spl_Token_V3_5_0,
                    ProgramInfoSource {
                        program_id: spl_token::ID,
                        is_spl: true,
                        default_loader: bpf_loader::ID,
                        elf: compiled_in!("programs/spl_token-3.5.0.so"),
                    },
                ),
                (
                    KnownPrograms::Spl_Token2022_V1_0_0,
                    ProgramInfoSource {
                        program_id: spl_token_2022::ID,
                        is_spl: true,
                        default_loader: bpf_loader_upgradeable::ID,
                        elf: compiled_in!("programs/spl_token_2022-1.0.0.so"),
                    },
                ),
                (
                    KnownPrograms::Spl_Memo_V1_0_0,
                    ProgramInfoSource {
                        program_id: spl_memo_1_0::ID,
                        is_spl: true,
                        default_loader: bpf_loader::ID,
                        elf: compiled_in!("programs/spl_memo-1.0.0.so"),
                    },
                ),
                (
                    KnownPrograms::Spl_Memo_V3_0_0,
                    ProgramInfoSource {
                        program_id: spl_memo_3_0::ID,
                        is_spl: true,
                        default_loader: bpf_loader::ID,
                        elf: compiled_in!("programs/spl_memo-3.0.0.so"),
                    },
                ),
                (
                    KnownPrograms::Spl_AssociatedTokenAccount_V1_1_1,
                    ProgramInfoSource {
                        program_id: spl_associated_token_account::ID,
                        is_spl: true,
                        default_loader: bpf_loader::ID,
                        elf: compiled_in!("programs/spl_associated_token_account-1.1.1.so"),
                    },
                ),
            ]))
        })
        .lock()
        .unwrap()
}
