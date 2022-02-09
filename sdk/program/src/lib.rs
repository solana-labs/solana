//! The base library for all Solana on-chain Rust programs.
//!
//! All Solana Rust programs that run on-chain will link to this crate, which
//! acts as a standard library for Solana programs. Solana programs also link to
//! the [Rust standard library][std], though it is [modified][hm] for the
//! Solana runtime environment. While off-chain programs that interact with the
//! Solana network _can_ link to this crate, they typically instead use the
//! [`solana-sdk`] crate, which reexports all modules from `solana-program`.
//!
//! [std]: https://doc.rust-lang.org/stable/std/
//! [hm]: #limitations-to-the-solana-version-of-the-rust-standard-library
//! [`solana-sdk`]: https://docs.rs/solana-sdk/latest/solana_sdk/
//!
//! This library defines
//!
//! - macros for declaring the [program entrypoint][pe],
//! - [core data types][cdt],
//! - [logging] macros,
//! - [serialization] methods,
//! - methods for [cross-program instruction execution][cpi],
//! - program IDs and instruction constructors for the system program and other
//!   [native programs][np],
//! - [sysvar] accessors.
//!
//! [pe]: #defining-a-solana-program
//! [cdt]: #core-data-types
//! [logging]: crate::log
//! [serialization]: #serialization
//! [np]: #native-programs
//! [cpi]: #cross-program-instruction-execution
//! [sysvar]: #sysvars
//! [spec]: #other-special-accounts
//!
//! Idiomatic examples of `solana-program` usage can be found in
//! [the Solana Program Library][spl].
//!
//! [spl]: https://github.com/solana-labs/solana-program-library
//!
//! # Defining a solana program
//!
//! Solana program crates have some unique properties compared to typical Rust
//! programs:
//!
//! - They are often compiled for both on-chain use and off-chain use. This is
//!   primarily because off-chain clients may need access to data types
//!   defined by the on-chain program.
//! - They do not define a `main` function, but instead define their entrypoint
//!   with the [`entrypoint!`] macro.
//! - They are compiled as the ["cdylib"] crate type for dynamic loading
//!   by the Solana runtime.
//! - They run in a constrained VM environment, and while they do have access to
//!   the [Rust standard library][std], many features of the standard library,
//!   particularly related to I/O, will fail at runtime, will silently do
//!   nothing, or are not defined. See _[limitiations of the Solana version of
//!   the Rust standard library][sstd]_.
//!
//! [std]: https://doc.rust-lang.org/std/index.html
//! [sstd]: #limitations-to-the-solana-version-of-the-rust-standard-library
//! ["cdylib"]: https://doc.rust-lang.org/reference/linkage.html
//!
//! Because multiple crates that are linked together cannot all define
//! program entrypoints (see the [`entrypoint!`] documentation) a common
//! convention is to use a [Cargo feature] called `no-entrypoint` to allow
//! the program entrypoint to be disabled.
//!
//! [Cargo feature]: https://doc.rust-lang.org/cargo/reference/features.html
//!
//! The skeleton of a Solana program typically looks like:
//!
//! ```
//! #[cfg(not(feature = "no-entrypoint"))]
//! pub mod entrypoint {
//!     use solana_program::{
//!         account_info::AccountInfo,
//!         entrypoint,
//!         entrypoint::ProgramResult,
//!         pubkey::Pubkey,
//!     };
//!
//!     entrypoint!(process_instruction);
//!
//!     pub fn process_instruction(
//!         program_id: &Pubkey,
//!         accounts: &[AccountInfo],
//!         instruction_data: &[u8],
//!     ) -> ProgramResult {
//!         // Decode and dispatch instructions here.
//!         todo!()
//!     }
//! }
//!
//! // Additional code goes here.
//! ```
//!
//! With a `Cargo.toml` file that contains
//!
//! ```toml
//! [lib]
//! crate-type = ["cdylib", "rlib"]
//!
//! [features]
//! no-entrypoint = []
//! ```
//!
//! Note that a Solana program must specify its crate-type as "cdylib", and
//! "cdylib" crates will automatically be discovered and built by the `cargo
//! build-bpf` command. Solana programs also often have crate-type "rlib" so
//! they can be linked to other Rust crates.
//!
//! # On-chain vs. off-chain compilation targets
//!
//! Solana programs run on the [rbpf] VM, which implements a variant of the
//! [eBPF] instruction set. Because this crate can be compiled for both on-chain
//! and off-chain execution, the environments of which are significantly
//! different, it extensively uses [conditional compilation][cc] to tailor its
//! implementation to the environment. The `cfg` predicate used for identifying
//! compilation for on-chain programs is `target_arch = "bpf"`, as in this
//! example from the `solana-program` codebase that logs a message via a
//! syscall when run on-chain, and via a library call when offchain:
//!
//! [rbpf]: https://github.com/solana-labs/rbpf
//! [eBPF]: https://ebpf.io/
//! [cc]: https://doc.rust-lang.org/reference/conditional-compilation.html
//!
//! ```
//! pub fn sol_log(message: &str) {
//!     #[cfg(target_arch = "bpf")]
//!     unsafe {
//!         sol_log_(message.as_ptr(), message.len() as u64);
//!     }
//!
//!     #[cfg(not(target_arch = "bpf"))]
//!     program_stubs::sol_log(message);
//! }
//! # mod program_stubs {
//! #     pub(crate) fn sol_log(message: &str) { }
//! # }
//! ```
//!
//! This `cfg` pattern is suitable as well for user code that needs to work both
//! on-chain and off-chain.
//!
//! `solana-program` and `solana-sdk` were previously a single crate. Because of
//! this history, and because of the dual-usage of `solana-program` for two
//! different environments, it contains some features that are not available to
//! on-chain programs at compile-time. It also contains some on-chain features
//! that will fail in off-chain scenarios at runtime. This distinction is not
//! well-reflected in the documentation.
//!
//! For a more complete description of Solana's implementation of eBPF and its
//! limitations, see the main Solana documentation on [on-chain programs][ocp].
//!
//! [ocp]: https://docs.solana.com/developing/on-chain-programs/overview
//!
//! # Core data types
//!
//! - [`Pubkey`] - An [ed25519] public key. All Solana accounts are identified
//!   by a `Pubkey`, though some may not have corresponding private keys. As
//!   Solana programs do not handle secret keys, the full [`Keypair`] is not
//!   defined in `solana-program` but in `solana-sdk`.
//! - [`Hash`] - A [SHA-256] hash, used to uniquely identify blocks.
//! - [`AccountInfo`] - A description of a single Solana account. All accounts
//!   that might be accessed by a program invocation are provided to the program
//!   entrypoint as `AccountInfo`.
//! - [`Instruction`] - A directive telling the runtime to execute a program,
//!   passing it a set of accounts and program-specific data.
//! - [`ProgramError`] and [`ProgramResult`] - The error type that all programs
//!   must return, reported to the runtime as a `u64`.
//!
//! [`Pubkey`]: pubkey::Pubkey
//! [`Hash`]: hash::Hash
//! [`Instruction`]: instruction::Instruction
//! [`AccountInfo`]: account_info::AccountInfo
//! [`ProgramError`]: program_error::ProgramError
//! [`ProgramResult`]: entrypoint::ProgramResult
//!
//! [ed25519]: https://ed25519.cr.yp.to/
//! [`Keypair`]: https://docs.rs/solana-sdk/latest/solana_sdk/signer/keypair/struct.Keypair.html
//! [SHA-256]: https://en.wikipedia.org/wiki/SHA-2
//!
//! # Serialization
//!
//! Within the Solana runtime, programs, and network, at least three different
//! serialization formats are used, and `solana-program` provides access to
//! those needed by programs.
//!
//! In user-written Solana program code, serialization is primarily used for accessing
//! [`AccountInfo`] data and [`Instruction`] data, both of which are program-specific
//! binary data, the format up to the author.
//!
//! [`AccountInfo`]: account_info::AccountInfo
//! [`Instruction`]: instruction::Instruction
//!
//! The three serialization formats in use in Solana are:
//!
//! - __[Borsh]__, a compact and well-specified format developed by the [NEAR]
//!   project, suitable for use in protocol definitions and for archival storage.
//!   It has a [Rust implementation][brust] and a [JavaScript implementation][bjs]
//!   and is recommended for all purposes.
//!
//!   Users need to import the [`borsh`] crate themselves &mdash; it is not
//!   re-exported by `solana-program`, though this crate provides several useful
//!   utilities in its [`borsh` module][borshmod] that are not available in the
//!   `borsh` library.
//!
//!   The `Instruction` type has built-in support for borsh.
//!
//!   [Borsh]: https://borsh.io/
//!   [NEAR]: https://near.org/
//!   [brust]: https://docs.rs/borsh
//!   [bjs]: https://github.com/near/borsh-js
//!   [`borsh`]: https://docs.rs/borsh
//!   [borshmod]: crate::borsh
//!
//! - __[Bincode]__, a compact serialization format that implements the [Serde]
//!   Rust APIs. As it does not have a specification nor a JavaScript
//!   implementation, it is not recommend for new code.
//!
//!   Many system program and native program instructions are serialized with
//!   bincode, and it is used for other purposes in the runtime. In these cases
//!   Rust programmers are generally not directly exposed to the encoding format
//!   as it is hidden behind APIs.
//!
//!   The `Instruction` type has built-in support for bincode.
//!
//!   [Bincode]: https://docs.rs/bincode
//!   [Serde]: https://serde.rs/
//!
//! - __[`Pack`]__, a Solana-specific serialization API that is used by many
//!   older programs in the [Solana Program Library][spl] to define their
//!   account format. It is difficult to implement and does not define a
//!   language-independent serialization format. It is not generally recommended
//!   for new code.
//!
//!   [`Pack`]: program_pack::Pack
//!
//! # Cross-program instruction execution
//!
//! Solana programs may call other programs, termed [_cross-program
//! invocation_][cpi] (CPI), with the [`invoke`] and [`invoke_signed`]
//! functions. When calling another program the caller must provide the
//! [`Instruction`] to be invoked, as well as the [`AccountInfo`] for every
//! account required by the instruction. Because the only way for a program to
//! acquire `AccountInfo` values is by receiving them from the runtime at the
//! [program entrypoint][entrypoint!], any account required by the callee
//! program must transitively be required by the caller program, and provided by
//! _its_ caller.
//!
//! [`invoke`]: program::invoke
//! [`invoke_signed`]: program::invoke_signed
//! [cpi]: https://docs.solana.com/developing/programming-model/calling-between-programs
//!
//! A simple example of transferring lamports via CPI:
//!
//! ```
//! use solana_program::{
//!     account_info::{next_account_info, AccountInfo},
//!     entrypoint,
//!     entrypoint::ProgramResult,
//!     program::invoke,
//!     pubkey::Pubkey,
//!     system_instruction,
//!     system_program,
//! };
//!
//! entrypoint!(process_instruction);
//!
//! fn process_instruction(
//!     program_id: &Pubkey,
//!     accounts: &[AccountInfo],
//!     instruction_data: &[u8],
//! ) -> ProgramResult {
//!     let account_info_iter = &mut accounts.iter();
//!
//!     let payer = next_account_info(account_info_iter)?;
//!     let recipient = next_account_info(account_info_iter)?;
//!     // The system program is a required account to invoke a system
//!     // instruction, even though we don't use it directly.
//!     let system_account = next_account_info(account_info_iter)?;
//!
//!     assert!(payer.is_writable);
//!     assert!(payer.is_signer);
//!     assert!(recipient.is_writable);
//!     assert!(system_program::check_id(system_account.key));
//!
//!     let lamports = 1000000;
//!
//!     invoke(
//!         &system_instruction::transfer(payer.key, recipient.key, lamports),
//!         &[payer.clone(), recipient.clone(), system_account.clone()],
//!     )
//! }
//! ```
//!
//! Solana also includes a mechinasm to let programs control and sign for
//! accounts without needing to protect a corresponding secret key, called
//! [_program derived addresses_][pdas]. PDAs are derived with the
//! [`Pubkey::find_program_address`] function. With a PDA, a program can call
//! `invoke_signed` to call another program while virtually "signing" for the
//! PDA.
//!
//! [pdas]: https://docs.solana.com/developing/programming-model/calling-between-programs#program-derived-addresses
//! [`Pubkey::find_program_address`]: pubkey::Pubkey::find_program_address
//!
//! A simple example of creating an account for a PDA:
//!
//! ```
//! use solana_program::{
//!     account_info::{next_account_info, AccountInfo},
//!     entrypoint,
//!     entrypoint::ProgramResult,
//!     program::invoke_signed,
//!     pubkey::Pubkey,
//!     system_instruction,
//!     system_program,
//! };
//!
//! entrypoint!(process_instruction);
//!
//! fn process_instruction(
//!     program_id: &Pubkey,
//!     accounts: &[AccountInfo],
//!     instruction_data: &[u8],
//! ) -> ProgramResult {
//!     let account_info_iter = &mut accounts.iter();
//!     let payer = next_account_info(account_info_iter)?;
//!     let vault_pda = next_account_info(account_info_iter)?;
//!     let system_program = next_account_info(account_info_iter)?;
//!
//!     assert!(payer.is_writable);
//!     assert!(payer.is_signer);
//!     assert!(vault_pda.is_writable);
//!     assert_eq!(vault_pda.owner, &system_program::ID);
//!     assert!(system_program::check_id(system_program.key));
//!
//!     let vault_bump_seed = instruction_data[0];
//!     let vault_seeds = &[b"vault", payer.key.as_ref(), &[vault_bump_seed]];
//!     let expected_vault_pda = Pubkey::create_program_address(vault_seeds, program_id)?;
//!
//!     assert_eq!(vault_pda.key, &expected_vault_pda);
//!
//!     let lamports = 10000000;
//!     let vault_size = 16;
//!
//!     invoke_signed(
//!         &system_instruction::create_account(
//!             &payer.key,
//!             &vault_pda.key,
//!             lamports,
//!             vault_size,
//!             &program_id,
//!         ),
//!         &[
//!             payer.clone(),
//!             vault_pda.clone(),
//!         ],
//!         &[
//!             &[
//!                 b"vault",
//!                 payer.key.as_ref(),
//!                 &[vault_bump_seed],
//!             ],
//!         ]
//!     )?;
//!     Ok(())
//! }
//! ```
//!
//! # Native programs
//!
//! Some solana programs are [_native programs_][np2], running native machine
//! code that is distributed with the runtime, with well-known program IDs.
//!
//! [np2]: https://docs.solana.com/developing/runtime-facilities/programs
//!
//! Native programs are divided into several categories:
//!
//! __Built-ins__ act just like other Solana programs, in that they can
//! be [invoked] from within Solana programs or from within [transactions]
//! submitted by clients. They implement fundamental operations that require
//! direct access to the runtime.
//!
//! [invoked]: #cross-program-instruction-execution
//! [transactions]: https://docs.rs/solana-sdk/latest/solana_sdk/transaction/index.html
//!
//! __Special built-ins__ include a few programs that are defined like built-ins,
//! but do not work when invoked from another program, instead only functioning
//! when submitted as part of a transaction. The exact behavior when invoked
//! from another program depends on the built-in.
//!
//! __Precompiles__ are extremely compute-intensive operations. These can not be
//! invoked by other programs, and can only be executed as instructions within
//! transactions submitted by clients. Precompile instructions may be
//! batch-executed in parallel with other precompiles prior to execution of
//! other instructions. Invoking a precompile from another program will return
//! an error.
//!
//! __Program loaders__ are responsible for loading and executing Solana programs.
//! There may be multiple program loaders. Today the only program loaders are
//! for [eBPF] programs. When an executable program's ownership is assigned to a
//! particular program loader then that loader will be used to run the program.
//! Program loaders are typically invoked by the CLI `solana deploy` command,
//! to deploy programs, and by the runtime to execute programs. Invoking a
//! program loader from another program will return an error.
//!
//! This crate defines the program IDs for most native programs: even though
//! some native programs cannot be invoked by other programs, a Solana program
//! may need to verify that certain accounts represent those native programs,
//! e.g. when verifying that the ed25519 precompile verified a signature.
//!
//! For many built-ins, this crate also defines enums that represent the
//! instructions they process, and constructors for building the instructions.
//! Some built-ins' instructions and constructors, though technically callable
//! from Solana programs, are not defined somewhere accessible to Solana
//! programs. This is a historical artifact due to lack of use cases for
//! invocation from other programs.
//!
//! Locations of program IDs and instruction constructors are noted in the lists
//! below.
//!
//! While some native programs have been active since the genesis block, others
//! are activated dynamically after a specific slot, and some are not yet
//! active. This documentation does not distinguish which native programs are
//! actually active on any particular network. The `solana feature status` CLI
//! command can help in determining active features.
//!
//! The native programs are:
//!
//! ## Built-ins
//!
//! - __System Program__: Creates new accounts, allocates account data, assigns
//!   accounts to owning programs, transfers lamports from System Program owned
//!   accounts and pays transaction fees.
//!   - ID: [`solana_program::system_program`]
//!   - Instruction: [`solana_program::system_instruction`]
//!
//! - __Stake Program__: Creates and manages accounts representing stake and
//!   rewards for delegations to validators.
//!   - ID: [`solana_program::stake::program`]
//!   - Instruction: [`solana_program::stake::instruction`]
//!
//! - __Vote Program__: Creates and manage accounts that track validator voting
//!   state and rewards.
//!   - ID: [`solana_program::vote::program`]
//!   - Instruction: [`solana_vote_program::vote_instruction`](https://docs.rs/solana-vote-program/latest/solana_vote_program/vote_instruction/index.html)
//!
//! - __Config Program__: Adds configuration data to the chain and the list of
//!   public keys that are permitted to modify it.
//!   - ID: [`solana_program::config::program`]
//!   - Instruction: [`solana_config_program::config_instruction`](https://docs.rs/solana-config-program/latest/solana_config_program/config_instruction/index.html)
//!
//! - __Address Lookup Table Program__: Support for [on-chain address lookup tables][lut].
//!   - ID: [`solana_address_lookup_table_program`](https://docs.rs/solana-address-lookup-table-program/latest/solana_address_lookup_table_program/)
//!   - Instruction: [`solana_address_lookup_table_program::instruction`](https://docs.rs/solana-address-lookup-table-program/latest/solana_address_lookup_table_program/instruction/index.html)
//!
//! [lut]: https://docs.solana.com/proposals/transactions-v2
//!
//! ## Special built-ins
//!
//! - __Compute Budget Program__: Requests additional CPU or memory resources
//!   for a transaction. This program does nothing when called from another
//!   program.
//!   - ID: [`solana_sdk::compute_budget`](https://docs.rs/solana-sdk/latest/solana_sdk/compute_budget/index.html)
//!   - Instruction: [`solana_sdk::compute_budget`](https://docs.rs/solana-sdk/latest/solana_sdk/compute_budget/index.html)
//!
//! ## Precompiles
//!
//! - __ed25519 Program__: Verifies an ed25519 signature.
//!   - ID: [`solana_program::ed25519_program`]
//!   - Instruction: [`solana_sdk::ed25519_instruction`](https://docs.rs/solana-sdk/latest/solana_sdk/ed25519_instruction/index.html)
//!
//! - __secp256k1 Program__: Verifies secp256k1 public key recovery operations.
//!   - ID: [`solana_program::secp256k1_program`]
//!   - Instruction: [`solana_sdk::secp256k1_instruction`](https://docs.rs/solana-sdk/latest/solana_sdk/secp256k1_instruction/index.html)
//!
//! ## Program loaders
//!
//! - __BPF Loader__: Deploys, and executes immutable programs on the chain.
//!   - ID: [`solana_program::bpf_loader`]
//!   - Instruction: [`solana_program::loader_instruction`]
//!
//! - __Upgradable BPF Loader__: Deploys, upgrades, and executes upgradable
//!   programs on the chain.
//!   - ID: [`solana_program::bpf_loader_upgradeable`]
//!   - Instruction: [`solana_program::loader_upgradeable_instruction`]
//!
//! - __Deprecated BPF Loader__: Deploys, and executes immutable programs on the
//!   chain.
//!   - ID: [`solana_program::bpf_loader_deprecated`]
//!   - Instruction: [`solana_program::loader_instruction`]
//!
//! # Sysvars
//!
//! Sysvars are special accounts that contain dynamically updating data about
//! the network cluster, the blockchain history, and the executing transaction.
//!
//! The program IDs and deserializers for sysvars are defined in the [`sysvar`]
//! module, and simple sysvars implement the [`Sysvar`] trait. Since to a Solana
//! program sysvars are just accounts, if the `AccountInfo` is provided to the
//! program, then the program can deserialize the sysvar with
//! [`Sysvar::from_account_info`] to access its data, as in this example that
//! logs the [`clock`][clk] sysvar.
//!
//! [`Sysvar`]: sysvar::Sysvar
//! [`Sysvar::from_account_info`]: sysvar::Sysvar::from_account_info
//! [clk]: sysvar::clock
//!
//! ```
//! use solana_program::{
//!     account_info::{next_account_info, AccountInfo},
//!     clock,
//!     entrypoint::ProgramResult,
//!     msg,
//!     pubkey::Pubkey,
//!     sysvar::Sysvar,
//! };
//!
//! fn process_instruction(
//!     program_id: &Pubkey,
//!     accounts: &[AccountInfo],
//!     instruction_data: &[u8],
//! ) -> ProgramResult {
//!     let account_info_iter = &mut accounts.iter();
//!     let clock_account = next_account_info(account_info_iter)?;
//!     let clock = clock::Clock::from_account_info(&clock_account)?;
//!     msg!("clock: {:#?}", clock);
//!     Ok(())
//! }
//! ```
//!
//! As an optimization, to avoid including sysvar accounts in instructions and deserializing them
//! in the program, some sysvars implement [`Sysvar::get`], which loads
//! a deserialized sysvar directly from the runtime, as in this
//! example, again using the `clock` sysvar:
//!
//! [`Sysvar::get`]: sysvar::Sysvar::get
//!
//! ```
//! use solana_program::{
//!     account_info::AccountInfo,
//!     clock,
//!     entrypoint::ProgramResult,
//!     msg,
//!     pubkey::Pubkey,
//!     sysvar::Sysvar,
//! };
//!
//! fn process_instruction(
//!     program_id: &Pubkey,
//!     accounts: &[AccountInfo],
//!     instruction_data: &[u8],
//! ) -> ProgramResult {
//!     let clock = clock::Clock::get()?;
//!     msg!("clock: {:#?}", clock);
//!     Ok(())
//! }
//! ```
//!
//! Some sysvars are too large to deserialize within a program, and
//! `Sysvar::from_account_info` returns an error. Some sysvars are too large
//! to deserialize within a program, and attempting to will exhaust the
//! program's compute budget. Some sysvars do not implement `Sysvar::get` and
//! return an error. Some sysvars have custom deserializers that do not
//! implement the `Sysvar` trait. These cases are documented in the modules for
//! individual sysvars.
//!
//! For more details see the Solana [documentation on sysvars][sysvardoc].
//!
//! [sysvardoc]: https://docs.solana.com/developing/runtime-facilities/sysvars
//!
//! # Limitations to the Solana version of the Rust standard library
//!
//! Solana programs, and the `solana-program` crate, are linked to the [Rust
//! standard library][std], which allows Solana programs greater compatibility
//! with the Rust crate ecosystem than would be possible if Solana programs only
//! had access to the [Rust core library][core]. The Solana version of the
//! standard library though is heavily modified in [Solana's Rust fork][fork].
//! Some APIs are re-defined to be non-functional, and some are removed
//! completely. The following is a partial list of Solana modifications to `std`.
//!
//! [std]: https://doc.rust-lang.org/stable/std/
//! [core]: https://doc.rust-lang.org/stable/core/
//! [fork]: https://github.com/solana-labs/rust
//!
//! - Standard I/O functions partially work:
//!   - [`Stdin`] has no inherent methods and its implementation of [`Read`] always
//!     successfully reads no bytes.
//!   - [`Stdout`] has no inherent methods and its [`Write`] implementation writes
//!     to the Solana program log. Users should use the [`msg!`] macro instead.
//!   - [`Stderr`] has no inherent methods and its [`Write`] implementation writes
//!     to the Solana program log. Users should use the [`msg!`] macro instead.
//! - [`HashMap`] works but is not seeded with a random number and will produce
//!   deterministic access patterns.
//! - All file and networking operations return an error.
//! - Most functions in [`std::env`] either panic, return an error, or return `None`.
//! - Most functions in [`std::process`] either panic, return an error, or return `None`.
//! - [`std::process::exit`] aborts &mdash; to exit successfully a Solana program
//!   must return 0 from its initial stack frame.
//! - Most functions in [`std::thread`] either panic, return an error, or return `None`.
//! - Synchronization types like [`Mutex`] and [`RwLock`] work, though Solana
//!   programs are single-threaded. Likewise for [atomics]. These types are only
//!   required for compatibility with other Rust crates.
//! - Waiting on an [`Condvar`] panics.
//! - The following functions are not defined:
//!   - [`std::alloc::set_alloc_error_hook`]
//!   - [`std::alloc::take_alloc_error_hook`]
//! - [`std::backtrace`] is not defined
//! - [`std::error::Error::backtrace`] is not defined.
//! - The BPF target does not support mutable static storage, so features
//!   typically used for mutating statics will not work in that context,
//!   including [`std::sync::atomic`], [`Lazy`] and [`OnceCell`]. Attempting to mutate
//!   static storage will result in an executable that cannot be deployed.
//! - [`std::panic::set_hook`] is a no-op. Custom panic handling in Solana
//!   uses the [`custom_panic_default!`] macro.
//! - The Solana runtime does not support thread local storage and using the
//!   [`thread_local!`] macro will result in an executable that cannot be
//!   deployed.
//! - The Solana runtime does not support traditional notions of time:
//!   - [`Instant::now`] always returns the zero instant.
//!   - [`SystemTime::now`] panics.
//!
//! [`Stdin`]: https://doc.rust-lang.org/std/io/struct.Stdin.html
//! [`Stdout`]: https://doc.rust-lang.org/std/io/struct.Stdout.html
//! [`Stderr`]: https://doc.rust-lang.org/std/io/struct.Stderr.html
//! [`Read`]: https://doc.rust-lang.org/std/io/trait.Read.html
//! [`Write`]: https://doc.rust-lang.org/std/io/trait.Write.html
//! [`HashMap`]: https://doc.rust-lang.org/std/collections/struct.HashMap.html
//! [`std::env`]: https://doc.rust-lang.org/std/env/index.html
//! [`std::process`]: https://doc.rust-lang.org/std/process/index.html
//! [`std::process::exit`]: https://doc.rust-lang.org/std/process/fn.exit.html
//! [`std::thread`]: https://doc.rust-lang.org/std/thread/index.html
//! [`Mutex`]: https://doc.rust-lang.org/std/sync/struct.Mutex.html
//! [`RwLock`]: https://doc.rust-lang.org/std/sync/struct.RwLock.html
//! [atomics]: https://doc.rust-lang.org/std/sync/atomic/index.html
//! [`Condvar`]: https://doc.rust-lang.org/std/sync/struct.Condvar.html
//! [`std::alloc::set_alloc_error_hook`]: https://doc.rust-lang.org/std/alloc/fn.set_alloc_error_hook.html
//! [`std::alloc::take_alloc_error_hook`]: https://doc.rust-lang.org/std/alloc/fn.take_alloc_error_hook.html
//! [`std::backtrace`]: https://doc.rust-lang.org/std/backtrace/index.html
//! [`std::error::Error::backtrace`]: https://doc.rust-lang.org/std/error/trait.Error.html#method.backtrace
//! [`std::sync::atomic`]: https://doc.rust-lang.org/std/sync/atomic/index.html
//! [`Lazy`]: https://doc.rust-lang.org/std/lazy/struct.Lazy.html
//! [`OnceCell`]: https://doc.rust-lang.org/std/lazy/struct.OnceCell.html
//! [`std::panic::set_hook`]: https://doc.rust-lang.org/std/panic/fn.set_hook.html
//! [`thread_local!`]: https://doc.rust-lang.org/std/macro.thread_local.html
//! [`Instant::now`]: https://doc.rust-lang.org/std/time/struct.Instant.html#method.now
//! [`SystemTime::now`]: https://doc.rust-lang.org/std/time/struct.SystemTime.html#method.now

