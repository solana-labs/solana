//! Atomically-committed sequences of instructions.
//!
//! While [`Instruction`]s are the basic unit of computation in Solana, they are
//! submitted by clients in [`Transaction`]s containing one or more
//! instructions, and signed by one or more [`Signer`]s. Solana executes the
//! instructions in a transaction in order, and only commits any changes if all
//! instructions terminate without producing an error or exception.
//!
//! Transactions do not directly contain their instructions but instead include
//! a [`Message`], a precompiled representation of a sequence of instructions.
//! `Message`'s constructors handle the complex task of reordering the
//! individual lists of accounts required by each instruction into a single flat
//! list of deduplicated accounts required by the Solana runtime. The
//! `Transaction` type has constructors that build the `Message` so that clients
//! don't need to interact with them directly.
//!
//! Prior to submission to the network, transactions must be signed by one or or
//! more keypairs, and this signing is typically performed by an abstract
//! [`Signer`], which may be a [`Keypair`] but may also be other types of
//! signers including remote wallets, such as Ledger devices, as represented by
//! the [`RemoteKeypair`] type in the [`solana-remote-wallet`] crate.
//!
//! [`Signer`]: crate::signer::Signer
//! [`Keypair`]: crate::signer::keypair::Keypair
//! [`solana-remote-wallet`]: https://docs.rs/solana-remote-wallet/latest/
//! [`RemoteKeypair`]: https://docs.rs/solana-remote-wallet/latest/solana_remote_wallet/remote_keypair/struct.RemoteKeypair.html
//!
//! Every transaction must be signed by a fee-paying account, the account from
//! which the cost of executing the transaction is withdrawn. Other required
//! signatures are determined by the requirements of the programs being executed
//! by each instruction, and are conventionally specified by that program's
//! documentation.
//!
//! When signing a transaction, a recent blockhash must be provided (which can
//! be retrieved with [`RpcClient::get_latest_blockhash`]). This allows
//! validators to drop old but unexecuted transactions; and to distinguish
//! between accidentally duplicated transactions and intentionally duplicated
//! transactions &mdash; any identical transactions will not be executed more
//! than once, so updating the blockhash between submitting otherwise identical
//! transactions makes them unique. If a client must sign a transaction long
//! before submitting it to the network, then it can use the _[durable
//! transaction nonce]_ mechanism instead of a recent blockhash to ensure unique
//! transactions.
//!
//! [`RpcClient::get_latest_blockhash`]: https://docs.rs/solana-rpc-client/latest/solana_rpc_client/rpc_client/struct.RpcClient.html#method.get_latest_blockhash
//! [durable transaction nonce]: https://docs.solana.com/implemented-proposals/durable-tx-nonces
//!
//! # Examples
//!
//! This example uses the [`solana_rpc_client`] and [`anyhow`] crates.
//!
//! [`solana_rpc_client`]: https://docs.rs/solana-rpc-client
//! [`anyhow`]: https://docs.rs/anyhow
//!
//! ```
//! # use solana_sdk::example_mocks::solana_rpc_client;
//! use anyhow::Result;
//! use borsh::{BorshSerialize, BorshDeserialize};
//! use solana_rpc_client::rpc_client::RpcClient;
//! use solana_sdk::{
//!      instruction::Instruction,
//!      message::Message,
//!      pubkey::Pubkey,
//!      signature::{Keypair, Signer},
//!      transaction::Transaction,
//! };
//!
//! // A custom program instruction. This would typically be defined in
//! // another crate so it can be shared between the on-chain program and
//! // the client.
//! #[derive(BorshSerialize, BorshDeserialize)]
//! enum BankInstruction {
//!     Initialize,
//!     Deposit { lamports: u64 },
//!     Withdraw { lamports: u64 },
//! }
//!
//! fn send_initialize_tx(
//!     client: &RpcClient,
//!     program_id: Pubkey,
//!     payer: &Keypair
//! ) -> Result<()> {
//!
//!     let bank_instruction = BankInstruction::Initialize;
//!
//!     let instruction = Instruction::new_with_borsh(
//!         program_id,
//!         &bank_instruction,
//!         vec![],
//!     );
//!
//!     let blockhash = client.get_latest_blockhash()?;
//!     let mut tx = Transaction::new_signed_with_payer(
//!         &[instruction],
//!         Some(&payer.pubkey()),
//!         &[payer],
//!         blockhash,
//!     );
//!     client.send_and_confirm_transaction(&tx)?;
//!
//!     Ok(())
//! }
//! #
//! # let client = RpcClient::new(String::new());
//! # let program_id = Pubkey::new_unique();
//! # let payer = Keypair::new();
//! # send_initialize_tx(&client, program_id, &payer)?;
//! #
//! # Ok::<(), anyhow::Error>(())
//! ```

#![cfg(feature = "full")]

use {
    crate::{
        hash::Hash,
        instruction::{CompiledInstruction, Instruction},
        message::Message,
        nonce::NONCED_TX_MARKER_IX_INDEX,
        precompiles::verify_if_precompile,
        program_utils::limited_deserialize,
        pubkey::Pubkey,
        sanitize::{Sanitize, SanitizeError},
        short_vec,
        signature::{Signature, SignerError},
        signers::Signers,
        wasm_bindgen,
    },
    serde::Serialize,
    solana_program::{system_instruction::SystemInstruction, system_program},
    solana_sdk::feature_set,
    std::result,
};

mod error;
mod sanitized;
mod versioned;

pub use {error::*, sanitized::*, versioned::*};

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum TransactionVerificationMode {
    HashOnly,
    HashAndVerifyPrecompiles,
    FullVerification,
}

pub type Result<T> = result::Result<T, TransactionError>;

/// An atomically-commited sequence of instructions.
///
/// While [`Instruction`]s are the basic unit of computation in Solana,
/// they are submitted by clients in [`Transaction`]s containing one or
/// more instructions, and signed by one or more [`Signer`]s.
///
/// [`Signer`]: crate::signer::Signer
///
/// See the [module documentation] for more details about transactions.
///
/// [module documentation]: self
///
/// Some constructors accept an optional `payer`, the account responsible for
/// paying the cost of executing a transaction. In most cases, callers should
/// specify the payer explicitly in these constructors. In some cases though,
/// the caller is not _required_ to specify the payer, but is still allowed to:
/// in the [`Message`] structure, the first account is always the fee-payer, so
/// if the caller has knowledge that the first account of the constructed
/// transaction's `Message` is both a signer and the expected fee-payer, then
/// redundantly specifying the fee-payer is not strictly required.
#[wasm_bindgen]
#[frozen_abi(digest = "FZtncnS1Xk8ghHfKiXE5oGiUbw2wJhmfXQuNgQR3K6Mc")]
#[derive(Debug, PartialEq, Default, Eq, Clone, Serialize, Deserialize, AbiExample)]
pub struct Transaction {
    /// A set of signatures of a serialized [`Message`], signed by the first
    /// keys of the `Message`'s [`account_keys`], where the number of signatures
    /// is equal to [`num_required_signatures`] of the `Message`'s
    /// [`MessageHeader`].
    ///
    /// [`account_keys`]: Message::account_keys
    /// [`MessageHeader`]: crate::message::MessageHeader
    /// [`num_required_signatures`]: crate::message::MessageHeader::num_required_signatures
    // NOTE: Serialization-related changes must be paired with the direct read at sigverify.
    #[wasm_bindgen(skip)]
    #[serde(with = "short_vec")]
    pub signatures: Vec<Signature>,

    /// The message to sign.
    #[wasm_bindgen(skip)]
    pub message: Message,
}

impl Sanitize for Transaction {
    fn sanitize(&self) -> std::result::Result<(), SanitizeError> {
        if self.message.header.num_required_signatures as usize > self.signatures.len() {
            return Err(SanitizeError::IndexOutOfBounds);
        }
        if self.signatures.len() > self.message.account_keys.len() {
            return Err(SanitizeError::IndexOutOfBounds);
        }
        self.message.sanitize()
    }
}

