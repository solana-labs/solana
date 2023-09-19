//! Instructions for the [secp256k1 native program][np].
//!
//! [np]: https://docs.solana.com/developing/runtime-facilities/programs#secp256k1-program
//!
//! _This module provides low-level cryptographic building blocks that must be
//! used carefully to ensure proper security. Read this documentation and
//! accompanying links thoroughly._
//!
//! The secp26k1 native program peforms flexible verification of [secp256k1]
//! ECDSA signatures, as used by Ethereum. It can verify up to 255 signatures on
//! up to 255 messages, with those signatures, messages, and their public keys
//! arbitrarily distributed across the instruction data of any instructions in
//! the same transaction as the secp256k1 instruction.
//!
//! The secp256k1 native program ID is located in the [`secp256k1_program`] module.
//!
//! The instruction is designed for Ethereum interoperability, but may be useful
//! for other purposes. It operates on Ethereum addresses, which are [`keccak`]
//! hashes of secp256k1 public keys, and internally is implemented using the
//! secp256k1 key recovery algorithm. Ethereum address can be created for
//! secp256k1 public keys with the [`construct_eth_pubkey`] function.
//!
//! [`keccak`]: crate::keccak
//!
//! This instruction does not directly allow for key recovery as in Ethereum's
//! [`ecrecover`] precompile. For that Solana provides the [`secp256k1_recover`]
//! syscall.
//!
//! [secp256k1]: https://en.bitcoin.it/wiki/Secp256k1
//! [`secp256k1_program`]: solana_program::secp256k1_program
//! [`secp256k1_recover`]: solana_program::secp256k1_recover
//! [`ecrecover`]: https://docs.soliditylang.org/en/v0.8.14/units-and-global-variables.html?highlight=ecrecover#mathematical-and-cryptographic-functions
//!
//! Use cases for the secp256k1 instruction include:
//!
//! - Verifying Ethereum transaction signatures.
//! - Verifying Ethereum [EIP-712] signatures.
//! - Verifying arbitrary secp256k1 signatures.
//! - Signing a single message with multiple signatures.
//!
//! [EIP-712]: https://eips.ethereum.org/EIPS/eip-712
//!
//! The [`new_secp256k1_instruction`] function is suitable for building a
//! secp256k1 program instruction for basic use cases were a single message must
//! be signed by a known secret key. For other uses cases, including many
//! Ethereum-integration use cases, construction of the secp256k1 instruction
//! must be done manually.
//!
//! # How to use this program
//!
//! Transactions that uses the secp256k1 native program will typically include
//! at least two instructions: one for the secp256k1 program to verify the
//! signatures, and one for a custom program that will check that the secp256k1
//! instruction data matches what the program expects (using
//! [`load_instruction_at_checked`] or [`get_instruction_relative`]). The
//! signatures, messages, and Ethereum addresses being verified may reside in the
//! instruction data of either of these instructions, or in the instruction data
//! of one or more additional instructions, as long as those instructions are in
//! the same transaction.
//!
//! [`load_instruction_at_checked`]: crate::sysvar::instructions::load_instruction_at_checked
//! [`get_instruction_relative`]: crate::sysvar::instructions::get_instruction_relative
//!
//! Correct use of this program involves multiple steps, in client code and
//! program code:
//!
//! - In the client:
//!   - Sign the [`keccak`]-hashed messages with a secp256k1 ECDSA library,
//!     like the [`libsecp256k1`] crate.
//!   - Build any custom instruction data that contain signature, message, or
//!     Ethereum address data that will be used by the secp256k1 instruction.
//!   - Build the secp256k1 program instruction data, specifying the number of
//!     signatures to verify, the instruction indexes within the transaction,
//!     and offsets within those instruction's data, where the signatures,
//!     messages, and Ethereum addresses are located.
//!   - Build the custom instruction for the program that will check the results
//!     of the secp256k1 native program.
//!   - Package all instructions into a single transaction and submit them.
//! - In the program:
//!   - Load the secp256k1 instruction data with
//!     [`load_instruction_at_checked`]. or [`get_instruction_relative`].
//!   - Check that the secp256k1 program ID is equal to
//!     [`secp256k1_program::ID`], so that the signature verification cannot be
//!     faked with a malicious program.
//!   - Check that the public keys and messages are the expected values per
//!     the program's requirements.
//!
//! [`secp256k1_program::ID`]: crate::secp256k1_program::ID
//!
//! The signature, message, or Ethereum addresses may reside in the secp256k1
//! instruction data itself as additional data, their bytes following the bytes
//! of the protocol required by the secp256k1 instruction to locate the
//! signature, message, and Ethereum address data. This is the technique used by
//! `new_secp256k1_instruction` for simple signature verification.
//!
//! The `solana_sdk` crate provides few APIs for building the instructions and
//! transactions necessary for properly using the secp256k1 native program.
//! Many steps must be done manually.
//!
//! The `solana_program` crate provides no APIs to assist in interpreting
//! the the secp256k1 instruction data. It must be done manually.
//!
//! The secp256k1 program is implemented with the [`libsecp256k1`] crate,
//! which clients may also want to use.
//!
//! [`libsecp256k1`]: https://docs.rs/libsecp256k1/latest/libsecp256k1
//!
//! # Layout and interpretation of the secp256k1 instruction data
//!
//! The secp256k1 instruction data contains:
//!
//! - 1 byte indicating the number of signatures to verify, 0 - 255,
//! - A number of _signature offset_ structures that indicate where in the
//!   transaction to locate each signature, message, and Ethereum address.
//! - 0 or more bytes of arbitrary data, which may contain signatures,
//!   messages or Ethereum addresses.
//!
//! The signature offset structure is defined by [`SecpSignatureOffsets`],
//! and can be serialized to the correct format with [`bincode::serialize_into`].
//! Note that the bincode format may not be stable,
//! and callers should ensure they use the same version of `bincode` as the Solana SDK.
//! This data structure is not provided to Solana programs,
//! which are expected to interpret the signature offsets manually.
//!
//! [`bincode::serialize_into`]: https://docs.rs/bincode/1.3.3/bincode/fn.serialize_into.html
//!
//! The serialized signature offset structure has the following 11-byte layout,
//! with data types in little-endian encoding.
//!
//! | index  | bytes | type  | description |
//! |--------|-------|-------|-------------|
//! | 0      | 2     | `u16` | `signature_offset` - offset to 64-byte signature plus 1-byte recovery ID. |
//! | 2      | 1     | `u8`  | `signature_offset_instruction_index` - within the transaction, the index of the transaction whose instruction data contains the signature. |
//! | 3      | 2     | `u16` | `eth_address_offset` - offset to 20-byte Ethereum address. |
//! | 5      | 1     | `u8`  | `eth_address_instruction_index` - within the transaction, the index of the instruction whose instruction data contains the Ethereum address. |
//! | 6      | 2     | `u16` | `message_data_offset` - Offset to start of message data. |
//! | 8      | 2     | `u16` | `message_data_size` - Size of message data in bytes. |
//! | 10     | 1     | `u8`  | `message_instruction_index` - Within the transaction, the index of the instruction whose instruction data contains the message data. |
//!
//! # Signature malleability
//!
//! With the ECDSA signature algorithm it is possible for any party, given a
//! valid signature of some message, to create a second signature that is
//! equally valid. This is known as _signature malleability_. In many cases this
//! is not a concern, but in cases where applications rely on signatures to have
//! a unique representation this can be the source of bugs, potentially with
//! security implications.
//!
//! **The solana `secp256k1_recover` function does not prevent signature
//! malleability**. This is in contrast to the Bitcoin secp256k1 library, which
//! does prevent malleability by default. Solana accepts signatures with `S`
//! values that are either in the _high order_ or in the _low order_, and it
//! is trivial to produce one from the other.
//!
//! For more complete documentation of the subject, and techniques to prevent
//! malleability, see the documentation for the [`secp256k1_recover`] syscall.
//!
//! # Additional security considerations
//!
//! Most programs will want to be conservative about the layout of the secp256k1 instruction
//! to prevent unforeseen bugs. The following checks may be desirable:
//!
//! - That there are exactly the expected number of signatures.
//! - That the three indexes, `signature_offset_instruction_index`,
//!   `eth_address_instruction_index`, and `message_instruction_index` are as
//!   expected, placing the signature, message and Ethereum address in the
//!   expected instruction.
//!
//! Loading the secp256k1 instruction data within a program requires access to
//! the [instructions sysvar][is], which must be passed to the program by its
//! caller. Programs must verify the ID of this program to avoid calling an
//! imposter program. This does not need to be done manually though, as long as
//! it is only used through the [`load_instruction_at_checked`] or
//! [`get_instruction_relative`] functions. Both of these functions check their
//! sysvar argument to ensure it is the known instruction sysvar.
//!
//! [is]: crate::sysvar::instructions
//!
//! Programs should _always_ verify that the secp256k1 program ID loaded through
//! the instructions sysvar has the same value as in the [`secp256k1_program`]
//! module. Again this prevents imposter programs.
//!
//! [`secp256k1_program`]: crate::secp256k1_program
//!
//! # Errors
//!
//! The transaction will fail if any of the following are true:
//!
//! - Any signature was not created by the secret key corresponding to the
//!   specified public key.
//! - Any signature is invalid.
//! - Any signature is "overflowing", a non-standard condition.
//! - The instruction data is empty.
//! - The first byte of instruction data is equal to 0 (indicating no signatures),
//!   but the instruction data's length is greater than 1.
//! - The instruction data is not long enough to hold the number of signature
//!   offsets specified in the first byte.
//! - Any instruction indexes specified in the signature offsets are greater or
//!   equal to the number of instructions in the transaction.
//! - Any bounds specified in the signature offsets exceed the bounds of the
//!   instruction data to which they are indexed.
//!
//! # Examples
//!
//! Both of the following examples make use of the following module definition
//! to parse the secp256k1 instruction data from within a Solana program.
//!
//! ```no_run
//! mod secp256k1_defs {
//!     use solana_program::program_error::ProgramError;
//!     use std::iter::Iterator;
//!
//!     pub const HASHED_PUBKEY_SERIALIZED_SIZE: usize = 20;
//!     pub const SIGNATURE_SERIALIZED_SIZE: usize = 64;
//!     pub const SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 11;
//!
//!     /// The structure encoded in the secp2256k1 instruction data.
//!     pub struct SecpSignatureOffsets {
//!         pub signature_offset: u16,
//!         pub signature_instruction_index: u8,
//!         pub eth_address_offset: u16,
//!         pub eth_address_instruction_index: u8,
//!         pub message_data_offset: u16,
//!         pub message_data_size: u16,
//!         pub message_instruction_index: u8,
//!     }
//!
//!     pub fn iter_signature_offsets(
//!        secp256k1_instr_data: &[u8],
//!     ) -> Result<impl Iterator<Item = SecpSignatureOffsets> + '_, ProgramError> {
//!         // First element is the number of `SecpSignatureOffsets`.
//!         let num_structs = *secp256k1_instr_data
//!             .get(0)
//!             .ok_or(ProgramError::InvalidArgument)?;
//!
//!         let all_structs_size = SIGNATURE_OFFSETS_SERIALIZED_SIZE * num_structs as usize;
//!         let all_structs_slice = secp256k1_instr_data
//!             .get(1..all_structs_size + 1)
//!             .ok_or(ProgramError::InvalidArgument)?;
//!
//!         fn decode_u16(chunk: &[u8], index: usize) -> u16 {
//!             u16::from_le_bytes(<[u8; 2]>::try_from(&chunk[index..index + 2]).unwrap())
//!         }
//!
//!         Ok(all_structs_slice
//!             .chunks(SIGNATURE_OFFSETS_SERIALIZED_SIZE)
//!             .map(|chunk| SecpSignatureOffsets {
//!                 signature_offset: decode_u16(chunk, 0),
//!                 signature_instruction_index: chunk[2],
//!                 eth_address_offset: decode_u16(chunk, 3),
//!                 eth_address_instruction_index: chunk[5],
//!                 message_data_offset: decode_u16(chunk, 6),
//!                 message_data_size: decode_u16(chunk, 8),
//!                 message_instruction_index: chunk[10],
//!             }))
//!     }
//! }
//! ```
//!
//! ## Example: Signing and verifying with `new_secp256k1_instruction`
//!
//! This example demonstrates the simplest way to use the secp256k1 program, by
//! calling [`new_secp256k1_instruction`] to sign a single message and build the
//! corresponding secp256k1 instruction.
//!
//! This example has two components: a Solana program, and an RPC client that
//! sends a transaction to call it. The RPC client will sign a single message,
//! and the Solana program will introspect the secp256k1 instruction to verify
//! that the signer matches a known authorized public key.
//!
//! The Solana program. Note that it uses `libsecp256k1` version 0.7.0 to parse
//! the secp256k1 signature to prevent malleability.
//!
//! ```no_run
//! # mod secp256k1_defs {
//! #     use solana_program::program_error::ProgramError;
//! #     use std::iter::Iterator;
//! #
//! #     pub const HASHED_PUBKEY_SERIALIZED_SIZE: usize = 20;
//! #     pub const SIGNATURE_SERIALIZED_SIZE: usize = 64;
//! #     pub const SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 11;
//! #
//! #     /// The structure encoded in the secp2256k1 instruction data.
//! #     pub struct SecpSignatureOffsets {
//! #         pub signature_offset: u16,
//! #         pub signature_instruction_index: u8,
//! #         pub eth_address_offset: u16,
//! #         pub eth_address_instruction_index: u8,
//! #         pub message_data_offset: u16,
//! #         pub message_data_size: u16,
//! #         pub message_instruction_index: u8,
//! #     }
//! #
//! #     pub fn iter_signature_offsets(
//! #        secp256k1_instr_data: &[u8],
//! #     ) -> Result<impl Iterator<Item = SecpSignatureOffsets> + '_, ProgramError> {
//! #         // First element is the number of `SecpSignatureOffsets`.
//! #         let num_structs = *secp256k1_instr_data
//! #             .get(0)
//! #             .ok_or(ProgramError::InvalidArgument)?;
//! #
//! #         let all_structs_size = SIGNATURE_OFFSETS_SERIALIZED_SIZE * num_structs as usize;
//! #         let all_structs_slice = secp256k1_instr_data
//! #             .get(1..all_structs_size + 1)
//! #             .ok_or(ProgramError::InvalidArgument)?;
//! #
//! #         fn decode_u16(chunk: &[u8], index: usize) -> u16 {
//! #             u16::from_le_bytes(<[u8; 2]>::try_from(&chunk[index..index + 2]).unwrap())
//! #         }
//! #
//! #         Ok(all_structs_slice
//! #             .chunks(SIGNATURE_OFFSETS_SERIALIZED_SIZE)
//! #             .map(|chunk| SecpSignatureOffsets {
//! #                 signature_offset: decode_u16(chunk, 0),
//! #                 signature_instruction_index: chunk[2],
//! #                 eth_address_offset: decode_u16(chunk, 3),
//! #                 eth_address_instruction_index: chunk[5],
//! #                 message_data_offset: decode_u16(chunk, 6),
//! #                 message_data_size: decode_u16(chunk, 8),
//! #                 message_instruction_index: chunk[10],
//! #             }))
//! #     }
//! # }
//! use solana_program::{
//!     account_info::{next_account_info, AccountInfo},
//!     entrypoint::ProgramResult,
//!     msg,
//!     program_error::ProgramError,
//!     secp256k1_program,
//!     sysvar,
//! };
//!
//! /// An Ethereum address corresponding to a secp256k1 secret key that is
//! /// authorized to sign our messages.
//! const AUTHORIZED_ETH_ADDRESS: [u8; 20] = [
//!     0x18, 0x8a, 0x5c, 0xf2, 0x3b, 0x0e, 0xff, 0xe9, 0xa8, 0xe1, 0x42, 0x64, 0x5b, 0x82, 0x2f, 0x3a,
//!     0x6b, 0x8b, 0x52, 0x35,
//! ];
//!
//! /// Check the secp256k1 instruction to ensure it was signed by
//! /// `AUTHORIZED_ETH_ADDRESS`s key.
//! ///
//! /// `accounts` is the slice of all accounts passed to the program
//! /// entrypoint. The only account it should contain is the instructions sysvar.
//! fn demo_secp256k1_verify_basic(
//!    accounts: &[AccountInfo],
//! ) -> ProgramResult {
//!     let account_info_iter = &mut accounts.iter();
//!
//!     // The instructions sysvar gives access to the instructions in the transaction.
//!     let instructions_sysvar_account = next_account_info(account_info_iter)?;
//!     assert!(sysvar::instructions::check_id(
//!         instructions_sysvar_account.key
//!     ));
//!
//!     // Load the secp256k1 instruction.
//!     // `new_secp256k1_instruction` generates an instruction that must be at index 0.
//!     let secp256k1_instr =
//!         sysvar::instructions::load_instruction_at_checked(0, instructions_sysvar_account)?;
//!
//!     // Verify it is a secp256k1 instruction.
//!     // This is security-critical - what if the transaction uses an imposter secp256k1 program?
//!     assert!(secp256k1_program::check_id(&secp256k1_instr.program_id));
//!
//!     // There must be at least one byte. This is also verified by the runtime,
//!     // and doesn't strictly need to be checked.
//!     assert!(secp256k1_instr.data.len() > 1);
//!
//!     let num_signatures = secp256k1_instr.data[0];
//!     // `new_secp256k1_instruction` generates an instruction that contains one signature.
//!     assert_eq!(1, num_signatures);
//!
//!     // Load the first and only set of signature offsets.
//!     let offsets: secp256k1_defs::SecpSignatureOffsets =
//!         secp256k1_defs::iter_signature_offsets(&secp256k1_instr.data)?
//!             .next()
//!             .ok_or(ProgramError::InvalidArgument)?;
//!
//!     // `new_secp256k1_instruction` generates an instruction that only uses instruction index 0.
//!     assert_eq!(0, offsets.signature_instruction_index);
//!     assert_eq!(0, offsets.eth_address_instruction_index);
//!     assert_eq!(0, offsets.message_instruction_index);
//!
//!     // Reject high-s value signatures to prevent malleability.
//!     // Solana does not do this itself.
//!     // This may or may not be necessary depending on use case.
//!     {
//!         let signature = &secp256k1_instr.data[offsets.signature_offset as usize
//!             ..offsets.signature_offset as usize + secp256k1_defs::SIGNATURE_SERIALIZED_SIZE];
//!         let signature = libsecp256k1::Signature::parse_standard_slice(signature)
//!             .map_err(|_| ProgramError::InvalidArgument)?;
//!
//!         if signature.s.is_high() {
//!             msg!("signature with high-s value");
//!             return Err(ProgramError::InvalidArgument);
//!         }
//!     }
//!
//!     // There is likely at least one more verification step a real program needs
//!     // to do here to ensure it trusts the secp256k1 instruction, e.g.:
//!     //
//!     // - verify the tx signer is authorized
//!     // - verify the secp256k1 signer is authorized
//!
//!     // Here we are checking the secp256k1 pubkey against a known authorized pubkey.
//!     let eth_address = &secp256k1_instr.data[offsets.eth_address_offset as usize
//!         ..offsets.eth_address_offset as usize + secp256k1_defs::HASHED_PUBKEY_SERIALIZED_SIZE];
//!
//!     if eth_address != AUTHORIZED_ETH_ADDRESS {
//!         return Err(ProgramError::InvalidArgument);
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! The client program:
//!
//! ```no_run
//! # use solana_sdk::example_mocks::solana_rpc_client;
//! use anyhow::Result;
//! use solana_rpc_client::rpc_client::RpcClient;
//! use solana_sdk::{
//!     instruction::{AccountMeta, Instruction},
//!     secp256k1_instruction,
//!     signature::{Keypair, Signer},
//!     sysvar,
//!     transaction::Transaction,
//! };
//!
//! fn demo_secp256k1_verify_basic(
//!     payer_keypair: &Keypair,
//!     secp256k1_secret_key: &libsecp256k1::SecretKey,
//!     client: &RpcClient,
//!     program_keypair: &Keypair,
//! ) -> Result<()> {
//!     // Internally to `new_secp256k1_instruction` and
//!     // `secp256k_instruction::verify` (the secp256k1 program), this message is
//!     // keccak-hashed before signing.
//!     let msg = b"hello world";
//!     let secp256k1_instr = secp256k1_instruction::new_secp256k1_instruction(&secp256k1_secret_key, msg);
//!
//!     let program_instr = Instruction::new_with_bytes(
//!         program_keypair.pubkey(),
//!         &[],
//!         vec![
//!             AccountMeta::new_readonly(sysvar::instructions::ID, false)
//!         ],
//!     );
//!
//!     let blockhash = client.get_latest_blockhash()?;
//!     let tx = Transaction::new_signed_with_payer(
//!         &[secp256k1_instr, program_instr],
//!         Some(&payer_keypair.pubkey()),
//!         &[payer_keypair],
//!         blockhash,
//!     );
//!
//!     client.send_and_confirm_transaction(&tx)?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Example: Verifying multiple signatures in one instruction
//!
//! This examples demonstrates manually creating a secp256k1 instruction
//! containing many signatures, and a Solana program that parses them all. This
//! example on its own has no practical purpose. It simply demonstrates advanced
//! use of the secp256k1 program.
//!
//! Recall that the secp256k1 program will accept signatures, messages, and
//! Ethereum addresses that reside in any instruction contained in the same
//! transaction. In the _previous_ example, the Solana program asserted that all
//! signatures, messages, and addresses were stored in the instruction at 0. In
//! this next example the Solana program supports signatures, messages, and
//! addresses stored in any instruction. For simplicity the client still only
//! stores signatures, messages, and addresses in a single instruction, the
//! secp256k1 instruction. The code for storing this data across multiple
//! instructions would be complex, and may not be necessary in practice.
//!
//! This example has two components: a Solana program, and an RPC client that
//! sends a transaction to call it.
//!
//! The Solana program:
//!
//! ```no_run
//! # mod secp256k1_defs {
//! #     use solana_program::program_error::ProgramError;
//! #     use std::iter::Iterator;
//! #
//! #     pub const HASHED_PUBKEY_SERIALIZED_SIZE: usize = 20;
//! #     pub const SIGNATURE_SERIALIZED_SIZE: usize = 64;
//! #     pub const SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 11;
//! #
//! #     /// The structure encoded in the secp2256k1 instruction data.
//! #     pub struct SecpSignatureOffsets {
//! #         pub signature_offset: u16,
//! #         pub signature_instruction_index: u8,
//! #         pub eth_address_offset: u16,
//! #         pub eth_address_instruction_index: u8,
//! #         pub message_data_offset: u16,
//! #         pub message_data_size: u16,
//! #         pub message_instruction_index: u8,
//! #     }
//! #
//! #     pub fn iter_signature_offsets(
//! #        secp256k1_instr_data: &[u8],
//! #     ) -> Result<impl Iterator<Item = SecpSignatureOffsets> + '_, ProgramError> {
//! #         // First element is the number of `SecpSignatureOffsets`.
//! #         let num_structs = *secp256k1_instr_data
//! #             .get(0)
//! #             .ok_or(ProgramError::InvalidArgument)?;
//! #
//! #         let all_structs_size = SIGNATURE_OFFSETS_SERIALIZED_SIZE * num_structs as usize;
//! #         let all_structs_slice = secp256k1_instr_data
//! #             .get(1..all_structs_size + 1)
//! #             .ok_or(ProgramError::InvalidArgument)?;
//! #
//! #         fn decode_u16(chunk: &[u8], index: usize) -> u16 {
//! #             u16::from_le_bytes(<[u8; 2]>::try_from(&chunk[index..index + 2]).unwrap())
//! #         }
//! #
//! #         Ok(all_structs_slice
//! #             .chunks(SIGNATURE_OFFSETS_SERIALIZED_SIZE)
//! #             .map(|chunk| SecpSignatureOffsets {
//! #                 signature_offset: decode_u16(chunk, 0),
//! #                 signature_instruction_index: chunk[2],
//! #                 eth_address_offset: decode_u16(chunk, 3),
//! #                 eth_address_instruction_index: chunk[5],
//! #                 message_data_offset: decode_u16(chunk, 6),
//! #                 message_data_size: decode_u16(chunk, 8),
//! #                 message_instruction_index: chunk[10],
//! #             }))
//! #     }
//! # }
//! use solana_program::{
//!     account_info::{next_account_info, AccountInfo},
//!     entrypoint::ProgramResult,
//!     msg,
//!     program_error::ProgramError,
//!     secp256k1_program,
//!     sysvar,
//! };
//!
//! /// A struct to hold the values specified in the `SecpSignatureOffsets` struct.
//! struct SecpSignature {
//!     signature: [u8; secp256k1_defs::SIGNATURE_SERIALIZED_SIZE],
//!     recovery_id: u8,
//!     eth_address: [u8; secp256k1_defs::HASHED_PUBKEY_SERIALIZED_SIZE],
//!     message: Vec<u8>,
//! }
//!
//! /// Load all signatures indicated in the secp256k1 instruction.
//! ///
//! /// This function is quite inefficient for reloading the same instructions
//! /// repeatedly and making copies and allocations.
//! fn load_signatures(
//!     secp256k1_instr_data: &[u8],
//!     instructions_sysvar_account: &AccountInfo,
//! ) -> Result<Vec<SecpSignature>, ProgramError> {
//!     let mut sigs = vec![];
//!     for offsets in secp256k1_defs::iter_signature_offsets(secp256k1_instr_data)? {
//!         let signature_instr = sysvar::instructions::load_instruction_at_checked(
//!             offsets.signature_instruction_index as usize,
//!             instructions_sysvar_account,
//!         )?;
//!         let eth_address_instr = sysvar::instructions::load_instruction_at_checked(
//!             offsets.eth_address_instruction_index as usize,
//!             instructions_sysvar_account,
//!         )?;
//!         let message_instr = sysvar::instructions::load_instruction_at_checked(
//!             offsets.message_instruction_index as usize,
//!             instructions_sysvar_account,
//!         )?;
//!
//!         // These indexes must all be valid because the runtime already verified them.
//!         let signature = &signature_instr.data[offsets.signature_offset as usize
//!             ..offsets.signature_offset as usize + secp256k1_defs::SIGNATURE_SERIALIZED_SIZE];
//!         let recovery_id = signature_instr.data
//!             [offsets.signature_offset as usize + secp256k1_defs::SIGNATURE_SERIALIZED_SIZE];
//!         let eth_address = &eth_address_instr.data[offsets.eth_address_offset as usize
//!             ..offsets.eth_address_offset as usize + secp256k1_defs::HASHED_PUBKEY_SERIALIZED_SIZE];
//!         let message = &message_instr.data[offsets.message_data_offset as usize
//!             ..offsets.message_data_offset as usize + offsets.message_data_size as usize];
//!
//!         let signature =
//!             <[u8; secp256k1_defs::SIGNATURE_SERIALIZED_SIZE]>::try_from(signature).unwrap();
//!         let eth_address =
//!             <[u8; secp256k1_defs::HASHED_PUBKEY_SERIALIZED_SIZE]>::try_from(eth_address).unwrap();
//!         let message = Vec::from(message);
//!
//!         sigs.push(SecpSignature {
//!             signature,
//!             recovery_id,
//!             eth_address,
//!             message,
//!         })
//!     }
//!     Ok(sigs)
//! }
//!
//! fn demo_secp256k1_custom_many(
//!     accounts: &[AccountInfo],
//! ) -> ProgramResult {
//!     let account_info_iter = &mut accounts.iter();
//!
//!     let instructions_sysvar_account = next_account_info(account_info_iter)?;
//!     assert!(sysvar::instructions::check_id(
//!         instructions_sysvar_account.key
//!     ));
//!
//!     let secp256k1_instr =
//!         sysvar::instructions::get_instruction_relative(-1, instructions_sysvar_account)?;
//!
//!     assert!(secp256k1_program::check_id(&secp256k1_instr.program_id));
//!
//!     let signatures = load_signatures(&secp256k1_instr.data, instructions_sysvar_account)?;
//!     for (idx, signature_bundle) in signatures.iter().enumerate() {
//!         let signature = hex::encode(&signature_bundle.signature);
//!         let eth_address = hex::encode(&signature_bundle.eth_address);
//!         let message = hex::encode(&signature_bundle.message);
//!         msg!("sig {}: {:?}", idx, signature);
//!         msg!("recid: {}: {}", idx, signature_bundle.recovery_id);
//!         msg!("eth address {}: {}", idx, eth_address);
//!         msg!("message {}: {}", idx, message);
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! The client program:
//!
//! ```no_run
//! # use solana_sdk::example_mocks::solana_rpc_client;
//! use anyhow::Result;
//! use solana_rpc_client::rpc_client::RpcClient;
//! use solana_sdk::{
//!     instruction::{AccountMeta, Instruction},
//!     keccak,
//!     secp256k1_instruction::{
//!         self, SecpSignatureOffsets, HASHED_PUBKEY_SERIALIZED_SIZE,
//!         SIGNATURE_OFFSETS_SERIALIZED_SIZE, SIGNATURE_SERIALIZED_SIZE,
//!     },
//!     signature::{Keypair, Signer},
//!     sysvar,
//!     transaction::Transaction,
//! };
//!
//! /// A struct to hold the values specified in the `SecpSignatureOffsets` struct.
//! struct SecpSignature {
//!     signature: [u8; SIGNATURE_SERIALIZED_SIZE],
//!     recovery_id: u8,
//!     eth_address: [u8; HASHED_PUBKEY_SERIALIZED_SIZE],
//!     message: Vec<u8>,
//! }
//!
//! /// Create the instruction data for a secp256k1 instruction.
//! ///
//! /// `instruction_index` is the index the secp256k1 instruction will appear
//! /// within the transaction. For simplicity, this function only supports packing
//! /// the signatures into the secp256k1 instruction data, and not into any other
//! /// instructions within the transaction.
//! fn make_secp256k1_instruction_data(
//!     signatures: &[SecpSignature],
//!     instruction_index: u8,
//! ) -> Result<Vec<u8>> {
//!     assert!(signatures.len() <= u8::max_value().into());
//!
//!     // We're going to pack all the signatures into the secp256k1 instruction data.
//!     // Before our signatures though is the signature offset structures
//!     // the secp256k1 program parses to find those signatures.
//!     // This value represents the byte offset where the signatures begin.
//!     let data_start = 1 + signatures.len() * SIGNATURE_OFFSETS_SERIALIZED_SIZE;
//!
//!     let mut signature_offsets = vec![];
//!     let mut signature_buffer = vec![];
//!
//!     for signature_bundle in signatures {
//!         let data_start = data_start
//!             .checked_add(signature_buffer.len())
//!             .expect("overflow");
//!
//!         let signature_offset = data_start;
//!         let eth_address_offset = data_start
//!             .checked_add(SIGNATURE_SERIALIZED_SIZE + 1)
//!             .expect("overflow");
//!         let message_data_offset = eth_address_offset
//!             .checked_add(HASHED_PUBKEY_SERIALIZED_SIZE)
//!             .expect("overflow");
//!         let message_data_size = signature_bundle.message.len();
//!
//!         let signature_offset = u16::try_from(signature_offset)?;
//!         let eth_address_offset = u16::try_from(eth_address_offset)?;
//!         let message_data_offset = u16::try_from(message_data_offset)?;
//!         let message_data_size = u16::try_from(message_data_size)?;
//!
//!         signature_offsets.push(SecpSignatureOffsets {
//!             signature_offset,
//!             signature_instruction_index: instruction_index,
//!             eth_address_offset,
//!             eth_address_instruction_index: instruction_index,
//!             message_data_offset,
//!             message_data_size,
//!             message_instruction_index: instruction_index,
//!         });
//!
//!         signature_buffer.extend(signature_bundle.signature);
//!         signature_buffer.push(signature_bundle.recovery_id);
//!         signature_buffer.extend(&signature_bundle.eth_address);
//!         signature_buffer.extend(&signature_bundle.message);
//!     }
//!
//!     let mut instr_data = vec![];
//!     instr_data.push(signatures.len() as u8);
//!
//!     for offsets in signature_offsets {
//!         let offsets = bincode::serialize(&offsets)?;
//!         instr_data.extend(offsets);
//!     }
//!
//!     instr_data.extend(signature_buffer);
//!
//!     Ok(instr_data)
//! }
//!
//! fn demo_secp256k1_custom_many(
//!     payer_keypair: &Keypair,
//!     client: &RpcClient,
//!     program_keypair: &Keypair,
//! ) -> Result<()> {
//!     // Sign some messages.
//!     let mut signatures = vec![];
//!     for idx in 0..2 {
//!         let secret_key = libsecp256k1::SecretKey::random(&mut rand0_7::thread_rng());
//!         let message = format!("hello world {}", idx).into_bytes();
//!         let message_hash = {
//!             let mut hasher = keccak::Hasher::default();
//!             hasher.hash(&message);
//!             hasher.result()
//!         };
//!         let secp_message = libsecp256k1::Message::parse(&message_hash.0);
//!         let (signature, recovery_id) = libsecp256k1::sign(&secp_message, &secret_key);
//!         let signature = signature.serialize();
//!         let recovery_id = recovery_id.serialize();
//!
//!         let public_key = libsecp256k1::PublicKey::from_secret_key(&secret_key);
//!         let eth_address = secp256k1_instruction::construct_eth_pubkey(&public_key);
//!
//!         signatures.push(SecpSignature {
//!             signature,
//!             recovery_id,
//!             eth_address,
//!             message,
//!         });
//!     }
//!
//!     let secp256k1_instr_data = make_secp256k1_instruction_data(&signatures, 0)?;
//!     let secp256k1_instr = Instruction::new_with_bytes(
//!         solana_sdk::secp256k1_program::ID,
//!         &secp256k1_instr_data,
//!         vec![],
//!     );
//!
//!     let program_instr = Instruction::new_with_bytes(
//!         program_keypair.pubkey(),
//!         &[],
//!         vec![
//!             AccountMeta::new_readonly(sysvar::instructions::ID, false)
//!         ],
//!     );
//!
//!     let blockhash = client.get_latest_blockhash()?;
//!     let tx = Transaction::new_signed_with_payer(
//!         &[secp256k1_instr, program_instr],
//!         Some(&payer_keypair.pubkey()),
//!         &[payer_keypair],
//!         blockhash,
//!     );
//!
//!     client.send_and_confirm_transaction(&tx)?;
//!
//!     Ok(())
//! }
//! ```

