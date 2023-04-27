//! Transaction processing glue code, mainly consisting of Object-safe traits
//!
//! `trait InstalledSchedulerPool` is the most crucial piece of code for this whole integration.
//!
//! It lends one of pooled `trait InstalledScheduler`s out to a `Bank`, so that the ubiquitous
//! `Arc<Bank>` can conveniently work as a facade for transaction scheduling, to higher-layer
//! subsystems (i.e. both `ReplayStage` and `BankingStage`). `BankForks`/`BankWithScheduler` and
//! some `Bank` methods are responsible for this book-keeping, including returning the scheduler
//! from the bank to the pool after use.
//!
//! `trait InstalledScheduler` can be fed with `SanitizedTransaction`s. Then, it schedules and
//! commits those transaction execution results into the associated _bank_. That means,
//! `InstalledScheduler` and `Bank` are mutually linked to each other, resulting in somewhat
//! special handling as part of their life-cycle.
//!
//! Albeit being at this abstract interface level, it's generally assumed that each
//! `InstalledScheduler` is backed by multiple threads for performant transaction execution and
//! there're multiple independent schedulers inside a single instance of `InstalledSchedulerPool`.
//!
//! Dynamic dispatch was inevitable, due to the need of delegating those implementations to the
//! dependent crate (`solana-scheduler-pool`, which in turn depends on `solana-ledger`; another
//! dependent crate of `solana-runtime`...), while cutting cyclic dependency.

#[cfg(any(test, feature = "test-in-workspace"))]
use mockall::automock;
use {
    crate::{bank::Bank, bank_forks::BankForks},
    log::*,
    solana_program_runtime::timings::ExecuteTimings,
    solana_scheduler::{SchedulingMode, WithSchedulingMode},
    solana_sdk::{
        slot_history::Slot,
        transaction::{Result, SanitizedTransaction},
    },
    std::{fmt::Debug, ops::Deref, sync::Arc},
};

// Send + Sync is needed to be a field of BankForks
#[cfg_attr(any(test, feature = "test-in-workspace"), automock)]
pub trait InstalledSchedulerPool: Send + Sync + Debug {
    fn take_from_pool(&self, context: SchedulingContext) -> InstalledSchedulerBox;
    fn return_to_pool(&self, scheduler: InstalledSchedulerBox);
}

#[cfg_attr(any(test, feature = "test-in-workspace"), automock)]
// suppress false clippy complaints arising from mockall-derive:
//   warning: `#[must_use]` has no effect when applied to a struct field
#[allow(unused_attributes)]
// Send + Sync is needed to be a field of Bank
pub trait InstalledScheduler: Send + Sync + Debug {
    fn scheduler_id(&self) -> SchedulerId;
    fn scheduler_pool(&self) -> InstalledSchedulerPoolArc;

    // Calling this is illegal as soon as schedule_termiantion is called on &self.
    fn schedule_execution(&self, sanitized_tx: &SanitizedTransaction, index: usize);

    // This optionally signals scheduling termination request to the scheduler.
    // This is subtle but important, to break circular dependency of Arc<Bank> => Scheduler =>
    // SchedulingContext => Arc<Bank> in the middle of the tear-down process, otherwise it would
    // prevent Bank::drop()'s last resort scheduling termination attempt indefinitely
    fn schedule_termination(&mut self);

    #[must_use]
    fn wait_for_termination(&mut self, reason: &WaitReason) -> Option<ResultWithTimings>;

    // suppress false clippy complaints arising from mockall-derive:
    //   warning: the following explicit lifetimes could be elided: 'a
    #[allow(clippy::needless_lifetimes)]
    fn scheduling_context<'a>(&'a self) -> Option<&'a SchedulingContext>;
    fn replace_scheduling_context(&mut self, context: SchedulingContext);
}

pub type InstalledSchedulerPoolArc = Arc<dyn InstalledSchedulerPool>;

pub type SchedulerId = u64;

pub type ResultWithTimings = (Result<()>, ExecuteTimings);

#[derive(Debug, PartialEq, Eq)]
pub enum WaitReason {
    // most normal termination waiting mode; couldn't be done implicitly inside Bank::freeze() -> () to return
    // the result and timing in some way to higher-layer subsystems;
    TerminatedToFreeze,
    TerminatedFromBankDrop,
    TerminatedInternallyByScheduler,
    // scheduler will be restarted without being returned to pool in order to reuse it immediately.
    ReinitializedForRecentBlockhash,
}

pub type InstalledSchedulerBox = Box<dyn InstalledScheduler>;
// somewhat arbitrary new type just to pacify Bank's frozen_abi...
#[derive(Debug, Default)]
pub(crate) struct InstalledSchedulerBoxInBank(Option<InstalledSchedulerBox>);

#[cfg(RUSTC_WITH_SPECIALIZATION)]
use solana_frozen_abi::abi_example::AbiExample;

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl AbiExample for InstalledSchedulerBoxInBank {
    fn example() -> Self {
        Self(None)
    }
}