impl Transaction {
    /// Create an unsigned transaction from a [`Message`].
    ///
    /// # Examples
    ///
    /// This example uses the [`solana_rpc_client`] and [`anyhow`] crates.
    ///
    /// [`solana_rpc_client`]: https://docs.rs/solana-rpc-client
    /// [`anyhow`]: https://docs.rs/anyhow
    ///
    /// ```
    /// # use solana_sdk::example_mocks::solana_rpc_client;
    /// use anyhow::Result;
    /// use borsh::{BorshSerialize, BorshDeserialize};
    /// use solana_rpc_client::rpc_client::RpcClient;
    /// use solana_sdk::{
    ///      instruction::Instruction,
    ///      message::Message,
    ///      pubkey::Pubkey,
    ///      signature::{Keypair, Signer},
    ///      transaction::Transaction,
    /// };
    ///
    /// // A custom program instruction. This would typically be defined in
    /// // another crate so it can be shared between the on-chain program and
    /// // the client.
    /// #[derive(BorshSerialize, BorshDeserialize)]
    /// enum BankInstruction {
    ///     Initialize,
    ///     Deposit { lamports: u64 },
    ///     Withdraw { lamports: u64 },
    /// }
    ///
    /// fn send_initialize_tx(
    ///     client: &RpcClient,
    ///     program_id: Pubkey,
    ///     payer: &Keypair
    /// ) -> Result<()> {
    ///
    ///     let bank_instruction = BankInstruction::Initialize;
    ///
    ///     let instruction = Instruction::new_with_borsh(
    ///         program_id,
    ///         &bank_instruction,
    ///         vec![],
    ///     );
    ///
    ///     let message = Message::new(
    ///         &[instruction],
    ///         Some(&payer.pubkey()),
    ///     );
    ///
    ///     let mut tx = Transaction::new_unsigned(message);
    ///     let blockhash = client.get_latest_blockhash()?;
    ///     tx.sign(&[payer], blockhash);
    ///     client.send_and_confirm_transaction(&tx)?;
    ///
    ///     Ok(())
    /// }
    /// #
    /// # let client = RpcClient::new(String::new());
    /// # let program_id = Pubkey::new_unique();
    /// # let payer = Keypair::new();
    /// # send_initialize_tx(&client, program_id, &payer)?;
    /// #
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn new_unsigned(message: Message) -> Self {
        Self {
            signatures: vec![Signature::default(); message.header.num_required_signatures as usize],
            message,
        }
    }

    /// Create a fully-signed transaction from a [`Message`].
    ///
    /// # Panics
    ///
    /// Panics when signing fails. See [`Transaction::try_sign`] and
    /// [`Transaction::try_partial_sign`] for a full description of failure
    /// scenarios.
    ///
    /// # Examples
    ///
    /// This example uses the [`solana_rpc_client`] and [`anyhow`] crates.
    ///
    /// [`solana_rpc_client`]: https://docs.rs/solana-rpc-client
    /// [`anyhow`]: https://docs.rs/anyhow
    ///
    /// ```
    /// # use solana_sdk::example_mocks::solana_rpc_client;
    /// use anyhow::Result;
    /// use borsh::{BorshSerialize, BorshDeserialize};
    /// use solana_rpc_client::rpc_client::RpcClient;
    /// use solana_sdk::{
    ///      instruction::Instruction,
    ///      message::Message,
    ///      pubkey::Pubkey,
    ///      signature::{Keypair, Signer},
    ///      transaction::Transaction,
    /// };
    ///
    /// // A custom program instruction. This would typically be defined in
    /// // another crate so it can be shared between the on-chain program and
    /// // the client.
    /// #[derive(BorshSerialize, BorshDeserialize)]
    /// enum BankInstruction {
    ///     Initialize,
    ///     Deposit { lamports: u64 },
    ///     Withdraw { lamports: u64 },
    /// }
    ///
    /// fn send_initialize_tx(
    ///     client: &RpcClient,
    ///     program_id: Pubkey,
    ///     payer: &Keypair
    /// ) -> Result<()> {
    ///
    ///     let bank_instruction = BankInstruction::Initialize;
    ///
    ///     let instruction = Instruction::new_with_borsh(
    ///         program_id,
    ///         &bank_instruction,
    ///         vec![],
    ///     );
    ///
    ///     let message = Message::new(
    ///         &[instruction],
    ///         Some(&payer.pubkey()),
    ///     );
    ///
    ///     let blockhash = client.get_latest_blockhash()?;
    ///     let mut tx = Transaction::new(&[payer], message, blockhash);
    ///     client.send_and_confirm_transaction(&tx)?;
    ///
    ///     Ok(())
    /// }
    /// #
    /// # let client = RpcClient::new(String::new());
    /// # let program_id = Pubkey::new_unique();
    /// # let payer = Keypair::new();
    /// # send_initialize_tx(&client, program_id, &payer)?;
    /// #
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn new<T: Signers + ?Sized>(
        from_keypairs: &T,
        message: Message,
        recent_blockhash: Hash,
    ) -> Transaction {
        let mut tx = Self::new_unsigned(message);
        tx.sign(from_keypairs, recent_blockhash);
        tx
    }

    /// Create an unsigned transaction from a list of [`Instruction`]s.
    ///
    /// `payer` is the account responsible for paying the cost of executing the
    /// transaction. It is typically provided, but is optional in some cases.
    /// See the [`Transaction`] docs for more.
    ///
    /// # Examples
    ///
    /// This example uses the [`solana_rpc_client`] and [`anyhow`] crates.
    ///
    /// [`solana_rpc_client`]: https://docs.rs/solana-rpc-client
    /// [`anyhow`]: https://docs.rs/anyhow
    ///
    /// ```
    /// # use solana_sdk::example_mocks::solana_rpc_client;
    /// use anyhow::Result;
    /// use borsh::{BorshSerialize, BorshDeserialize};
    /// use solana_rpc_client::rpc_client::RpcClient;
    /// use solana_sdk::{
    ///      instruction::Instruction,
    ///      message::Message,
    ///      pubkey::Pubkey,
    ///      signature::{Keypair, Signer},
    ///      transaction::Transaction,
    /// };
    ///
    /// // A custom program instruction. This would typically be defined in
    /// // another crate so it can be shared between the on-chain program and
    /// // the client.
    /// #[derive(BorshSerialize, BorshDeserialize)]
    /// enum BankInstruction {
    ///     Initialize,
    ///     Deposit { lamports: u64 },
    ///     Withdraw { lamports: u64 },
    /// }
    ///
    /// fn send_initialize_tx(
    ///     client: &RpcClient,
    ///     program_id: Pubkey,
    ///     payer: &Keypair
    /// ) -> Result<()> {
    ///
    ///     let bank_instruction = BankInstruction::Initialize;
    ///
    ///     let instruction = Instruction::new_with_borsh(
    ///         program_id,
    ///         &bank_instruction,
    ///         vec![],
    ///     );
    ///
    ///     let mut tx = Transaction::new_with_payer(&[instruction], Some(&payer.pubkey()));
    ///     let blockhash = client.get_latest_blockhash()?;
    ///     tx.sign(&[payer], blockhash);
    ///     client.send_and_confirm_transaction(&tx)?;
    ///
    ///     Ok(())
    /// }
    /// #
    /// # let client = RpcClient::new(String::new());
    /// # let program_id = Pubkey::new_unique();
    /// # let payer = Keypair::new();
    /// # send_initialize_tx(&client, program_id, &payer)?;
    /// #
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn new_with_payer(instructions: &[Instruction], payer: Option<&Pubkey>) -> Self {
        let message = Message::new(instructions, payer);
        Self::new_unsigned(message)
    }