#![allow(incomplete_features)]
#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(specialization))]
#![cfg_attr(RUSTC_NEEDS_PROC_MACRO_HYGIENE, feature(proc_macro_hygiene))]

// Allows macro expansion of `use ::solana_program::*` to work within this crate
extern crate self as solana_program;

pub mod account_info;
pub(crate) mod atomic_u64;
pub mod blake3;
pub mod borsh;
pub mod bpf_loader;
pub mod bpf_loader_deprecated;
pub mod bpf_loader_upgradeable;
pub mod clock;
pub mod debug_account_data;
pub mod decode_error;
pub mod ed25519_program;
pub mod entrypoint;
pub mod entrypoint_deprecated;
pub mod epoch_schedule;
pub mod example_mocks;
pub mod feature;
pub mod fee_calculator;
pub mod hash;
pub mod incinerator;
pub mod instruction;
pub mod keccak;
pub mod lamports;
pub mod loader_instruction;
pub mod loader_upgradeable_instruction;
pub mod log;
pub mod message;
pub mod native_token;
pub mod nonce;
pub mod program;
pub mod program_error;
pub mod program_memory;
pub mod program_option;
pub mod program_pack;
pub mod program_stubs;
pub mod program_utils;
pub mod pubkey;
pub mod rent;
pub mod sanitize;
pub mod secp256k1_program;
pub mod secp256k1_recover;
pub mod serialize_utils;
pub mod short_vec;
pub mod slot_hashes;
pub mod slot_history;
pub mod stake;
pub mod stake_history;
pub mod system_instruction;
pub mod system_program;
pub mod sysvar;
pub mod wasm;