#![cfg(feature = "full")]

use {
    crate::{
        feature_set::{
            libsecp256k1_0_5_upgrade_enabled, libsecp256k1_fail_on_bad_count,
            libsecp256k1_fail_on_bad_count2, FeatureSet,
        },
        instruction::Instruction,
        precompiles::PrecompileError,
    },
    digest::Digest,
    serde_derive::{Deserialize, Serialize},
};

pub const HASHED_PUBKEY_SERIALIZED_SIZE: usize = 20;
pub const SIGNATURE_SERIALIZED_SIZE: usize = 64;
pub const SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 11;
pub const DATA_START: usize = SIGNATURE_OFFSETS_SERIALIZED_SIZE + 1;

/// Offsets of signature data within a secp256k1 instruction.
///
/// See the [module documentation][md] for a complete description.
///
/// [md]: self
#[derive(Default, Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct SecpSignatureOffsets {
    /// Offset to 64-byte signature plus 1-byte recovery ID.
    pub signature_offset: u16,
    /// Within the transaction, the index of the instruction whose instruction data contains the signature.
    pub signature_instruction_index: u8,
    /// Offset to 20-byte Ethereum address.
    pub eth_address_offset: u16,
    /// Within the transaction, the index of the instruction whose instruction data contains the address.
    pub eth_address_instruction_index: u8,
    /// Offset to start of message data.
    pub message_data_offset: u16,
    /// Size of message data in bytes.
    pub message_data_size: u16,
    /// Within the transaction, the index of the instruction whose instruction data contains the message.
    pub message_instruction_index: u8,
}

