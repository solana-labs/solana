use std::sync::Arc;
use solana_runtime::bank::Bank;

struct TransactionRunner(Arc<Bank>, solana_poh::poh_recorder::PohRecorder);