#[cfg(target_arch = "bpf")]
pub use solana_sdk_macro::wasm_bindgen_stub as wasm_bindgen;
#[cfg(not(target_arch = "bpf"))]
pub use wasm_bindgen::prelude::wasm_bindgen;

pub mod config {
    pub mod program {
        crate::declare_id!("Config1111111111111111111111111111111111111");
    }
}

pub mod vote {
    pub mod program {
        crate::declare_id!("Vote111111111111111111111111111111111111111");
    }
}

/// Same as `declare_id` except report that this id has been deprecated
pub use solana_sdk_macro::program_declare_deprecated_id as declare_deprecated_id;
/// Convenience macro to declare a static public key and functions to interact with it
///
/// Input: a single literal base58 string representation of a program's id
///
/// # Example
///
/// ```
/// # // wrapper is used so that the macro invocation occurs in the item position
/// # // rather than in the statement position which isn't allowed.
/// use std::str::FromStr;
/// use solana_program::{declare_id, pubkey::Pubkey};
///
/// # mod item_wrapper {
/// #   use solana_program::declare_id;
/// declare_id!("My11111111111111111111111111111111111111111");
/// # }
/// # use item_wrapper::id;
///
/// let my_id = Pubkey::from_str("My11111111111111111111111111111111111111111").unwrap();
/// assert_eq!(id(), my_id);
/// ```
pub use solana_sdk_macro::program_declare_id as declare_id;
/// Convenience macro to define a static public key
///
/// Input: a single literal base58 string representation of a Pubkey
///
/// # Example
///
/// ```
/// use std::str::FromStr;
/// use solana_program::{pubkey, pubkey::Pubkey};
///
/// static ID: Pubkey = pubkey!("My11111111111111111111111111111111111111111");
///
/// let my_id = Pubkey::from_str("My11111111111111111111111111111111111111111").unwrap();
/// assert_eq!(ID, my_id);
/// ```
pub use solana_sdk_macro::program_pubkey as pubkey;