    /// Create a fully-signed transaction from a list of [`Instruction`]s.
    ///
    /// `payer` is the account responsible for paying the cost of executing the
    /// transaction. It is typically provided, but is optional in some cases.
    /// See the [`Transaction`] docs for more.
    ///
    /// # Panics
    ///
    /// Panics when signing fails. See [`Transaction::try_sign`] and
    /// [`Transaction::try_partial_sign`] for a full description of failure
    /// scenarios.
    ///
    /// # Examples
    ///
    /// This example uses the [`solana_rpc_client`] and [`anyhow`] crates.
    ///
    /// [`solana_rpc_client`]: https://docs.rs/solana-rpc-client
    /// [`anyhow`]: https://docs.rs/anyhow
    ///
    /// ```
    /// # use solana_sdk::example_mocks::solana_rpc_client;
    /// use anyhow::Result;
    /// use borsh::{BorshSerialize, BorshDeserialize};
    /// use solana_rpc_client::rpc_client::RpcClient;
    /// use solana_sdk::{
    ///      instruction::Instruction,
    ///      message::Message,
    ///      pubkey::Pubkey,
    ///      signature::{Keypair, Signer},
    ///      transaction::Transaction,
    /// };
    ///
    /// // A custom program instruction. This would typically be defined in
    /// // another crate so it can be shared between the on-chain program and
    /// // the client.
    /// #[derive(BorshSerialize, BorshDeserialize)]
    /// enum BankInstruction {
    ///     Initialize,
    ///     Deposit { lamports: u64 },
    ///     Withdraw { lamports: u64 },
    /// }
    ///
    /// fn send_initialize_tx(
    ///     client: &RpcClient,
    ///     program_id: Pubkey,
    ///     payer: &Keypair
    /// ) -> Result<()> {
    ///
    ///     let bank_instruction = BankInstruction::Initialize;
    ///
    ///     let instruction = Instruction::new_with_borsh(
    ///         program_id,
    ///         &bank_instruction,
    ///         vec![],
    ///     );
    ///
    ///     let blockhash = client.get_latest_blockhash()?;
    ///     let mut tx = Transaction::new_signed_with_payer(
    ///         &[instruction],
    ///         Some(&payer.pubkey()),
    ///         &[payer],
    ///         blockhash,
    ///     );
    ///     client.send_and_confirm_transaction(&tx)?;
    ///
    ///     Ok(())
    /// }
    /// #
    /// # let client = RpcClient::new(String::new());
    /// # let program_id = Pubkey::new_unique();
    /// # let payer = Keypair::new();
    /// # send_initialize_tx(&client, program_id, &payer)?;
    /// #
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn new_signed_with_payer<T: Signers + ?Sized>(
        instructions: &[Instruction],
        payer: Option<&Pubkey>,
        signing_keypairs: &T,
        recent_blockhash: Hash,
    ) -> Self {
        let message = Message::new(instructions, payer);
        Self::new(signing_keypairs, message, recent_blockhash)
    }

    /// Create a fully-signed transaction from pre-compiled instructions.
    ///
    /// # Arguments
    ///
    /// * `from_keypairs` - The keys used to sign the transaction.
    /// * `keys` - The keys for the transaction.  These are the program state
    ///    instances or lamport recipient keys.
    /// * `recent_blockhash` - The PoH hash.
    /// * `program_ids` - The keys that identify programs used in the `instruction` vector.
    /// * `instructions` - Instructions that will be executed atomically.
    ///
    /// # Panics
    ///
    /// Panics when signing fails. See [`Transaction::try_sign`] and for a full
    /// description of failure conditions.
    pub fn new_with_compiled_instructions<T: Signers + ?Sized>(
        from_keypairs: &T,
        keys: &[Pubkey],
        recent_blockhash: Hash,
        program_ids: Vec<Pubkey>,
        instructions: Vec<CompiledInstruction>,
    ) -> Self {
        let mut account_keys = from_keypairs.pubkeys();
        let from_keypairs_len = account_keys.len();
        account_keys.extend_from_slice(keys);
        account_keys.extend(&program_ids);
        let message = Message::new_with_compiled_instructions(
            from_keypairs_len as u8,
            0,
            program_ids.len() as u8,
            account_keys,
            Hash::default(),
            instructions,
        );
        Transaction::new(from_keypairs, message, recent_blockhash)
    }

    /// Get the data for an instruction at the given index.
    ///
    /// The `instruction_index` corresponds to the [`instructions`] vector of
    /// the `Transaction`'s [`Message`] value.
    ///
    /// [`instructions`]: Message::instructions
    ///
    /// # Panics
    ///
    /// Panics if `instruction_index` is greater than or equal to the number of
    /// instructions in the transaction.
    pub fn data(&self, instruction_index: usize) -> &[u8] {
        &self.message.instructions[instruction_index].data
    }

    fn key_index(&self, instruction_index: usize, accounts_index: usize) -> Option<usize> {
        self.message
            .instructions
            .get(instruction_index)
            .and_then(|instruction| instruction.accounts.get(accounts_index))
            .map(|&account_keys_index| account_keys_index as usize)
    }

    /// Get the `Pubkey` of an account required by one of the instructions in
    /// the transaction.
    ///
    /// The `instruction_index` corresponds to the [`instructions`] vector of
    /// the `Transaction`'s [`Message`] value; and the `account_index` to the
    /// [`accounts`] vector of the message's [`CompiledInstruction`]s.
    ///
    /// [`instructions`]: Message::instructions
    /// [`accounts`]: CompiledInstruction::accounts
    /// [`CompiledInstruction`]: CompiledInstruction
    ///
    /// Returns `None` if `instruction_index` is greater than or equal to the
    /// number of instructions in the transaction; or if `accounts_index` is
    /// greater than or equal to the number of accounts in the instruction.
    pub fn key(&self, instruction_index: usize, accounts_index: usize) -> Option<&Pubkey> {
        self.key_index(instruction_index, accounts_index)
            .and_then(|account_keys_index| self.message.account_keys.get(account_keys_index))
    }

    /// Get the `Pubkey` of a signing account required by one of the
    /// instructions in the transaction.
    ///
    /// The transaction does not need to be signed for this function to return a
    /// signing account's pubkey.
    ///
    /// Returns `None` if the indexed account is not required to sign the
    /// transaction. Returns `None` if the [`signatures`] field does not contain
    /// enough elements to hold a signature for the indexed account (this should
    /// only be possible if `Transaction` has been manually constructed).
    ///
    /// [`signatures`]: Transaction::signatures
    ///
    /// Returns `None` if `instruction_index` is greater than or equal to the
    /// number of instructions in the transaction; or if `accounts_index` is
    /// greater than or equal to the number of accounts in the instruction.
    pub fn signer_key(&self, instruction_index: usize, accounts_index: usize) -> Option<&Pubkey> {
        match self.key_index(instruction_index, accounts_index) {
            None => None,
            Some(signature_index) => {
                if signature_index >= self.signatures.len() {
                    return None;
                }
                self.message.account_keys.get(signature_index)
            }
        }
    }

    /// Return the message containing all data that should be signed.
    pub fn message(&self) -> &Message {
        &self.message
    }

    /// Return the serialized message data to sign.
    pub fn message_data(&self) -> Vec<u8> {
        self.message().serialize()
    }

