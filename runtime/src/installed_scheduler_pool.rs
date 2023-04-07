//! Transaction processing glue code, mainly consisting of Object-safe traits
//!
//! `trait InstalledSchedulerPool` is the most crucial piece for this whole integration.
//!
//! It lends one of pooled `trait InstalledScheduler`s out to a `Bank`, so that the ubiquitous
//! `Arc<Bank>` can conveniently work as a facade for transaction scheduling, both to higher-layer
//! subsystems (i.e. `ReplayStage` and `BankingStage`). `BankForks` is responsible for this
//! book-keeping.
//!
//! `trait InstalledScheduler` can be fed with `SanitizedTransaction`s. Then, it schedules and
//! commits those transaction execution results into the associated _bank_. That means,
//! `InstalledScheduler` and `Bank` are mutually linked to each other, resulting somewhat special
//! handling as part of their life-cycle.
//!
//! At these interfaces level, it's generally assumed that each `InstalledScheduler` is backed by
//! multiple threads for performant transaction execution and there's multiple of it inside a
//! single instance of `InstalledSchedulerPool`.
//!
//! Dynamic dispatch was inevitable, due to the need of delegating those implementations to the
//! dependent crate to cut cyclic dependency (`solana-scheduler-pool`, which in turn depends on
//! `solana-ledger`; another dependent crate of `solana-runtime`...).

use {
    crate::bank::Bank,
    log::*,
    solana_program_runtime::timings::ExecuteTimings,
    solana_scheduler::{SchedulingMode, WithSchedulingMode},
    solana_sdk::{
        slot_history::Slot,
        transaction::{Result, SanitizedTransaction},
    },
    std::{fmt::Debug, ops::Deref, sync::Arc},
};
use crate::bank_forks::BankForks;


pub trait InstalledSchedulerPool: Send + Sync + Debug {
    fn take_from_pool(&self, context: SchedulingContext) -> Box<dyn InstalledScheduler>;
    fn return_to_pool(&self, scheduler: Box<dyn InstalledScheduler>);
}

pub trait InstalledScheduler: Send + Sync + Debug {
    fn random_id(&self) -> u64;
    fn scheduler_pool(&self) -> Box<dyn InstalledSchedulerPool>;

    fn schedule_execution(&self, sanitized_tx: &SanitizedTransaction, index: usize);
    fn schedule_termination(&mut self);
    fn wait_for_termination(
        &mut self,
        from_internal: bool,
        is_restart: bool,
    ) -> Option<(ExecuteTimings, Result<()>)>;

    fn replace_scheduler_context(&self, context: SchedulingContext);
}

// somewhat arbitrarily new type just to pacify Bank's frozen_abi...
#[derive(Debug, Default)]
pub(crate) struct InstalledSchedulerBox(pub(crate) Option<Box<dyn InstalledScheduler>>);

#[cfg(RUSTC_WITH_SPECIALIZATION)]
use solana_frozen_abi::abi_example::AbiExample;

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl AbiExample for InstalledSchedulerBox {
    fn example() -> Self {
        Self(None)
    }
}

#[derive(Clone, Debug)]
pub struct SchedulingContext {
    mode: SchedulingMode,
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

    pub fn log_prefix(random_id: u64, context: Option<&Self>) -> String {
        format!(
            "id_{:016x}{}",
            random_id,
            context
                .as_ref()
                .map(|c| format!(" slot: {}, mode: {:?}", c.slot(), c.mode))
                .unwrap_or_else(|| "".into())
        )
    }

    pub fn into_bank(self) -> Option<Bank> {
        Arc::try_unwrap(self.bank).ok()
    }
}

pub(crate) struct BankWithScheduler(pub(crate) Arc<Bank>);

impl BankWithScheduler {
    pub(crate) fn new_arc(&self) -> Arc<Bank> {
        self.0.clone()
    }

