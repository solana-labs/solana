//! Currently, there are only two things: minimal InstalledScheduler trait and an auxiliary type
//! called BankWithScheduler.. This file will be populated by later PRs to align with the filename.

use {
    crate::bank::Bank,
    log::*,
    solana_sdk::transaction::SanitizedTransaction,
    std::{
        fmt::Debug,
        ops::Deref,
        sync::{Arc, RwLock},
    },
};
#[cfg(feature = "dev-context-only-utils")]
use {mockall::automock, qualifier_attr::qualifiers};

#[cfg_attr(feature = "dev-context-only-utils", automock)]
// suppress false clippy complaints arising from mockall-derive:
//   warning: `#[must_use]` has no effect when applied to a struct field
//   warning: the following explicit lifetimes could be elided: 'a
#[cfg_attr(
    feature = "dev-context-only-utils",
    allow(unused_attributes, clippy::needless_lifetimes)
)]
pub trait InstalledScheduler: Send + Sync + Debug + 'static {
    fn schedule_execution<'a>(
        &'a self,
        transaction_with_index: &'a (&'a SanitizedTransaction, usize),
    );
}

pub type DefaultInstalledSchedulerBox = Box<dyn InstalledScheduler>;

/// Very thin wrapper around Arc<Bank>
///
/// It brings type-safety against accidental mixing of bank and scheduler with different slots,
/// which is a pretty dangerous condition. Also, it guarantees to call wait_for_termination() via
/// ::drop() inside BankForks::set_root()'s pruning, perfectly matching to Arc<Bank>'s lifetime by
/// piggybacking on the pruning.
///
/// Semantically, a scheduler is tightly coupled with a particular bank. But scheduler wasn't put
/// into Bank fields to avoid circular-references (a scheduler needs to refer to its accompanied
/// Arc<Bank>). BankWithScheduler behaves almost like Arc<Bank>. It only adds a few of transaction
/// scheduling and scheduler management functions. For this reason, `bank` variable names should be
/// used for `BankWithScheduler` across codebase.
///
/// BankWithScheduler even implements Deref for convenience. And Clone is omitted to implement to
/// avoid ambiguity as to which to clone: BankWithScheduler or Arc<Bank>. Use
/// clone_without_scheduler() for Arc<Bank>. Otherwise, use clone_with_scheduler() (this should be
/// unusual outside scheduler code-path)
#[derive(Debug)]
pub struct BankWithScheduler {
    inner: Arc<BankWithSchedulerInner>,
}

#[derive(Debug)]
pub struct BankWithSchedulerInner {
    bank: Arc<Bank>,
    scheduler: InstalledSchedulerRwLock,
}
pub type InstalledSchedulerRwLock = RwLock<Option<DefaultInstalledSchedulerBox>>;

impl BankWithScheduler {
    #[cfg_attr(feature = "dev-context-only-utils", qualifiers(pub))]
    pub(crate) fn new(bank: Arc<Bank>, scheduler: Option<DefaultInstalledSchedulerBox>) -> Self {
        Self {
            inner: Arc::new(BankWithSchedulerInner {
                bank,
                scheduler: RwLock::new(scheduler),
            }),
        }
    }

    pub fn new_without_scheduler(bank: Arc<Bank>) -> Self {
        Self::new(bank, None)
    }

    pub fn clone_with_scheduler(&self) -> BankWithScheduler {
        BankWithScheduler {
            inner: self.inner.clone(),
        }
    }

    pub fn clone_without_scheduler(&self) -> Arc<Bank> {
        self.inner.bank.clone()
    }

    pub fn has_installed_scheduler(&self) -> bool {
        self.inner.scheduler.read().unwrap().is_some()
    }

    // 'a is needed; anonymous_lifetime_in_impl_trait isn't stabilized yet...
    pub fn schedule_transaction_executions<'a>(
        &self,
        transactions_with_indexes: impl ExactSizeIterator<Item = (&'a SanitizedTransaction, &'a usize)>,
    ) {
        trace!(
            "schedule_transaction_executions(): {} txs",
            transactions_with_indexes.len()
        );

        let scheduler_guard = self.inner.scheduler.read().unwrap();
        let scheduler = scheduler_guard.as_ref().unwrap();

        for (sanitized_transaction, &index) in transactions_with_indexes {
            scheduler.schedule_execution(&(sanitized_transaction, index));
        }
    }

    pub const fn no_scheduler_available() -> InstalledSchedulerRwLock {
        RwLock::new(None)
    }
}

impl Deref for BankWithScheduler {
    type Target = Arc<Bank>;

    fn deref(&self) -> &Self::Target {
        &self.inner.bank
    }
}
