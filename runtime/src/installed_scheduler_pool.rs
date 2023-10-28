//! Currently, there are only two things: minimal InstalledScheduler trait and an auxiliary type
//! called BankWithScheduler.. This file will be populated by later PRs to align with the filename.

use {
    crate::bank::Bank,
    log::*,
    solana_program_runtime::timings::ExecuteTimings,
    solana_sdk::{
        hash::Hash,
        transaction::{Result, SanitizedTransaction},
    },
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
    // Calling this is illegal as soon as wait_for_termination is called.
    fn schedule_execution<'a>(
        &'a self,
        transaction_with_index: &'a (&'a SanitizedTransaction, usize),
    );

    #[must_use]
    fn wait_for_termination(&mut self, reason: &WaitReason) -> Option<ResultWithTimings>;
}

pub type DefaultInstalledSchedulerBox = Box<dyn InstalledScheduler>;

pub type ResultWithTimings = (Result<()>, ExecuteTimings);

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum WaitReason {
    // most normal termination waiting mode; couldn't be done implicitly inside Bank::freeze() -> () to return
    // the result and timing in some way to higher-layer subsystems;
    TerminatedToFreeze,
    // just behaves like TerminatedToFreeze but hint that this is called from Drop::drop().
    DroppedFromBankForks,
    // scheduler is paused without being returned to the pool to collect ResultWithTimings later.
    PausedForRecentBlockhash,
}

impl WaitReason {
    pub fn is_paused(&self) -> bool {
        match self {
            WaitReason::PausedForRecentBlockhash => true,
            WaitReason::TerminatedToFreeze | WaitReason::DroppedFromBankForks => false,
        }
    }
}

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

    pub fn register_tick(&self, hash: &Hash) {
        self.inner.bank.register_tick(hash, &self.inner.scheduler);
    }

    pub fn fill_bank_with_ticks_for_tests(&self) {
        self.do_fill_bank_with_ticks_for_tests(&self.inner.scheduler);
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

    // take needless &mut only to communicate its semantic mutability to humans...
    #[cfg(feature = "dev-context-only-utils")]
    pub fn drop_scheduler(&mut self) {
        self.inner.drop_scheduler();
    }

    pub(crate) fn wait_for_paused_scheduler(bank: &Bank, scheduler: &InstalledSchedulerRwLock) {
        let maybe_result_with_timings = BankWithSchedulerInner::wait_for_scheduler(
            bank,
            scheduler,
            WaitReason::PausedForRecentBlockhash,
        );
        assert!(
            maybe_result_with_timings.is_none(),
            "Premature result was returned from scheduler after paused"
        );
    }

    #[must_use]
    pub fn wait_for_completed_scheduler(&self) -> Option<ResultWithTimings> {
        BankWithSchedulerInner::wait_for_scheduler(
            &self.inner.bank,
            &self.inner.scheduler,
            WaitReason::TerminatedToFreeze,
        )
    }

    pub const fn no_scheduler_available() -> InstalledSchedulerRwLock {
        RwLock::new(None)
    }
}

impl BankWithSchedulerInner {
    #[must_use]
    fn wait_for_completed_scheduler_from_drop(&self) -> Option<ResultWithTimings> {
        Self::wait_for_scheduler(
            &self.bank,
            &self.scheduler,
            WaitReason::DroppedFromBankForks,
        )
    }

    #[must_use]
    fn wait_for_scheduler(
        bank: &Bank,
        scheduler: &InstalledSchedulerRwLock,
        reason: WaitReason,
    ) -> Option<ResultWithTimings> {
        debug!(
            "wait_for_scheduler(slot: {}, reason: {:?}): started...",
            bank.slot(),
            reason,
        );

        let mut scheduler = scheduler.write().unwrap();
        let result_with_timings = if scheduler.is_some() {
            let result_with_timings = scheduler
                .as_mut()
                .and_then(|scheduler| scheduler.wait_for_termination(&reason));
            if !reason.is_paused() {
                drop(scheduler.take().expect("scheduler after waiting"));
            }
            result_with_timings
        } else {
            None
        };
        debug!(
            "wait_for_scheduler(slot: {}, reason: {:?}): finished with: {:?}...",
            bank.slot(),
            reason,
            result_with_timings.as_ref().map(|(result, _)| result),
        );

        result_with_timings
    }

    fn drop_scheduler(&self) {
        if std::thread::panicking() {
            error!(
                "BankWithSchedulerInner::drop_scheduler(): slot: {} skipping due to already panicking...",
                self.bank.slot(),
            );
            return;
        }

        // There's no guarantee ResultWithTimings is available or not at all when being dropped.
        if let Some(Err(err)) = self
            .wait_for_completed_scheduler_from_drop()
            .map(|(result, _timings)| result)
        {
            warn!(
                "BankWithSchedulerInner::drop_scheduler(): slot: {} discarding error from scheduler: {:?}",
                self.bank.slot(),
                err,
            );
        }
    }
}

