//! PayTube's "settler" component for settling the final ledgers across all
//! channel participants.
//!
//! When users are finished transacting, the resulting ledger is used to craft
//! a batch of transactions to settle all state changes to the base chain
//! (Solana).
//!
//! The interesting piece here is that there can be hundreds or thousands of
//! transactions across a handful of users, but only the resulting difference
//! between their balance when the channel opened and their balance when the
//! channel is about to close are needed to create the settlement transaction.

use {
    crate::transaction::PayTubeTransaction,
    solana_client::{rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig},
    solana_sdk::{
        commitment_config::CommitmentConfig, instruction::Instruction as SolanaInstruction,
        pubkey::Pubkey, signature::Keypair, signer::Signer, system_instruction,
        transaction::Transaction as SolanaTransaction,
    },
    solana_svm::{
        transaction_processing_result::TransactionProcessingResultExtensions,
        transaction_processor::LoadAndExecuteSanitizedTransactionsOutput,
    },
    spl_associated_token_account::get_associated_token_address,
    std::collections::HashMap,
};

/// The key used for storing ledger entries.
///
/// Each entry in the ledger represents the movement of SOL or tokens between
/// two parties. The two keys of the two parties are stored in a sorted array
/// of length two, and the value's sign determines the direction of transfer.
///
/// This design allows the ledger to combine transfers from a -> b and b -> a
/// in the same entry, calculating the final delta between two parties.
///
/// Note that this design could be even _further_ optimized to minimize the
/// number of required settlement transactions in a few ways, including
/// combining transfers across parties, ignoring zero-balance changes, and
/// more. An on-chain program on the base chain could even facilitate
/// multi-party transfers, further reducing the number of required
/// settlement transactions.
#[derive(PartialEq, Eq, Hash)]
struct LedgerKey {
    mint: Option<Pubkey>,
    keys: [Pubkey; 2],
}

/// A ledger of PayTube transactions, used to deconstruct into base chain
/// transactions.
///
/// The value is stored as a signed `i128`, in order to include a sign but also
/// provide enough room to store `u64::MAX`.
struct Ledger {
    ledger: HashMap<LedgerKey, i128>,
}

impl Ledger {
    fn new(
        paytube_transactions: &[PayTubeTransaction],
        svm_output: LoadAndExecuteSanitizedTransactionsOutput,
    ) -> Self {
        let mut ledger: HashMap<LedgerKey, i128> = HashMap::new();
        paytube_transactions
            .iter()
            .zip(svm_output.processing_results)
            .for_each(|(transaction, result)| {
                // Only append to the ledger if the PayTube transaction was
                // successful.
                if result.was_processed_with_successful_result() {
                    let mint = transaction.mint;
                    let mut keys = [transaction.from, transaction.to];
                    keys.sort();
                    let amount = if keys.iter().position(|k| k.eq(&transaction.from)).unwrap() == 0
                    {
                        transaction.amount as i128
                    } else {
                        (transaction.amount as i128)
                            .checked_neg()
                            .unwrap_or_default()
                    };
                    ledger
                        .entry(LedgerKey { mint, keys })
                        .and_modify(|e| *e = e.checked_add(amount).unwrap())
                        .or_insert(amount);
                }
            });
        Self { ledger }
    }

    fn generate_base_chain_instructions(&self) -> Vec<SolanaInstruction> {
        self.ledger
            .iter()
            .map(|(key, amount)| {
                let (from, to, amount) = if *amount < 0 {
                    (key.keys[1], key.keys[0], (amount * -1) as u64)
                } else {
                    (key.keys[0], key.keys[1], *amount as u64)
                };
                if let Some(mint) = key.mint {
                    let source_pubkey = get_associated_token_address(&from, &mint);
                    let destination_pubkey = get_associated_token_address(&to, &mint);
                    return spl_token::instruction::transfer(
                        &spl_token::id(),
                        &source_pubkey,
                        &destination_pubkey,
                        &from,
                        &[],
                        amount,
                    )
                    .unwrap();
                }
                system_instruction::transfer(&from, &to, amount)
            })
            .collect::<Vec<_>>()
    }
}

const CHUNK_SIZE: usize = 10;

/// PayTube final transaction settler.
pub struct PayTubeSettler<'a> {
    instructions: Vec<SolanaInstruction>,
    keys: &'a [Keypair],
    rpc_client: &'a RpcClient,
}

impl<'a> PayTubeSettler<'a> {
    /// Create a new instance of a `PayTubeSettler` by tallying up all
    /// transfers into a ledger.
    pub fn new(
        rpc_client: &'a RpcClient,
        paytube_transactions: &[PayTubeTransaction],
        svm_output: LoadAndExecuteSanitizedTransactionsOutput,
        keys: &'a [Keypair],
    ) -> Self {
        // Build the ledger from the processed PayTube transactions.
        let ledger = Ledger::new(paytube_transactions, svm_output);

        // Build the Solana instructions from the ledger.
        let instructions = ledger.generate_base_chain_instructions();

        Self {
            instructions,
            keys,
            rpc_client,
        }
    }

    /// Count how many settlement transactions are estimated to be required.
    pub(crate) fn num_transactions(&self) -> usize {
        self.instructions.len().div_ceil(CHUNK_SIZE)
    }

    /// Settle the payment channel results to the Solana blockchain.
    pub fn process_settle(&self) {
        let recent_blockhash = self.rpc_client.get_latest_blockhash().unwrap();
        self.instructions.chunks(CHUNK_SIZE).for_each(|chunk| {
            let transaction = SolanaTransaction::new_signed_with_payer(
                chunk,
                Some(&self.keys[0].pubkey()),
                self.keys,
                recent_blockhash,
            );
            self.rpc_client
                .send_and_confirm_transaction_with_spinner_and_config(
                    &transaction,
                    CommitmentConfig::processed(),
                    RpcSendTransactionConfig {
                        skip_preflight: true,
                        ..Default::default()
                    },
                )
                .unwrap();
        });
    }
}