/// Sign a message and create a secp256k1 program instruction to verify the signature.
///
/// This function is suitable for simple uses of the secp256k1 program.
/// More complex uses must encode the secp256k1 instruction data manually.
/// See the [module documentation][md] for examples.
///
/// [md]: self
///
/// The instruction generated by this function must be the first instruction
/// included in a transaction or it will not verify. The
/// [`SecpSignatureOffsets`] structure encoded in the instruction data specify
/// the instruction indexes as 0.
///
/// `message_arr` is hashed with the [`keccak`] hash function prior to signing.
///
/// [`keccak`]: crate::keccak
pub fn new_secp256k1_instruction(
    priv_key: &libsecp256k1::SecretKey,
    message_arr: &[u8],
) -> Instruction {
    let secp_pubkey = libsecp256k1::PublicKey::from_secret_key(priv_key);
    let eth_pubkey = construct_eth_pubkey(&secp_pubkey);
    let mut hasher = sha3::Keccak256::new();
    hasher.update(message_arr);
    let message_hash = hasher.finalize();
    let mut message_hash_arr = [0u8; 32];
    message_hash_arr.copy_from_slice(message_hash.as_slice());
    let message = libsecp256k1::Message::parse(&message_hash_arr);
    let (signature, recovery_id) = libsecp256k1::sign(&message, priv_key);
    let signature_arr = signature.serialize();
    assert_eq!(signature_arr.len(), SIGNATURE_SERIALIZED_SIZE);

    let instruction_data_len = DATA_START
        .saturating_add(eth_pubkey.len())
        .saturating_add(signature_arr.len())
        .saturating_add(message_arr.len())
        .saturating_add(1);
    let mut instruction_data = vec![0; instruction_data_len];

    let eth_address_offset = DATA_START;
    instruction_data[eth_address_offset..eth_address_offset.saturating_add(eth_pubkey.len())]
        .copy_from_slice(&eth_pubkey);

    let signature_offset = DATA_START.saturating_add(eth_pubkey.len());
    instruction_data[signature_offset..signature_offset.saturating_add(signature_arr.len())]
        .copy_from_slice(&signature_arr);

    instruction_data[signature_offset.saturating_add(signature_arr.len())] =
        recovery_id.serialize();

    let message_data_offset = signature_offset
        .saturating_add(signature_arr.len())
        .saturating_add(1);
    instruction_data[message_data_offset..].copy_from_slice(message_arr);

    let num_signatures = 1;
    instruction_data[0] = num_signatures;
    let offsets = SecpSignatureOffsets {
        signature_offset: signature_offset as u16,
        signature_instruction_index: 0,
        eth_address_offset: eth_address_offset as u16,
        eth_address_instruction_index: 0,
        message_data_offset: message_data_offset as u16,
        message_data_size: message_arr.len() as u16,
        message_instruction_index: 0,
    };
    let writer = std::io::Cursor::new(&mut instruction_data[1..DATA_START]);
    bincode::serialize_into(writer, &offsets).unwrap();

    Instruction {
        program_id: solana_sdk::secp256k1_program::id(),
        accounts: vec![],
        data: instruction_data,
    }
}