impl Drop for BankWithSchedulerInner {
    fn drop(&mut self) {
        self.drop_scheduler();
    }
}

impl Deref for BankWithScheduler {
    type Target = Arc<Bank>;

    fn deref(&self) -> &Self::Target {
        &self.inner.bank
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            bank::test_utils::goto_end_of_slot_with_scheduler,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
        },
        assert_matches::assert_matches,
        mockall::Sequence,
        solana_sdk::system_transaction,
    };

    fn setup_mocked_scheduler_with_extra(
        wait_reasons: impl Iterator<Item = WaitReason>,
        f: Option<impl Fn(&mut MockInstalledScheduler)>,
    ) -> DefaultInstalledSchedulerBox {
        let mut mock = MockInstalledScheduler::new();
        let mut seq = Sequence::new();

        for wait_reason in wait_reasons {
            mock.expect_wait_for_termination()
                .with(mockall::predicate::eq(wait_reason))
                .times(1)
                .in_sequence(&mut seq)
                .returning(move |_| {
                    if wait_reason.is_paused() {
                        None
                    } else {
                        Some((Ok(()), ExecuteTimings::default()))
                    }
                });
        }

        if let Some(f) = f {
            f(&mut mock);
        }

        Box::new(mock)
    }

    fn setup_mocked_scheduler(
        wait_reasons: impl Iterator<Item = WaitReason>,
    ) -> DefaultInstalledSchedulerBox {
        setup_mocked_scheduler_with_extra(
            wait_reasons,
            None::<fn(&mut MockInstalledScheduler) -> ()>,
        )
    }

    #[test]
    fn test_scheduler_normal_termination() {
        solana_logger::setup();

        let bank = Arc::new(Bank::default_for_tests());
        let bank = BankWithScheduler::new(
            bank,
            Some(setup_mocked_scheduler(
                [WaitReason::TerminatedToFreeze].into_iter(),
            )),
        );
        assert!(bank.has_installed_scheduler());
        assert_matches!(bank.wait_for_completed_scheduler(), Some(_));

        // Repeating to call wait_for_completed_scheduler() is okay with no ResultWithTimings being
        // returned.
        assert!(!bank.has_installed_scheduler());
        assert_matches!(bank.wait_for_completed_scheduler(), None);
    }

    #[test]
    fn test_no_scheduler_termination() {
        solana_logger::setup();

        let bank = Arc::new(Bank::default_for_tests());
        let bank = BankWithScheduler::new_without_scheduler(bank);

        // Calling wait_for_completed_scheduler() is noop, when no scheduler is installed.
        assert!(!bank.has_installed_scheduler());
        assert_matches!(bank.wait_for_completed_scheduler(), None);
    }

    #[test]
    fn test_scheduler_termination_from_drop() {
        solana_logger::setup();

        let bank = Arc::new(Bank::default_for_tests());
        let bank = BankWithScheduler::new(
            bank,
            Some(setup_mocked_scheduler(
                [WaitReason::DroppedFromBankForks].into_iter(),
            )),
        );
        drop(bank);
    }

    #[test]
    fn test_scheduler_pause() {
        solana_logger::setup();

        let bank = Arc::new(crate::bank::tests::create_simple_test_bank(42));
        let bank = BankWithScheduler::new(
            bank,
            Some(setup_mocked_scheduler(
                [
                    WaitReason::PausedForRecentBlockhash,
                    WaitReason::TerminatedToFreeze,
                ]
                .into_iter(),
            )),
        );
        goto_end_of_slot_with_scheduler(&bank);
        assert_matches!(bank.wait_for_completed_scheduler(), Some(_));
    }

    #[test]
    fn test_schedule_executions() {
        solana_logger::setup();

        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10_000);
        let tx0 = SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
            &mint_keypair,
            &solana_sdk::pubkey::new_rand(),
            2,
            genesis_config.hash(),
        ));
        let bank = Arc::new(Bank::new_for_tests(&genesis_config));
        let mocked_scheduler = setup_mocked_scheduler_with_extra(
            [WaitReason::DroppedFromBankForks].into_iter(),
            Some(|mocked: &mut MockInstalledScheduler| {
                mocked
                    .expect_schedule_execution()
                    .times(1)
                    .returning(|(_, _)| ());
            }),
        );

        let bank = BankWithScheduler::new(bank, Some(mocked_scheduler));
        bank.schedule_transaction_executions([(&tx0, &0)].into_iter());
    }
}