    /// Sign the transaction.
    ///
    /// This method fully signs a transaction with all required signers, which
    /// must be present in the `keypairs` slice. To sign with only some of the
    /// required signers, use [`Transaction::partial_sign`].
    ///
    /// If `recent_blockhash` is different than recorded in the transaction message's
    /// [`recent_blockhash`] field, then the message's `recent_blockhash` will be updated
    /// to the provided `recent_blockhash`, and any prior signatures will be cleared.
    ///
    /// [`recent_blockhash`]: Message::recent_blockhash
    ///
    /// # Panics
    ///
    /// Panics when signing fails. Use [`Transaction::try_sign`] to handle the
    /// error. See the documentation for [`Transaction::try_sign`] for a full description of
    /// failure conditions.
    ///
    /// # Examples
    ///
    /// This example uses the [`solana_rpc_client`] and [`anyhow`] crates.
    ///
    /// [`solana_rpc_client`]: https://docs.rs/solana-rpc-client
    /// [`anyhow`]: https://docs.rs/anyhow
    ///
    /// ```
    /// # use solana_sdk::example_mocks::solana_rpc_client;
    /// use anyhow::Result;
    /// use borsh::{BorshSerialize, BorshDeserialize};
    /// use solana_rpc_client::rpc_client::RpcClient;
    /// use solana_sdk::{
    ///      instruction::Instruction,
    ///      message::Message,
    ///      pubkey::Pubkey,
    ///      signature::{Keypair, Signer},
    ///      transaction::Transaction,
    /// };
    ///
    /// // A custom program instruction. This would typically be defined in
    /// // another crate so it can be shared between the on-chain program and
    /// // the client.
    /// #[derive(BorshSerialize, BorshDeserialize)]
    /// enum BankInstruction {
    ///     Initialize,
    ///     Deposit { lamports: u64 },
    ///     Withdraw { lamports: u64 },
    /// }
    ///
    /// fn send_initialize_tx(
    ///     client: &RpcClient,
    ///     program_id: Pubkey,
    ///     payer: &Keypair
    /// ) -> Result<()> {
    ///
    ///     let bank_instruction = BankInstruction::Initialize;
    ///
    ///     let instruction = Instruction::new_with_borsh(
    ///         program_id,
    ///         &bank_instruction,
    ///         vec![],
    ///     );
    ///
    ///     let mut tx = Transaction::new_with_payer(&[instruction], Some(&payer.pubkey()));
    ///     let blockhash = client.get_latest_blockhash()?;
    ///     tx.sign(&[payer], blockhash);
    ///     client.send_and_confirm_transaction(&tx)?;
    ///
    ///     Ok(())
    /// }
    /// #
    /// # let client = RpcClient::new(String::new());
    /// # let program_id = Pubkey::new_unique();
    /// # let payer = Keypair::new();
    /// # send_initialize_tx(&client, program_id, &payer)?;
    /// #
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn sign<T: Signers + ?Sized>(&mut self, keypairs: &T, recent_blockhash: Hash) {
        if let Err(e) = self.try_sign(keypairs, recent_blockhash) {
            panic!("Transaction::sign failed with error {e:?}");
        }
    }

    /// Sign the transaction with a subset of required keys.
    ///
    /// Unlike [`Transaction::sign`], this method does not require all keypairs
    /// to be provided, allowing a transaction to be signed in multiple steps.
    ///
    /// It is permitted to sign a transaction with the same keypair multiple
    /// times.
    ///
    /// If `recent_blockhash` is different than recorded in the transaction message's
    /// [`recent_blockhash`] field, then the message's `recent_blockhash` will be updated
    /// to the provided `recent_blockhash`, and any prior signatures will be cleared.
    ///
    /// [`recent_blockhash`]: Message::recent_blockhash
    ///
    /// # Panics
    ///
    /// Panics when signing fails. Use [`Transaction::try_partial_sign`] to
    /// handle the error. See the documentation for
    /// [`Transaction::try_partial_sign`] for a full description of failure
    /// conditions.
    pub fn partial_sign<T: Signers + ?Sized>(&mut self, keypairs: &T, recent_blockhash: Hash) {
        if let Err(e) = self.try_partial_sign(keypairs, recent_blockhash) {
            panic!("Transaction::partial_sign failed with error {e:?}");
        }
    }

    /// Sign the transaction with a subset of required keys.
    ///
    /// This places each of the signatures created from `keypairs` in the
    /// corresponding position, as specified in the `positions` vector, in the
    /// transactions [`signatures`] field. It does not verify that the signature
    /// positions are correct.
    ///
    /// [`signatures`]: Transaction::signatures
    ///
    /// # Panics
    ///
    /// Panics if signing fails. Use [`Transaction::try_partial_sign_unchecked`]
    /// to handle the error.
    pub fn partial_sign_unchecked<T: Signers + ?Sized>(
        &mut self,
        keypairs: &T,
        positions: Vec<usize>,
        recent_blockhash: Hash,
    ) {
        if let Err(e) = self.try_partial_sign_unchecked(keypairs, positions, recent_blockhash) {
            panic!("Transaction::partial_sign_unchecked failed with error {e:?}");
        }
    }

    /// Sign the transaction, returning any errors.
    ///
    /// This method fully signs a transaction with all required signers, which
    /// must be present in the `keypairs` slice. To sign with only some of the
    /// required signers, use [`Transaction::try_partial_sign`].
    ///
    /// If `recent_blockhash` is different than recorded in the transaction message's
    /// [`recent_blockhash`] field, then the message's `recent_blockhash` will be updated
    /// to the provided `recent_blockhash`, and any prior signatures will be cleared.
    ///
    /// [`recent_blockhash`]: Message::recent_blockhash
    ///
    /// # Errors
    ///
    /// Signing will fail if some required signers are not provided in
    /// `keypairs`; or, if the transaction has previously been partially signed,
    /// some of the remaining required signers are not provided in `keypairs`.
    /// In other words, the transaction must be fully signed as a result of
    /// calling this function. The error is [`SignerError::NotEnoughSigners`].
    ///
    /// Signing will fail for any of the reasons described in the documentation
    /// for [`Transaction::try_partial_sign`].
    ///
    /// # Examples
    ///
    /// This example uses the [`solana_rpc_client`] and [`anyhow`] crates.
    ///
    /// [`solana_rpc_client`]: https://docs.rs/solana-rpc-client
    /// [`anyhow`]: https://docs.rs/anyhow
    ///
    /// ```
    /// # use solana_sdk::example_mocks::solana_rpc_client;
    /// use anyhow::Result;
    /// use borsh::{BorshSerialize, BorshDeserialize};
    /// use solana_rpc_client::rpc_client::RpcClient;
    /// use solana_sdk::{
    ///      instruction::Instruction,
    ///      message::Message,
    ///      pubkey::Pubkey,
    ///      signature::{Keypair, Signer},
    ///      transaction::Transaction,
    /// };
    ///
    /// // A custom program instruction. This would typically be defined in
    /// // another crate so it can be shared between the on-chain program and
    /// // the client.
    /// #[derive(BorshSerialize, BorshDeserialize)]
    /// enum BankInstruction {
    ///     Initialize,
    ///     Deposit { lamports: u64 },
    ///     Withdraw { lamports: u64 },
    /// }
    ///
    /// fn send_initialize_tx(
    ///     client: &RpcClient,
    ///     program_id: Pubkey,
    ///     payer: &Keypair
    /// ) -> Result<()> {
    ///
    ///     let bank_instruction = BankInstruction::Initialize;
    ///
    ///     let instruction = Instruction::new_with_borsh(
    ///         program_id,
    ///         &bank_instruction,
    ///         vec![],
    ///     );
    ///
    ///     let mut tx = Transaction::new_with_payer(&[instruction], Some(&payer.pubkey()));
    ///     let blockhash = client.get_latest_blockhash()?;
    ///     tx.try_sign(&[payer], blockhash)?;
    ///     client.send_and_confirm_transaction(&tx)?;
    ///
    ///     Ok(())
    /// }
    /// #
    /// # let client = RpcClient::new(String::new());
    /// # let program_id = Pubkey::new_unique();
    /// # let payer = Keypair::new();
    /// # send_initialize_tx(&client, program_id, &payer)?;
    /// #
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn try_sign<T: Signers + ?Sized>(
        &mut self,
        keypairs: &T,
        recent_blockhash: Hash,
    ) -> result::Result<(), SignerError> {
        self.try_partial_sign(keypairs, recent_blockhash)?;

        if !self.is_signed() {
            Err(SignerError::NotEnoughSigners)
        } else {
            Ok(())
        }
    }