/// Creates an Ethereum address from a secp256k1 public key.
pub fn construct_eth_pubkey(
    pubkey: &libsecp256k1::PublicKey,
) -> [u8; HASHED_PUBKEY_SERIALIZED_SIZE] {
    let mut addr = [0u8; HASHED_PUBKEY_SERIALIZED_SIZE];
    addr.copy_from_slice(&sha3::Keccak256::digest(&pubkey.serialize()[1..])[12..]);
    assert_eq!(addr.len(), HASHED_PUBKEY_SERIALIZED_SIZE);
    addr
}

/// Verifies the signatures specified in the secp256k1 instruction data.
///
/// This is same the verification routine executed by the runtime's secp256k1 native program,
/// and is primarily of use to the runtime.
///
/// `data` is the secp256k1 program's instruction data. `instruction_datas` is
/// the full slice of instruction datas for all instructions in the transaction,
/// including the secp256k1 program's instruction data.
///
/// `feature_set` is the set of active Solana features. It is used to enable or
/// disable a few minor additional checks that were activated on chain
/// subsequent to the addition of the secp256k1 native program. For many
/// purposes passing `FeatureSet::all_enabled()` is reasonable.
pub fn verify(
    data: &[u8],
    instruction_datas: &[&[u8]],
    feature_set: &FeatureSet,
) -> Result<(), PrecompileError> {
    if data.is_empty() {
        return Err(PrecompileError::InvalidInstructionDataSize);
    }
    let count = data[0] as usize;
    if (feature_set.is_active(&libsecp256k1_fail_on_bad_count::id())
        || feature_set.is_active(&libsecp256k1_fail_on_bad_count2::id()))
        && count == 0
        && data.len() > 1
    {
        // count is zero but the instruction data indicates that is probably not
        // correct, fail the instruction to catch probable invalid secp256k1
        // instruction construction.
        return Err(PrecompileError::InvalidInstructionDataSize);
    }
    let expected_data_size = count
        .saturating_mul(SIGNATURE_OFFSETS_SERIALIZED_SIZE)
        .saturating_add(1);
    if data.len() < expected_data_size {
        return Err(PrecompileError::InvalidInstructionDataSize);
    }
    for i in 0..count {
        let start = i
            .saturating_mul(SIGNATURE_OFFSETS_SERIALIZED_SIZE)
            .saturating_add(1);
        let end = start.saturating_add(SIGNATURE_OFFSETS_SERIALIZED_SIZE);

        let offsets: SecpSignatureOffsets = bincode::deserialize(&data[start..end])
            .map_err(|_| PrecompileError::InvalidSignature)?;

        // Parse out signature
        let signature_index = offsets.signature_instruction_index as usize;
        if signature_index >= instruction_datas.len() {
            return Err(PrecompileError::InvalidInstructionDataSize);
        }
        let signature_instruction = instruction_datas[signature_index];
        let sig_start = offsets.signature_offset as usize;
        let sig_end = sig_start.saturating_add(SIGNATURE_SERIALIZED_SIZE);
        if sig_end >= signature_instruction.len() {
            return Err(PrecompileError::InvalidSignature);
        }

        let sig_parse_result = if feature_set.is_active(&libsecp256k1_0_5_upgrade_enabled::id()) {
            libsecp256k1::Signature::parse_standard_slice(
                &signature_instruction[sig_start..sig_end],
            )
        } else {
            libsecp256k1::Signature::parse_overflowing_slice(
                &signature_instruction[sig_start..sig_end],
            )
        };

        let signature = sig_parse_result.map_err(|_| PrecompileError::InvalidSignature)?;

        let recovery_id = libsecp256k1::RecoveryId::parse(signature_instruction[sig_end])
            .map_err(|_| PrecompileError::InvalidRecoveryId)?;

        // Parse out pubkey
        let eth_address_slice = get_data_slice(
            instruction_datas,
            offsets.eth_address_instruction_index,
            offsets.eth_address_offset,
            HASHED_PUBKEY_SERIALIZED_SIZE,
        )?;

        // Parse out message
        let message_slice = get_data_slice(
            instruction_datas,
            offsets.message_instruction_index,
            offsets.message_data_offset,
            offsets.message_data_size as usize,
        )?;

        let mut hasher = sha3::Keccak256::new();
        hasher.update(message_slice);
        let message_hash = hasher.finalize();

        let pubkey = libsecp256k1::recover(
            &libsecp256k1::Message::parse_slice(&message_hash).unwrap(),
            &signature,
            &recovery_id,
        )
        .map_err(|_| PrecompileError::InvalidSignature)?;
        let eth_address = construct_eth_pubkey(&pubkey);

        if eth_address_slice != eth_address {
            return Err(PrecompileError::InvalidSignature);
        }
    }
    Ok(())
}