    pub(crate) fn into_arc(self) -> Arc<Bank> {
        let bank = self.new_arc();
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

    fn deref(&self) -> &Bank {
        &self.0
    }
}

impl BankForks {
}

impl Bank {
    pub(crate) fn install_scheduler(&self, scheduler: Box<dyn InstalledScheduler>) {
        let mut scheduler_guard = self.scheduler.write().unwrap();
        assert!(scheduler_guard.0.replace(scheduler).is_none());
    }

    pub fn with_scheduler(&self) -> bool {
        self.scheduler.read().unwrap().0.is_some()
    }

    pub fn schedule_transaction_executions<'a>(
        &self,
        transactions: &[SanitizedTransaction],
        transaction_indexes: impl Iterator<Item = &'a usize>,
    ) {
        trace!(
            "schedule_and_commit_transactions(): {} txs",
            transactions.len()
        );

        let scheduler_guard = self.scheduler.read().unwrap();
        let scheduler = scheduler_guard.0.as_ref().unwrap();

        for (st, &i) in transactions.iter().zip(transaction_indexes) {
            scheduler.schedule_execution(st, i);
        }
    }

    pub(crate) fn schedule_termination(&self) {
        let mut scheduler_guard = self.scheduler.write().unwrap();
        if let Some(scheduler) = scheduler_guard.0.as_mut() {
            scheduler.schedule_termination();
        }
    }

    fn wait_for_scheduler<
        const VIA_DROP: bool,
        const FROM_INTERNAL: bool,
        const IS_RESTART: bool,
    >(
        &self,
    ) -> Option<(ExecuteTimings, Result<()>)> {
        let mut scheduler_guard = self.scheduler.write().unwrap();
        let current_thread_name = std::thread::current().name().unwrap().to_string();
        if VIA_DROP {
            info!(
                "wait_for_scheduler(VIA_DROP): {}",
                std::backtrace::Backtrace::force_capture()
            );
        }
        if scheduler_guard.0.is_some() {
            info!("wait_for_scheduler({VIA_DROP}): gracefully stopping bank ({})... from_internal: {FROM_INTERNAL} by {current_thread_name}", self.slot());

            let timings_and_result = scheduler_guard
                .0
                .as_mut()
                .and_then(|scheduler| scheduler.wait_for_termination(FROM_INTERNAL, IS_RESTART));
            if !IS_RESTART {
                if let Some(scheduler) = scheduler_guard.0.take() {
                    scheduler.scheduler_pool().return_to_pool(scheduler);
                }
            }
            timings_and_result
        } else {
            warn!(
                "Bank::wait_for_scheduler(via_drop: {VIA_DROP}) skipped from_internal: {FROM_INTERNAL} by {} ...",
                current_thread_name
            );
            None
        }
    }

    pub fn wait_for_completed_scheduler(&self) -> (ExecuteTimings, Result<()>) {
        let maybe_timings_and_result = self.wait_for_scheduler::<false, false, false>();
        maybe_timings_and_result.unwrap_or((ExecuteTimings::default(), Ok(())))
    }

    pub(crate) fn wait_for_completed_scheduler_via_drop(&self) -> Option<Result<()>> {
        let maybe_timings_and_result = self.wait_for_scheduler::<true, false, false>();
        maybe_timings_and_result.map(|(_timings, result)| result)
    }

    pub fn wait_for_completed_scheduler_via_internal_drop(self) {
        let maybe_timings_and_result = self.wait_for_scheduler::<true, true, false>();
        assert!(maybe_timings_and_result.is_some());
    }

    pub(crate) fn wait_for_reusable_scheduler(&self) {
        let maybe_timings_and_result = self.wait_for_scheduler::<false, false, true>();
        assert!(maybe_timings_and_result.is_none());
    }

    pub(crate) fn drop_scheduler(&mut self) {
        if self.with_scheduler() {
            if let Some(Err(err)) = self.wait_for_completed_scheduler_via_drop() {
                warn!(
                    "Bank::drop(): slot: {} discarding error from scheduler: {:?}",
                    self.slot(),
                    err
                );
            }
        }
    }
}