#[derive(Clone, Debug)]
pub struct SchedulingContext {
    mode: SchedulingMode,
    // Intentionally not using Weak<Bank> for performance reasons
    bank: Arc<Bank>,
}

impl WithSchedulingMode for SchedulingContext {
    fn mode(&self) -> SchedulingMode {
        self.mode
    }
}

impl SchedulingContext {
    pub fn new(mode: SchedulingMode, bank: Arc<Bank>) -> Self {
        Self { mode, bank }
    }

    pub fn bank(&self) -> &Arc<Bank> {
        &self.bank
    }

    pub fn slot(&self) -> Slot {
        self.bank().slot()
    }

    pub fn log_prefix(context: Option<&Self>, scheduler_id: SchedulerId) -> String {
        const BITS_PER_HEX_DIGIT: usize = 4;

        format!(
            "sch_{:0width$x}{}: ",
            scheduler_id,
            context
                .map(|c| format!("(slot:{}/mode:{:?})", c.slot(), c.mode()))
                .unwrap_or_else(|| "(?)".into()),
            width = SchedulerId::BITS as usize / BITS_PER_HEX_DIGIT,
        )
    }
}

// tiny wrapper to ensure to call schedule_termination() via ::drop() inside
// BankForks::set_root()'s pruning.
pub(crate) struct BankWithScheduler(Arc<Bank>);

impl BankWithScheduler {
    pub(crate) fn new(bank: Arc<Bank>) -> Self {
        Self(bank)
    }

    pub(crate) fn bank_cloned(&self) -> Arc<Bank> {
        self.0.clone()
    }

    pub(crate) fn bank(&self) -> &Arc<Bank> {
        &self.0
    }

    pub(crate) fn into_bank(self) -> Arc<Bank> {
        let bank = self.bank_cloned();
        drop(self);
        bank
    }
}

impl Drop for BankWithScheduler {
    fn drop(&mut self) {
        self.0.schedule_termination();
    }
}

impl Deref for BankWithScheduler {
    type Target = Bank;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl BankForks {
    pub fn install_scheduler_pool(&mut self, pool: InstalledSchedulerPoolArc) {
        info!("Installed new scheduler_pool into bank_forks: {:?}", pool);
        assert!(self.scheduler_pool.replace(pool).is_none());
    }

    pub(crate) fn install_scheduler_into_bank(&self, bank: &Arc<Bank>) {
        if let Some(scheduler_pool) = &self.scheduler_pool {
            let context = SchedulingContext::new(SchedulingMode::BlockVerification, bank.clone());
            bank.install_scheduler(scheduler_pool.take_from_pool(context));
        }
    }
}

impl Bank {
    pub fn install_scheduler(&self, scheduler: InstalledSchedulerBox) {
        let mut scheduler_guard = self.scheduler.write().expect("not poisoned");
        assert!(scheduler_guard.0.replace(scheduler).is_none());
    }

    pub fn with_scheduler(&self) -> bool {
        self.scheduler.read().expect("not poisoned").0.is_some()
    }

    pub fn with_scheduling_context(&self) -> bool {
        self.scheduler
            .read()
            .expect("not poisoned")
            .0
            .as_ref()
            .and_then(|scheduler| scheduler.scheduling_context())
            .is_some()
    }

    pub fn schedule_transaction_executions<'a>(
        &self,
        transactions: &[SanitizedTransaction],
        transaction_indexes: impl Iterator<Item = &'a usize>,
    ) {
        trace!(
            "schedule_transaction_executions(): {} txs",
            transactions.len()
        );

        let scheduler_guard = self.scheduler.read().expect("not poisoned");
        let scheduler = scheduler_guard.0.as_ref().expect("active scheduler");

        for (sanitized_transaction, &index) in transactions.iter().zip(transaction_indexes) {
            scheduler.schedule_execution(sanitized_transaction, index);
        }
    }

    fn schedule_termination(&self) {
        let mut scheduler_guard = self.scheduler.write().expect("not poisoned");
        if let Some(scheduler) = scheduler_guard.0.as_mut() {
            scheduler.schedule_termination();
        }
    }

    #[must_use]
    fn wait_for_scheduler(&self, reason: WaitReason) -> Option<ResultWithTimings> {
        debug!(
            "wait_for_scheduler(slot: {}, reason: {reason:?}): started...",
            self.slot()
        );

        let mut scheduler_guard = self.scheduler.write().expect("not poisoned");
        let scheduler = &mut scheduler_guard.0;

        let result_with_timings = if scheduler.is_some() {
            let result_with_timings = scheduler
                .as_mut()
                .and_then(|scheduler| scheduler.wait_for_termination(&reason));
            if !matches!(reason, WaitReason::ReinitializedForRecentBlockhash) {
                let scheduler = scheduler.take().expect("scheduler after waiting");
                scheduler.scheduler_pool().return_to_pool(scheduler);
            }
            result_with_timings
        } else {
            None
        };
        debug!(
            "wait_for_scheduler(slot: {}, reason: {reason:?}): finished with: {:?}...",
            self.slot(),
            result_with_timings.as_ref().map(|(result, _)| result)
        );

        result_with_timings
    }