    /// Sign the transaction with a subset of required keys, returning any errors.
    ///
    /// Unlike [`Transaction::try_sign`], this method does not require all
    /// keypairs to be provided, allowing a transaction to be signed in multiple
    /// steps.
    ///
    /// It is permitted to sign a transaction with the same keypair multiple
    /// times.
    ///
    /// If `recent_blockhash` is different than recorded in the transaction message's
    /// [`recent_blockhash`] field, then the message's `recent_blockhash` will be updated
    /// to the provided `recent_blockhash`, and any prior signatures will be cleared.
    ///
    /// [`recent_blockhash`]: Message::recent_blockhash
    ///
    /// # Errors
    ///
    /// Signing will fail if
    ///
    /// - The transaction's [`Message`] is malformed such that the number of
    ///   required signatures recorded in its header
    ///   ([`num_required_signatures`]) is greater than the length of its
    ///   account keys ([`account_keys`]). The error is
    ///   [`SignerError::TransactionError`] where the interior
    ///   [`TransactionError`] is [`TransactionError::InvalidAccountIndex`].
    /// - Any of the provided signers in `keypairs` is not a required signer of
    ///   the message. The error is [`SignerError::KeypairPubkeyMismatch`].
    /// - Any of the signers is a [`Presigner`], and its provided signature is
    ///   incorrect. The error is [`SignerError::PresignerError`] where the
    ///   interior [`PresignerError`] is
    ///   [`PresignerError::VerificationFailure`].
    /// - The signer is a [`RemoteKeypair`] and
    ///   - It does not understand the input provided ([`SignerError::InvalidInput`]).
    ///   - The device cannot be found ([`SignerError::NoDeviceFound`]).
    ///   - The user cancels the signing ([`SignerError::UserCancel`]).
    ///   - An error was encountered connecting ([`SignerError::Connection`]).
    ///   - Some device-specific protocol error occurs ([`SignerError::Protocol`]).
    ///   - Some other error occurs ([`SignerError::Custom`]).
    ///
    /// See the documentation for the [`solana-remote-wallet`] crate for details
    /// on the operation of [`RemoteKeypair`] signers.
    ///
    /// [`num_required_signatures`]: crate::message::MessageHeader::num_required_signatures
    /// [`account_keys`]: Message::account_keys
    /// [`Presigner`]: crate::signer::presigner::Presigner
    /// [`PresignerError`]: crate::signer::presigner::PresignerError
    /// [`PresignerError::VerificationFailure`]: crate::signer::presigner::PresignerError::VerificationFailure
    /// [`solana-remote-wallet`]: https://docs.rs/solana-remote-wallet/latest/
    /// [`RemoteKeypair`]: https://docs.rs/solana-remote-wallet/latest/solana_remote_wallet/remote_keypair/struct.RemoteKeypair.html
    pub fn try_partial_sign<T: Signers + ?Sized>(
        &mut self,
        keypairs: &T,
        recent_blockhash: Hash,
    ) -> result::Result<(), SignerError> {
        let positions = self.get_signing_keypair_positions(&keypairs.pubkeys())?;
        if positions.iter().any(|pos| pos.is_none()) {
            return Err(SignerError::KeypairPubkeyMismatch);
        }
        let positions: Vec<usize> = positions.iter().map(|pos| pos.unwrap()).collect();
        self.try_partial_sign_unchecked(keypairs, positions, recent_blockhash)
    }

    /// Sign the transaction with a subset of required keys, returning any
    /// errors.
    ///
    /// This places each of the signatures created from `keypairs` in the
    /// corresponding position, as specified in the `positions` vector, in the
    /// transactions [`signatures`] field. It does not verify that the signature
    /// positions are correct.
    ///
    /// [`signatures`]: Transaction::signatures
    ///
    /// # Errors
    ///
    /// Returns an error if signing fails.
    pub fn try_partial_sign_unchecked<T: Signers + ?Sized>(
        &mut self,
        keypairs: &T,
        positions: Vec<usize>,
        recent_blockhash: Hash,
    ) -> result::Result<(), SignerError> {
        // if you change the blockhash, you're re-signing...
        if recent_blockhash != self.message.recent_blockhash {
            self.message.recent_blockhash = recent_blockhash;
            self.signatures
                .iter_mut()
                .for_each(|signature| *signature = Signature::default());
        }

        let signatures = keypairs.try_sign_message(&self.message_data())?;
        for i in 0..positions.len() {
            self.signatures[positions[i]] = signatures[i];
        }
        Ok(())
    }

    /// Returns a signature that is not valid for signing this transaction.
    pub fn get_invalid_signature() -> Signature {
        Signature::default()
    }

    /// Verifies that all signers have signed the message.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionError::SignatureFailure`] on error.
    pub fn verify(&self) -> Result<()> {
        let message_bytes = self.message_data();
        if !self
            ._verify_with_results(&message_bytes)
            .iter()
            .all(|verify_result| *verify_result)
        {
            Err(TransactionError::SignatureFailure)
        } else {
            Ok(())
        }
    }

    /// Verify the transaction and hash its message.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionError::SignatureFailure`] on error.
    pub fn verify_and_hash_message(&self) -> Result<Hash> {
        let message_bytes = self.message_data();
        if !self
            ._verify_with_results(&message_bytes)
            .iter()
            .all(|verify_result| *verify_result)
        {
            Err(TransactionError::SignatureFailure)
        } else {
            Ok(Message::hash_raw_message(&message_bytes))
        }
    }

    /// Verifies that all signers have signed the message.
    ///
    /// Returns a vector with the length of required signatures, where each
    /// element is either `true` if that signer has signed, or `false` if not.
    pub fn verify_with_results(&self) -> Vec<bool> {
        self._verify_with_results(&self.message_data())
    }

    pub(crate) fn _verify_with_results(&self, message_bytes: &[u8]) -> Vec<bool> {
        self.signatures
            .iter()
            .zip(&self.message.account_keys)
            .map(|(signature, pubkey)| signature.verify(pubkey.as_ref(), message_bytes))
            .collect()
    }

    /// Verify the precompiled programs in this transaction.
    pub fn verify_precompiles(&self, feature_set: &feature_set::FeatureSet) -> Result<()> {
        for instruction in &self.message().instructions {
            // The Transaction may not be sanitized at this point
            if instruction.program_id_index as usize >= self.message().account_keys.len() {
                return Err(TransactionError::AccountNotFound);
            }
            let program_id = &self.message().account_keys[instruction.program_id_index as usize];

            verify_if_precompile(
                program_id,
                instruction,
                &self.message().instructions,
                feature_set,
            )
            .map_err(|_| TransactionError::InvalidAccountIndex)?;
        }
        Ok(())
    }

    /// Get the positions of the pubkeys in `account_keys` associated with signing keypairs.
    ///
    /// [`account_keys`]: Message::account_keys
    pub fn get_signing_keypair_positions(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<usize>>> {
        if self.message.account_keys.len() < self.message.header.num_required_signatures as usize {
            return Err(TransactionError::InvalidAccountIndex);
        }
        let signed_keys =
            &self.message.account_keys[0..self.message.header.num_required_signatures as usize];

        Ok(pubkeys
            .iter()
            .map(|pubkey| signed_keys.iter().position(|x| x == pubkey))
            .collect())
    }

    /// Replace all the signatures and pubkeys.
    pub fn replace_signatures(&mut self, signers: &[(Pubkey, Signature)]) -> Result<()> {
        let num_required_signatures = self.message.header.num_required_signatures as usize;
        if signers.len() != num_required_signatures
            || self.signatures.len() != num_required_signatures
            || self.message.account_keys.len() < num_required_signatures
        {
            return Err(TransactionError::InvalidAccountIndex);
        }

        signers
            .iter()
            .enumerate()
            .for_each(|(i, (pubkey, signature))| {
                self.signatures[i] = *signature;
                self.message.account_keys[i] = *pubkey;
            });

        self.verify()
    }

    pub fn is_signed(&self) -> bool {
        self.signatures
            .iter()
            .all(|signature| *signature != Signature::default())
    }
}

pub fn uses_durable_nonce(tx: &Transaction) -> Option<&CompiledInstruction> {
    let message = tx.message();
    message
        .instructions
        .get(NONCED_TX_MARKER_IX_INDEX as usize)
        .filter(|instruction| {
            // Is system program
            matches!(
                message.account_keys.get(instruction.program_id_index as usize),
                Some(program_id) if system_program::check_id(program_id)
            )
            // Is a nonce advance instruction
            && matches!(
                limited_deserialize(&instruction.data),
                Ok(SystemInstruction::AdvanceNonceAccount)
            )
            // Nonce account is writable
            && matches!(
                instruction.accounts.first(),
                Some(index) if message.is_writable(*index as usize)
            )
        })
}

#[deprecated]
pub fn get_nonce_pubkey_from_instruction<'a>(
    ix: &CompiledInstruction,
    tx: &'a Transaction,
) -> Option<&'a Pubkey> {
    ix.accounts.first().and_then(|idx| {
        let idx = *idx as usize;
        tx.message().account_keys.get(idx)
    })
}