fn get_data_slice<'a>(
    instruction_datas: &'a [&[u8]],
    instruction_index: u8,
    offset_start: u16,
    size: usize,
) -> Result<&'a [u8], PrecompileError> {
    let signature_index = instruction_index as usize;
    if signature_index >= instruction_datas.len() {
        return Err(PrecompileError::InvalidDataOffsets);
    }
    let signature_instruction = &instruction_datas[signature_index];
    let start = offset_start as usize;
    let end = start.saturating_add(size);
    if end > signature_instruction.len() {
        return Err(PrecompileError::InvalidSignature);
    }

    Ok(&instruction_datas[signature_index][start..end])
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        crate::{
            feature_set,
            hash::Hash,
            keccak,
            secp256k1_instruction::{
                new_secp256k1_instruction, SecpSignatureOffsets, SIGNATURE_OFFSETS_SERIALIZED_SIZE,
            },
            signature::{Keypair, Signer},
            transaction::Transaction,
        },
        rand0_7::{thread_rng, Rng},
    };

    fn test_case(
        num_signatures: u8,
        offsets: &SecpSignatureOffsets,
    ) -> Result<(), PrecompileError> {
        let mut instruction_data = vec![0u8; DATA_START];
        instruction_data[0] = num_signatures;
        let writer = std::io::Cursor::new(&mut instruction_data[1..]);
        bincode::serialize_into(writer, &offsets).unwrap();
        let mut feature_set = FeatureSet::all_enabled();
        feature_set
            .active
            .remove(&libsecp256k1_0_5_upgrade_enabled::id());
        feature_set
            .inactive
            .insert(libsecp256k1_0_5_upgrade_enabled::id());

        verify(&instruction_data, &[&[0u8; 100]], &feature_set)
    }

    #[test]
    fn test_invalid_offsets() {
        solana_logger::setup();

        let mut instruction_data = vec![0u8; DATA_START];
        let offsets = SecpSignatureOffsets::default();
        instruction_data[0] = 1;
        let writer = std::io::Cursor::new(&mut instruction_data[1..]);
        bincode::serialize_into(writer, &offsets).unwrap();
        instruction_data.truncate(instruction_data.len() - 1);
        let mut feature_set = FeatureSet::all_enabled();
        feature_set
            .active
            .remove(&libsecp256k1_0_5_upgrade_enabled::id());
        feature_set
            .inactive
            .insert(libsecp256k1_0_5_upgrade_enabled::id());

        assert_eq!(
            verify(&instruction_data, &[&[0u8; 100]], &feature_set),
            Err(PrecompileError::InvalidInstructionDataSize)
        );

        let offsets = SecpSignatureOffsets {
            signature_instruction_index: 1,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidInstructionDataSize)
        );

        let offsets = SecpSignatureOffsets {
            message_instruction_index: 1,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidDataOffsets)
        );

        let offsets = SecpSignatureOffsets {
            eth_address_instruction_index: 1,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidDataOffsets)
        );
    }

    #[test]
    fn test_message_data_offsets() {
        let offsets = SecpSignatureOffsets {
            message_data_offset: 99,
            message_data_size: 1,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidSignature)
        );

        let offsets = SecpSignatureOffsets {
            message_data_offset: 100,
            message_data_size: 1,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidSignature)
        );

        let offsets = SecpSignatureOffsets {
            message_data_offset: 100,
            message_data_size: 1000,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidSignature)
        );

        let offsets = SecpSignatureOffsets {
            message_data_offset: std::u16::MAX,
            message_data_size: std::u16::MAX,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidSignature)
        );
    }

    #[test]
    fn test_eth_offset() {
        let offsets = SecpSignatureOffsets {
            eth_address_offset: std::u16::MAX,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidSignature)
        );

        let offsets = SecpSignatureOffsets {
            eth_address_offset: 100 - HASHED_PUBKEY_SERIALIZED_SIZE as u16 + 1,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidSignature)
        );
    }

    #[test]
    fn test_signature_offset() {
        let offsets = SecpSignatureOffsets {
            signature_offset: std::u16::MAX,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidSignature)
        );

        let offsets = SecpSignatureOffsets {
            signature_offset: 100 - SIGNATURE_SERIALIZED_SIZE as u16 + 1,
            ..SecpSignatureOffsets::default()
        };
        assert_eq!(
            test_case(1, &offsets),
            Err(PrecompileError::InvalidSignature)
        );
    }

    #[test]
    fn test_count_is_zero_but_sig_data_exists() {
        solana_logger::setup();

        let mut instruction_data = vec![0u8; DATA_START];
        let offsets = SecpSignatureOffsets::default();
        instruction_data[0] = 0;
        let writer = std::io::Cursor::new(&mut instruction_data[1..]);
        bincode::serialize_into(writer, &offsets).unwrap();
        let mut feature_set = FeatureSet::all_enabled();
        feature_set
            .active
            .remove(&libsecp256k1_0_5_upgrade_enabled::id());
        feature_set
            .inactive
            .insert(libsecp256k1_0_5_upgrade_enabled::id());

        assert_eq!(
            verify(&instruction_data, &[&[0u8; 100]], &feature_set),
            Err(PrecompileError::InvalidInstructionDataSize)
        );
    }

    #[test]
    fn test_secp256k1() {
        solana_logger::setup();
        let offsets = SecpSignatureOffsets::default();
        assert_eq!(
            bincode::serialized_size(&offsets).unwrap() as usize,
            SIGNATURE_OFFSETS_SERIALIZED_SIZE
        );

        let secp_privkey = libsecp256k1::SecretKey::random(&mut thread_rng());
        let message_arr = b"hello";
        let mut secp_instruction = new_secp256k1_instruction(&secp_privkey, message_arr);
        let mint_keypair = Keypair::new();
        let mut feature_set = feature_set::FeatureSet::all_enabled();
        feature_set
            .active
            .remove(&feature_set::libsecp256k1_0_5_upgrade_enabled::id());
        feature_set
            .inactive
            .insert(feature_set::libsecp256k1_0_5_upgrade_enabled::id());
        let feature_set = feature_set;

        let tx = Transaction::new_signed_with_payer(
            &[secp_instruction.clone()],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            Hash::default(),
        );

        assert!(tx.verify_precompiles(&feature_set).is_ok());

        let index = thread_rng().gen_range(0, secp_instruction.data.len());
        secp_instruction.data[index] = secp_instruction.data[index].wrapping_add(12);
        let tx = Transaction::new_signed_with_payer(
            &[secp_instruction],
            Some(&mint_keypair.pubkey()),
            &[&mint_keypair],
            Hash::default(),
        );
        assert!(tx.verify_precompiles(&feature_set).is_err());
    }

    // Signatures are malleable.
    #[test]
    fn test_malleability() {
        solana_logger::setup();

        let secret_key = libsecp256k1::SecretKey::random(&mut thread_rng());
        let public_key = libsecp256k1::PublicKey::from_secret_key(&secret_key);
        let eth_address = construct_eth_pubkey(&public_key);

        let message = b"hello";
        let message_hash = {
            let mut hasher = keccak::Hasher::default();
            hasher.hash(message);
            hasher.result()
        };

        let secp_message = libsecp256k1::Message::parse(&message_hash.0);
        let (signature, recovery_id) = libsecp256k1::sign(&secp_message, &secret_key);

        // Flip the S value in the signature to make a different but valid signature.
        let mut alt_signature = signature;
        alt_signature.s = -alt_signature.s;
        let alt_recovery_id = libsecp256k1::RecoveryId::parse(recovery_id.serialize() ^ 1).unwrap();

        let mut data: Vec<u8> = vec![];
        let mut both_offsets = vec![];

        // Verify both signatures of the same message.
        let sigs = [(signature, recovery_id), (alt_signature, alt_recovery_id)];
        for (signature, recovery_id) in sigs.iter() {
            let signature_offset = data.len();
            data.extend(signature.serialize());
            data.push(recovery_id.serialize());
            let eth_address_offset = data.len();
            data.extend(eth_address);
            let message_data_offset = data.len();
            data.extend(message);

            let data_start = 1 + SIGNATURE_OFFSETS_SERIALIZED_SIZE * 2;

            let offsets = SecpSignatureOffsets {
                signature_offset: (signature_offset + data_start) as u16,
                signature_instruction_index: 0,
                eth_address_offset: (eth_address_offset + data_start) as u16,
                eth_address_instruction_index: 0,
                message_data_offset: (message_data_offset + data_start) as u16,
                message_data_size: message.len() as u16,
                message_instruction_index: 0,
            };

            both_offsets.push(offsets);
        }

        let mut instruction_data: Vec<u8> = vec![2];

        for offsets in both_offsets {
            let offsets = bincode::serialize(&offsets).unwrap();
            instruction_data.extend(offsets);
        }

        instruction_data.extend(data);

        verify(
            &instruction_data,
            &[&instruction_data],
            &FeatureSet::all_enabled(),
        )
        .unwrap();
    }
}