    #[must_use]
    pub fn wait_for_completed_scheduler(&self) -> Option<ResultWithTimings> {
        self.wait_for_scheduler(WaitReason::TerminatedToFreeze)
    }

    #[must_use]
    fn wait_for_completed_scheduler_from_drop(&self) -> Option<Result<()>> {
        let maybe_timings_and_result = self.wait_for_scheduler(WaitReason::TerminatedFromBankDrop);
        maybe_timings_and_result.map(|(result, _timings)| result)
    }

    pub fn wait_for_completed_scheduler_from_scheduler_drop(self) {
        let maybe_timings_and_result =
            self.wait_for_scheduler(WaitReason::TerminatedInternallyByScheduler);
        assert!(maybe_timings_and_result.is_some());
    }

    pub(crate) fn wait_for_reusable_scheduler(&self) {
        let maybe_timings_and_result =
            self.wait_for_scheduler(WaitReason::ReinitializedForRecentBlockhash);
        assert!(maybe_timings_and_result.is_none());
    }

    pub(crate) fn drop_scheduler(&mut self) {
        if self.with_scheduler() {
            if let Some(Err(err)) = self.wait_for_completed_scheduler_from_drop() {
                warn!(
                    "Bank::drop(): slot: {} discarding error from scheduler: {err:?}",
                    self.slot(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            bank::test_utils::goto_end_of_slot,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
        },
        mockall::Sequence,
        solana_sdk::system_transaction,
    };

    fn setup_mocked_scheduler_pool(seq: &mut Sequence) -> InstalledSchedulerPoolArc {
        let mut mock = MockInstalledSchedulerPool::new();
        mock.expect_return_to_pool()
            .times(1)
            .in_sequence(seq)
            .returning(|_| ());
        Arc::new(mock)
    }

    fn setup_mocked_scheduler_with_extra(
        wait_reasons: impl Iterator<Item = WaitReason>,
        f: Option<impl Fn(&mut MockInstalledScheduler)>,
    ) -> InstalledSchedulerBox {
        let mut mock = MockInstalledScheduler::new();
        let mut seq = Sequence::new();

        for wait_reason in wait_reasons {
            mock.expect_wait_for_termination()
                .with(mockall::predicate::eq(wait_reason))
                .times(1)
                .in_sequence(&mut seq)
                .returning(|_| None);
        }

        mock.expect_scheduler_pool()
            .times(1)
            .in_sequence(&mut seq)
            .returning(move || setup_mocked_scheduler_pool(&mut seq));
        if let Some(f) = f {
            f(&mut mock);
        }

        Box::new(mock)
    }

    fn setup_mocked_scheduler(
        wait_reasons: impl Iterator<Item = WaitReason>,
    ) -> InstalledSchedulerBox {
        setup_mocked_scheduler_with_extra(
            wait_reasons,
            None::<fn(&mut MockInstalledScheduler) -> ()>,
        )
    }

    #[test]
    fn test_scheduling_context() {
        solana_logger::setup();

        let bank = Arc::new(Bank::default_for_tests());
        let context = &SchedulingContext::new(SchedulingMode::BlockVerification, bank);
        assert_eq!(context.slot(), 0);
        assert_eq!(
            SchedulingContext::log_prefix(Some(context), 3),
            "sch_0000000000000003(slot:0/mode:BlockVerification): "
        );
        assert_eq!(
            SchedulingContext::log_prefix(None, 3),
            "sch_0000000000000003(?): "
        );
    }

    #[test]
    fn test_scheduler_normal_termination() {
        solana_logger::setup();

        let bank = Bank::default_for_tests();
        bank.install_scheduler(setup_mocked_scheduler(
            [WaitReason::TerminatedToFreeze].into_iter(),
        ));
        assert!(bank.wait_for_completed_scheduler().is_none());
    }

    #[test]
    fn test_scheduler_termination_from_drop() {
        solana_logger::setup();

        let bank = Bank::default_for_tests();
        bank.install_scheduler(setup_mocked_scheduler(
            [WaitReason::TerminatedFromBankDrop].into_iter(),
        ));
        drop(bank);
    }

    #[test]
    fn test_scheduler_reinitialization() {
        solana_logger::setup();

        let mut bank = crate::bank::tests::create_simple_test_bank(42);
        bank.install_scheduler(setup_mocked_scheduler(
            [
                WaitReason::ReinitializedForRecentBlockhash,
                WaitReason::TerminatedFromBankDrop,
            ]
            .into_iter(),
        ));
        goto_end_of_slot(&mut bank);
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
        let bank = &Arc::new(Bank::new_for_tests(&genesis_config));
        let mocked_scheduler = setup_mocked_scheduler_with_extra(
            [WaitReason::TerminatedFromBankDrop].into_iter(),
            Some(|mocked: &mut MockInstalledScheduler| {
                mocked
                    .expect_schedule_execution()
                    .times(1)
                    .returning(|_, _| ());
            }),
        );

        bank.install_scheduler(mocked_scheduler);
        bank.schedule_transaction_executions(&[tx0], [0].iter());
    }
}