#[cfg(test)]
mod tests {
    #![allow(deprecated)]

    use {
        super::*,
        crate::{
            hash::hash,
            instruction::AccountMeta,
            signature::{Keypair, Presigner, Signer},
            system_instruction, sysvar,
        },
        bincode::{deserialize, serialize, serialized_size},
        std::mem::size_of,
    };

    fn get_program_id(tx: &Transaction, instruction_index: usize) -> &Pubkey {
        let message = tx.message();
        let instruction = &message.instructions[instruction_index];
        instruction.program_id(&message.account_keys)
    }

    #[test]
    fn test_refs() {
        let key = Keypair::new();
        let key1 = solana_sdk::pubkey::new_rand();
        let key2 = solana_sdk::pubkey::new_rand();
        let prog1 = solana_sdk::pubkey::new_rand();
        let prog2 = solana_sdk::pubkey::new_rand();
        let instructions = vec![
            CompiledInstruction::new(3, &(), vec![0, 1]),
            CompiledInstruction::new(4, &(), vec![0, 2]),
        ];
        let tx = Transaction::new_with_compiled_instructions(
            &[&key],
            &[key1, key2],
            Hash::default(),
            vec![prog1, prog2],
            instructions,
        );
        assert!(tx.sanitize().is_ok());

        assert_eq!(tx.key(0, 0), Some(&key.pubkey()));
        assert_eq!(tx.signer_key(0, 0), Some(&key.pubkey()));

        assert_eq!(tx.key(1, 0), Some(&key.pubkey()));
        assert_eq!(tx.signer_key(1, 0), Some(&key.pubkey()));

        assert_eq!(tx.key(0, 1), Some(&key1));
        assert_eq!(tx.signer_key(0, 1), None);

        assert_eq!(tx.key(1, 1), Some(&key2));
        assert_eq!(tx.signer_key(1, 1), None);

        assert_eq!(tx.key(2, 0), None);
        assert_eq!(tx.signer_key(2, 0), None);

        assert_eq!(tx.key(0, 2), None);
        assert_eq!(tx.signer_key(0, 2), None);

        assert_eq!(*get_program_id(&tx, 0), prog1);
        assert_eq!(*get_program_id(&tx, 1), prog2);
    }

