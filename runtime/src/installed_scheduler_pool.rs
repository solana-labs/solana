//! Currently, there's only one auxiliary type called BankWithScheduler.. This file will be
//! populated by later PRs to align with the filename.

#[cfg(feature = "dev-context-only-utils")]
use qualifier_attr::qualifiers;
use {
    crate::bank::Bank,
    std::{
        fmt::Debug,
        ops::Deref,
        sync::{Arc, RwLock},
    },
};

// currently dummy type; will be replaced with the introduction of real type by upcoming pr...
pub type DefaultInstalledSchedulerBox = ();

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
    #[allow(dead_code)]
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