#[macro_use]
extern crate serde_derive;

#[macro_use]
extern crate solana_frozen_abi_macro;

/// Convenience macro for doing integer division where the opersation's safety
/// can be checked at compile-time
///
/// Since `unchecked_div_by_const!()` is supposed to fail at compile-time, abuse
/// doctests to cover failure modes
/// Literal denominator div-by-zero fails
/// ```compile_fail
/// # use solana_program::unchecked_div_by_const;
/// # fn main() {
/// # let _ = unchecked_div_by_const!(10, 0);
/// # }
/// ```
/// #
/// # Const denominator div-by-zero fails
/// ```compile_fail
/// # use solana_program::unchecked_div_by_const;
/// # fn main() {
/// # const D: u64 = 0;
/// # let _ = unchecked_div_by_const!(10, D);
/// # }
/// ```
/// #
/// # Non-const denominator fails
/// ```compile_fail
/// # use solana_program::unchecked_div_by_const;
/// # fn main() {
/// # let d = 0;
/// # let _ = unchecked_div_by_const!(10, d);
/// # }
/// ```
/// #
/// Literal denominator div-by-zero fails
/// ```compile_fail
/// # use solana_program::unchecked_div_by_const;
/// # fn main() {
/// # const N: u64 = 10;
/// # let _ = unchecked_div_by_const!(N, 0);
/// # }
/// ```
/// #
/// # Const denominator div-by-zero fails
/// ```compile_fail
/// # use solana_program::unchecked_div_by_const;
/// # fn main() {
/// # const N: u64 = 10;
/// # const D: u64 = 0;
/// # let _ = unchecked_div_by_const!(N, D);
/// # }
/// ```
/// #
/// # Non-const denominator fails
/// ```compile_fail
/// # use solana_program::unchecked_div_by_const;
/// # fn main() {
/// # const N: u64 = 10;
/// # let d = 0;
/// # let _ = unchecked_div_by_const!(N, d);
/// # }
/// ```
/// #
/// Literal denominator div-by-zero fails
/// ```compile_fail
/// # use solana_program::unchecked_div_by_const;
/// # fn main() {
/// # let n = 10;
/// # let _ = unchecked_div_by_const!(n, 0);
/// # }
/// ```
/// #
/// # Const denominator div-by-zero fails
/// ```compile_fail
/// # use solana_program::unchecked_div_by_const;
/// # fn main() {
/// # let n = 10;
/// # const D: u64 = 0;
/// # let _ = unchecked_div_by_const!(n, D);
/// # }
/// ```
/// #
/// # Non-const denominator fails
/// ```compile_fail
/// # use solana_program::unchecked_div_by_const;
/// # fn main() {
/// # let n = 10;
/// # let d = 0;
/// # let _ = unchecked_div_by_const!(n, d);
/// # }
/// ```
/// #
#[macro_export]
macro_rules! unchecked_div_by_const {
    ($num:expr, $den:expr) => {{
        // Ensure the denominator is compile-time constant
        let _ = [(); ($den - $den) as usize];
        // Compile-time constant integer div-by-zero passes for some reason
        // when invoked from a compilation unit other than that where this
        // macro is defined. Do an explicit zero-check for now. Sorry about the
        // ugly error messages!
        // https://users.rust-lang.org/t/unexpected-behavior-of-compile-time-integer-div-by-zero-check-in-declarative-macro/56718
        let _ = [(); ($den as usize) - 1];
        #[allow(clippy::integer_arithmetic)]
        let quotient = $num / $den;
        quotient
    }};
}

#[cfg(test)]
mod tests {
    use super::unchecked_div_by_const;

    #[test]
    fn test_unchecked_div_by_const() {
        const D: u64 = 2;
        const N: u64 = 10;
        let n = 10;
        assert_eq!(unchecked_div_by_const!(10, 2), 5);
        assert_eq!(unchecked_div_by_const!(N, 2), 5);
        assert_eq!(unchecked_div_by_const!(n, 2), 5);
        assert_eq!(unchecked_div_by_const!(10, D), 5);
        assert_eq!(unchecked_div_by_const!(N, D), 5);
        assert_eq!(unchecked_div_by_const!(n, D), 5);
    }
}
