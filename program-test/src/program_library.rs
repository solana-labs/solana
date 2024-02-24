//! Holds compiled programs to be used in tests.

use {
    solana_sdk::{
        account::AccountSharedData, bpf_loader, bpf_loader_upgradeable, pubkey::Pubkey, rent::Rent,
    },
    std::path::PathBuf,
    thiserror::Error,
};

pub mod accounts;
mod programs;

pub use accounts::ProgramAccounts;

/// All programs that are stored in this library are identified by this enum entries.
// It seems reasonable to separate the program name from the version using an underscore.  But that
// generates a name that does not pass the camel case types linter.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KnownPrograms {
    Spl_Token_V3_5_0,
    Spl_Token2022_V1_0_0,
    Spl_Memo_V1_0_0,
    Spl_Memo_V3_0_0,
    Spl_AssociatedTokenAccount_V1_1_1,
}

impl KnownPrograms {
    /// Returns a [`ProgramInfo`] for the specified program.  Allowing further construction of the
    /// accounts for the program via, for example, a [`ProgramInfo::accounts()`] call.
    pub fn info(self) -> ProgramInfo {
        let mut library_guard = programs::library();

        let entry = library_guard
            .get_mut(&self)
            .expect("`LIBRARY` contains entries for all the known programs");

        entry
            .produce_info()
            .expect("Library is properly backed by all the necessary data files")
    }

    /// Return [`ProgramInfo`] for all SPL programs in the library.
    pub fn all_spl_programs() -> Vec<ProgramInfo> {
        let mut library_guard = programs::library();

        library_guard
            .iter_mut()
            .filter(|(_, info)| info.is_spl)
            .map(|(_, info)| info.produce_info())
            .collect::<Result<Vec<_>, _>>()
            .expect("Library is properly backed by all the necessary data files")
    }

    /// Calls [`ProgramInfo::accounts_with_default_loader()`] on all the elements returned by
    /// [`all_spl_programs()`].
    pub fn all_spl_program_accounts(rent: &Rent) -> Vec<(Pubkey, AccountSharedData)> {
        Self::all_spl_programs()
            .iter()
            .flat_map(|info| {
                let accounts = info.accounts_with_default_loader(&rent);
                Vec::<_>::from(accounts).into_iter()
            })
            .collect()
    }
}

pub type ProgramElf = Vec<u8>;

/// Describes a program.  It is not yet deployed and no accounts exist for it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProgramInfo {
    /// Programs define their identity even before they are deployed.
    ///
    /// When accounts are created for this program, they are created using this address.
    /// This only works for environments, where programs can be deployed at an address without
    /// knowing it's private key.
    program_id: Pubkey,

    /// Programs that are taken from Solana Program Library have this flag set.
    ///
    /// This is just for convenience.
    is_spl: bool,

    /// Loader used by the main net deployment of this program.
    /// This is only used by the [`ProgramAccounts::accounts_with_default_loader()`] call.
    /// This would be one of the loaders supported by the [`ProgramInfo::accounts()`] call.
    default_loader: Pubkey,

    /// Program ELF, as produced by the program compilation.
    elf: ProgramElf,
}

#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountConstructionError {
    #[error("Program does not have a ")]
    UnknownAddress(Pubkey),
}

impl ProgramInfo {
    pub fn accounts(&self, rent: &Rent, loader_id: Pubkey) -> ProgramAccounts {
        let ProgramInfo {
            program_id, elf, ..
        } = self;

        match loader_id {
            bpf_loader::ID => ProgramAccounts::loader_v2(rent, *program_id, elf),
            bpf_loader_upgradeable::ID => ProgramAccounts::loader_v3(rent, *program_id, elf),
            unexpected => panic!("Unexpected program loader: {unexpected}"),
        }
    }

    pub fn accounts_with_default_loader(&self, rent: &Rent) -> ProgramAccounts {
        self.accounts(rent, self.default_loader)
    }
}

/// ELF bytes might be either compiled into the application image, for example, for smaller
/// programs, or they might need to be loaded from disk.
enum ProgramElfSource {
    /// Bytes are part of the application image.
    CompiledIn(&'static [u8]),
    /// Byte need to be loaded from the specified path.
    // TODO
    #[allow(dead_code)]
    LoadFrom(PathBuf),
    /// Byte have been loaded and are directly available.
    Owned(Vec<u8>),
}

/// Just like [`ProgramInfo`], but ELF bytes could be either part of the application image, or be
/// absent, ponting
struct ProgramInfoSource {
    program_id: Pubkey,
    is_spl: bool,
    default_loader: Pubkey,
    elf: ProgramElfSource,
}

impl ProgramInfoSource {
    // Creates an instance of [`ProgramInfo`], based on teh `ProgramInfoSource`.
    //
    // It might need to load the program ELF from disk, in which case it will update `self` to hold
    // the loaded bytes.  Avoiding disk I/O on a subsequent load.
    fn produce_info(&mut self) -> Result<ProgramInfo, ()> {
        let ProgramInfoSource {
            program_id,
            is_spl,
            default_loader,
            elf,
        } = self;

        let elf = match elf {
            ProgramElfSource::CompiledIn(elf) => elf.to_vec(),
            ProgramElfSource::LoadFrom(_source) => {
                let res = vec![];
                *elf = ProgramElfSource::Owned(res.clone());

                todo!("Implement loading");

                // TODO
                #[allow(unreachable_code)]
                res
            }
            ProgramElfSource::Owned(elf) => elf.clone(),
        };

        Ok(ProgramInfo {
            program_id: *program_id,
            is_spl: *is_spl,
            default_loader: *default_loader,
            elf,
        })
    }
}