    #[test]
    fn test_refs_invalid_program_id() {
        let key = Keypair::new();
        let instructions = vec![CompiledInstruction::new(1, &(), vec![])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&key],
            &[],
            Hash::default(),
            vec![],
            instructions,
        );
        assert_eq!(tx.sanitize(), Err(SanitizeError::IndexOutOfBounds));
    }
    #[test]
    fn test_refs_invalid_account() {
        let key = Keypair::new();
        let instructions = vec![CompiledInstruction::new(1, &(), vec![2])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&key],
            &[],
            Hash::default(),
            vec![Pubkey::default()],
            instructions,
        );
        assert_eq!(*get_program_id(&tx, 0), Pubkey::default());
        assert_eq!(tx.sanitize(), Err(SanitizeError::IndexOutOfBounds));
    }

    #[test]
    fn test_sanitize_txs() {
        let key = Keypair::new();
        let id0 = Pubkey::default();
        let program_id = solana_sdk::pubkey::new_rand();
        let ix = Instruction::new_with_bincode(
            program_id,
            &0,
            vec![
                AccountMeta::new(key.pubkey(), true),
                AccountMeta::new(id0, true),
            ],
        );
        let mut tx = Transaction::new_with_payer(&[ix], Some(&key.pubkey()));
        let o = tx.clone();
        assert_eq!(tx.sanitize(), Ok(()));
        assert_eq!(tx.message.account_keys.len(), 3);

        tx = o.clone();
        tx.message.header.num_required_signatures = 3;
        assert_eq!(tx.sanitize(), Err(SanitizeError::IndexOutOfBounds));

        tx = o.clone();
        tx.message.header.num_readonly_signed_accounts = 4;
        tx.message.header.num_readonly_unsigned_accounts = 0;
        assert_eq!(tx.sanitize(), Err(SanitizeError::IndexOutOfBounds));

        tx = o.clone();
        tx.message.header.num_readonly_signed_accounts = 2;
        tx.message.header.num_readonly_unsigned_accounts = 2;
        assert_eq!(tx.sanitize(), Err(SanitizeError::IndexOutOfBounds));

        tx = o.clone();
        tx.message.header.num_readonly_signed_accounts = 0;
        tx.message.header.num_readonly_unsigned_accounts = 4;
        assert_eq!(tx.sanitize(), Err(SanitizeError::IndexOutOfBounds));

        tx = o.clone();
        tx.message.instructions[0].program_id_index = 3;
        assert_eq!(tx.sanitize(), Err(SanitizeError::IndexOutOfBounds));

        tx = o.clone();
        tx.message.instructions[0].accounts[0] = 3;
        assert_eq!(tx.sanitize(), Err(SanitizeError::IndexOutOfBounds));

        tx = o.clone();
        tx.message.instructions[0].program_id_index = 0;
        assert_eq!(tx.sanitize(), Err(SanitizeError::IndexOutOfBounds));

        tx = o.clone();
        tx.message.header.num_readonly_signed_accounts = 2;
        tx.message.header.num_readonly_unsigned_accounts = 3;
        tx.message.account_keys.resize(4, Pubkey::default());
        assert_eq!(tx.sanitize(), Err(SanitizeError::IndexOutOfBounds));

        tx = o;
        tx.message.header.num_readonly_signed_accounts = 2;
        tx.message.header.num_required_signatures = 1;
        assert_eq!(tx.sanitize(), Err(SanitizeError::IndexOutOfBounds));
    }

    fn create_sample_transaction() -> Transaction {
        let keypair = Keypair::from_bytes(&[
            255, 101, 36, 24, 124, 23, 167, 21, 132, 204, 155, 5, 185, 58, 121, 75, 156, 227, 116,
            193, 215, 38, 142, 22, 8, 14, 229, 239, 119, 93, 5, 218, 36, 100, 158, 252, 33, 161,
            97, 185, 62, 89, 99, 195, 250, 249, 187, 189, 171, 118, 241, 90, 248, 14, 68, 219, 231,
            62, 157, 5, 142, 27, 210, 117,
        ])
        .unwrap();
        let to = Pubkey::from([
            1, 1, 1, 4, 5, 6, 7, 8, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 8, 7, 6, 5, 4,
            1, 1, 1,
        ]);

        let program_id = Pubkey::from([
            2, 2, 2, 4, 5, 6, 7, 8, 9, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 9, 8, 7, 6, 5, 4,
            2, 2, 2,
        ]);
        let account_metas = vec![
            AccountMeta::new(keypair.pubkey(), true),
            AccountMeta::new(to, false),
        ];
        let instruction =
            Instruction::new_with_bincode(program_id, &(1u8, 2u8, 3u8), account_metas);
        let message = Message::new(&[instruction], Some(&keypair.pubkey()));
        let tx = Transaction::new(&[&keypair], message, Hash::default());
        tx.verify().expect("valid sample transaction signatures");
        tx
    }

    #[test]
    fn test_transaction_serialize() {
        let tx = create_sample_transaction();
        let ser = serialize(&tx).unwrap();
        let deser = deserialize(&ser).unwrap();
        assert_eq!(tx, deser);
    }

    /// Detect changes to the serialized size of payment transactions, which affects TPS.
    #[test]
    fn test_transaction_minimum_serialized_size() {
        let alice_keypair = Keypair::new();
        let alice_pubkey = alice_keypair.pubkey();
        let bob_pubkey = solana_sdk::pubkey::new_rand();
        let ix = system_instruction::transfer(&alice_pubkey, &bob_pubkey, 42);

        let expected_data_size = size_of::<u32>() + size_of::<u64>();
        assert_eq!(expected_data_size, 12);
        assert_eq!(
            ix.data.len(),
            expected_data_size,
            "unexpected system instruction size"
        );

        let expected_instruction_size = 1 + 1 + ix.accounts.len() + 1 + expected_data_size;
        assert_eq!(expected_instruction_size, 17);

        let message = Message::new(&[ix], Some(&alice_pubkey));
        assert_eq!(
            serialized_size(&message.instructions[0]).unwrap() as usize,
            expected_instruction_size,
            "unexpected Instruction::serialized_size"
        );

        let tx = Transaction::new(&[&alice_keypair], message, Hash::default());

        let len_size = 1;
        let num_required_sigs_size = 1;
        let num_readonly_accounts_size = 2;
        let blockhash_size = size_of::<Hash>();
        let expected_transaction_size = len_size
            + (tx.signatures.len() * size_of::<Signature>())
            + num_required_sigs_size
            + num_readonly_accounts_size
            + len_size
            + (tx.message.account_keys.len() * size_of::<Pubkey>())
            + blockhash_size
            + len_size
            + expected_instruction_size;
        assert_eq!(expected_transaction_size, 215);

        assert_eq!(
            serialized_size(&tx).unwrap() as usize,
            expected_transaction_size,
            "unexpected serialized transaction size"
        );
    }

    /// Detect binary changes in the serialized transaction data, which could have a downstream
    /// affect on SDKs and applications
    #[test]
    fn test_sdk_serialize() {
        assert_eq!(
            serialize(&create_sample_transaction()).unwrap(),
            vec![
                1, 120, 138, 162, 185, 59, 209, 241, 157, 71, 157, 74, 131, 4, 87, 54, 28, 38, 180,
                222, 82, 64, 62, 61, 62, 22, 46, 17, 203, 187, 136, 62, 43, 11, 38, 235, 17, 239,
                82, 240, 139, 130, 217, 227, 214, 9, 242, 141, 223, 94, 29, 184, 110, 62, 32, 87,
                137, 63, 139, 100, 221, 20, 137, 4, 5, 1, 0, 1, 3, 36, 100, 158, 252, 33, 161, 97,
                185, 62, 89, 99, 195, 250, 249, 187, 189, 171, 118, 241, 90, 248, 14, 68, 219, 231,
                62, 157, 5, 142, 27, 210, 117, 1, 1, 1, 4, 5, 6, 7, 8, 9, 9, 9, 9, 9, 9, 9, 9, 9,
                9, 9, 9, 9, 9, 9, 9, 8, 7, 6, 5, 4, 1, 1, 1, 2, 2, 2, 4, 5, 6, 7, 8, 9, 1, 1, 1, 1,
                1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 9, 8, 7, 6, 5, 4, 2, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 2, 0, 1,
                3, 1, 2, 3
            ]
        );
    }

    #[test]
    #[should_panic]
    fn test_transaction_missing_key() {
        let keypair = Keypair::new();
        let message = Message::new(&[], None);
        Transaction::new_unsigned(message).sign(&[&keypair], Hash::default());
    }

    #[test]
    #[should_panic]
    fn test_partial_sign_mismatched_key() {
        let keypair = Keypair::new();
        let fee_payer = solana_sdk::pubkey::new_rand();
        let ix = Instruction::new_with_bincode(
            Pubkey::default(),
            &0,
            vec![AccountMeta::new(fee_payer, true)],
        );
        let message = Message::new(&[ix], Some(&fee_payer));
        Transaction::new_unsigned(message).partial_sign(&[&keypair], Hash::default());
    }

    #[test]
    fn test_partial_sign() {
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let ix = Instruction::new_with_bincode(
            Pubkey::default(),
            &0,
            vec![
                AccountMeta::new(keypair0.pubkey(), true),
                AccountMeta::new(keypair1.pubkey(), true),
                AccountMeta::new(keypair2.pubkey(), true),
            ],
        );
        let message = Message::new(&[ix], Some(&keypair0.pubkey()));
        let mut tx = Transaction::new_unsigned(message);

        tx.partial_sign(&[&keypair0, &keypair2], Hash::default());
        assert!(!tx.is_signed());
        tx.partial_sign(&[&keypair1], Hash::default());
        assert!(tx.is_signed());

        let hash = hash(&[1]);
        tx.partial_sign(&[&keypair1], hash);
        assert!(!tx.is_signed());
        tx.partial_sign(&[&keypair0, &keypair2], hash);
        assert!(tx.is_signed());
    }

    #[test]
    #[should_panic]
    fn test_transaction_missing_keypair() {
        let program_id = Pubkey::default();
        let keypair0 = Keypair::new();
        let id0 = keypair0.pubkey();
        let ix = Instruction::new_with_bincode(program_id, &0, vec![AccountMeta::new(id0, true)]);
        let message = Message::new(&[ix], Some(&id0));
        Transaction::new_unsigned(message).sign(&Vec::<&Keypair>::new(), Hash::default());
    }

    #[test]
    #[should_panic]
    fn test_transaction_wrong_key() {
        let program_id = Pubkey::default();
        let keypair0 = Keypair::new();
        let wrong_id = Pubkey::default();
        let ix =
            Instruction::new_with_bincode(program_id, &0, vec![AccountMeta::new(wrong_id, true)]);
        let message = Message::new(&[ix], Some(&wrong_id));
        Transaction::new_unsigned(message).sign(&[&keypair0], Hash::default());
    }

    #[test]
    fn test_transaction_correct_key() {
        let program_id = Pubkey::default();
        let keypair0 = Keypair::new();
        let id0 = keypair0.pubkey();
        let ix = Instruction::new_with_bincode(program_id, &0, vec![AccountMeta::new(id0, true)]);
        let message = Message::new(&[ix], Some(&id0));
        let mut tx = Transaction::new_unsigned(message);
        tx.sign(&[&keypair0], Hash::default());
        assert_eq!(
            tx.message.instructions[0],
            CompiledInstruction::new(1, &0, vec![0])
        );
        assert!(tx.is_signed());
    }

    #[test]
    fn test_transaction_instruction_with_duplicate_keys() {
        let program_id = Pubkey::default();
        let keypair0 = Keypair::new();
        let id0 = keypair0.pubkey();
        let id1 = solana_sdk::pubkey::new_rand();
        let ix = Instruction::new_with_bincode(
            program_id,
            &0,
            vec![
                AccountMeta::new(id0, true),
                AccountMeta::new(id1, false),
                AccountMeta::new(id0, false),
                AccountMeta::new(id1, false),
            ],
        );
        let message = Message::new(&[ix], Some(&id0));
        let mut tx = Transaction::new_unsigned(message);
        tx.sign(&[&keypair0], Hash::default());
        assert_eq!(
            tx.message.instructions[0],
            CompiledInstruction::new(2, &0, vec![0, 1, 0, 1])
        );
        assert!(tx.is_signed());
    }

    #[test]
    fn test_try_sign_dyn_keypairs() {
        let program_id = Pubkey::default();
        let keypair = Keypair::new();
        let pubkey = keypair.pubkey();
        let presigner_keypair = Keypair::new();
        let presigner_pubkey = presigner_keypair.pubkey();

        let ix = Instruction::new_with_bincode(
            program_id,
            &0,
            vec![
                AccountMeta::new(pubkey, true),
                AccountMeta::new(presigner_pubkey, true),
            ],
        );
        let message = Message::new(&[ix], Some(&pubkey));
        let mut tx = Transaction::new_unsigned(message);

        let presigner_sig = presigner_keypair.sign_message(&tx.message_data());
        let presigner = Presigner::new(&presigner_pubkey, &presigner_sig);

        let signers: Vec<&dyn Signer> = vec![&keypair, &presigner];

        let res = tx.try_sign(&signers, Hash::default());
        assert_eq!(res, Ok(()));
        assert_eq!(tx.signatures[0], keypair.sign_message(&tx.message_data()));
        assert_eq!(tx.signatures[1], presigner_sig);

        // Wrong key should error, not panic
        let another_pubkey = solana_sdk::pubkey::new_rand();
        let ix = Instruction::new_with_bincode(
            program_id,
            &0,
            vec![
                AccountMeta::new(another_pubkey, true),
                AccountMeta::new(presigner_pubkey, true),
            ],
        );
        let message = Message::new(&[ix], Some(&another_pubkey));
        let mut tx = Transaction::new_unsigned(message);

        let res = tx.try_sign(&signers, Hash::default());
        assert!(res.is_err());
        assert_eq!(
            tx.signatures,
            vec![Signature::default(), Signature::default()]
        );
    }

    fn nonced_transfer_tx() -> (Pubkey, Pubkey, Transaction) {
        let from_keypair = Keypair::new();
        let from_pubkey = from_keypair.pubkey();
        let nonce_keypair = Keypair::new();
        let nonce_pubkey = nonce_keypair.pubkey();
        let instructions = [
            system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
            system_instruction::transfer(&from_pubkey, &nonce_pubkey, 42),
        ];
        let message = Message::new(&instructions, Some(&nonce_pubkey));
        let tx = Transaction::new(&[&from_keypair, &nonce_keypair], message, Hash::default());
        (from_pubkey, nonce_pubkey, tx)
    }

    #[test]
    fn tx_uses_nonce_ok() {
        let (_, _, tx) = nonced_transfer_tx();
        assert!(uses_durable_nonce(&tx).is_some());
    }

    #[test]
    fn tx_uses_nonce_empty_ix_fail() {
        assert!(uses_durable_nonce(&Transaction::default()).is_none());
    }

    #[test]
    fn tx_uses_nonce_bad_prog_id_idx_fail() {
        let (_, _, mut tx) = nonced_transfer_tx();
        tx.message.instructions.get_mut(0).unwrap().program_id_index = 255u8;
        assert!(uses_durable_nonce(&tx).is_none());
    }

    #[test]
    fn tx_uses_nonce_first_prog_id_not_nonce_fail() {
        let from_keypair = Keypair::new();
        let from_pubkey = from_keypair.pubkey();
        let nonce_keypair = Keypair::new();
        let nonce_pubkey = nonce_keypair.pubkey();
        let instructions = [
            system_instruction::transfer(&from_pubkey, &nonce_pubkey, 42),
            system_instruction::advance_nonce_account(&nonce_pubkey, &nonce_pubkey),
        ];
        let message = Message::new(&instructions, Some(&from_pubkey));
        let tx = Transaction::new(&[&from_keypair, &nonce_keypair], message, Hash::default());
        assert!(uses_durable_nonce(&tx).is_none());
    }

    #[test]
    fn tx_uses_ro_nonce_account() {
        let from_keypair = Keypair::new();
        let from_pubkey = from_keypair.pubkey();
        let nonce_keypair = Keypair::new();
        let nonce_pubkey = nonce_keypair.pubkey();
        let account_metas = vec![
            AccountMeta::new_readonly(nonce_pubkey, false),
            #[allow(deprecated)]
            AccountMeta::new_readonly(sysvar::recent_blockhashes::id(), false),
            AccountMeta::new_readonly(nonce_pubkey, true),
        ];
        let nonce_instruction = Instruction::new_with_bincode(
            system_program::id(),
            &system_instruction::SystemInstruction::AdvanceNonceAccount,
            account_metas,
        );
        let tx = Transaction::new_signed_with_payer(
            &[nonce_instruction],
            Some(&from_pubkey),
            &[&from_keypair, &nonce_keypair],
            Hash::default(),
        );
        assert!(uses_durable_nonce(&tx).is_none());
    }

    #[test]
    fn tx_uses_nonce_wrong_first_nonce_ix_fail() {
        let from_keypair = Keypair::new();
        let from_pubkey = from_keypair.pubkey();
        let nonce_keypair = Keypair::new();
        let nonce_pubkey = nonce_keypair.pubkey();
        let instructions = [
            system_instruction::withdraw_nonce_account(
                &nonce_pubkey,
                &nonce_pubkey,
                &from_pubkey,
                42,
            ),
            system_instruction::transfer(&from_pubkey, &nonce_pubkey, 42),
        ];
        let message = Message::new(&instructions, Some(&nonce_pubkey));
        let tx = Transaction::new(&[&from_keypair, &nonce_keypair], message, Hash::default());
        assert!(uses_durable_nonce(&tx).is_none());
    }

    #[test]
    fn get_nonce_pub_from_ix_ok() {
        let (_, nonce_pubkey, tx) = nonced_transfer_tx();
        let nonce_ix = uses_durable_nonce(&tx).unwrap();
        assert_eq!(
            get_nonce_pubkey_from_instruction(nonce_ix, &tx),
            Some(&nonce_pubkey),
        );
    }

    #[test]
    fn get_nonce_pub_from_ix_no_accounts_fail() {
        let (_, _, tx) = nonced_transfer_tx();
        let nonce_ix = uses_durable_nonce(&tx).unwrap();
        let mut nonce_ix = nonce_ix.clone();
        nonce_ix.accounts.clear();
        assert_eq!(get_nonce_pubkey_from_instruction(&nonce_ix, &tx), None,);
    }

    #[test]
    fn get_nonce_pub_from_ix_bad_acc_idx_fail() {
        let (_, _, tx) = nonced_transfer_tx();
        let nonce_ix = uses_durable_nonce(&tx).unwrap();
        let mut nonce_ix = nonce_ix.clone();
        nonce_ix.accounts[0] = 255u8;
        assert_eq!(get_nonce_pubkey_from_instruction(&nonce_ix, &tx), None,);
    }

    #[test]
    fn tx_keypair_pubkey_mismatch() {
        let from_keypair = Keypair::new();
        let from_pubkey = from_keypair.pubkey();
        let to_pubkey = Pubkey::new_unique();
        let instructions = [system_instruction::transfer(&from_pubkey, &to_pubkey, 42)];
        let mut tx = Transaction::new_with_payer(&instructions, Some(&from_pubkey));
        let unused_keypair = Keypair::new();
        let err = tx
            .try_partial_sign(&[&from_keypair, &unused_keypair], Hash::default())
            .unwrap_err();
        assert_eq!(err, SignerError::KeypairPubkeyMismatch);
    }

    #[test]
    fn test_unsized_signers() {
        fn instructions_to_tx(
            instructions: &[Instruction],
            signers: Box<dyn Signers>,
        ) -> Transaction {
            let pubkeys = signers.pubkeys();
            let first_signer = pubkeys.first().expect("should exist");
            let message = Message::new(instructions, Some(first_signer));
            Transaction::new(signers.as_ref(), message, Hash::default())
        }

        let signer: Box<dyn Signer> = Box::new(Keypair::new());
        let tx = instructions_to_tx(&[], Box::new(vec![signer]));

        assert!(tx.is_signed());
    }
}
