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
    solana_program_runtime::timings::ExecuteTimings,
    solana_scheduler::{SchedulingMode, WithSchedulingMode},
    solana_sdk::{
        slot_history::Slot,
        transaction::{Result, SanitizedTransaction},
    },
    std::{fmt::Debug, sync::Arc},
};

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

pub(crate) struct BankWithScheduler(Arc<Bank>);

impl BankWithScheduler {
    pub(crate) fn new_arc(&self) -> Arc<Bank> {
        self.0.clone()
    }

    pub(crate) fn into_arc(self) -> Arc<Bank> {
        let s = self.new_arc();
        drop(self);
        s
    }
}

impl Drop for BankWithScheduler {
    fn drop(&mut self) {
        self.0.schedule_termination();
    }
}

impl std::ops::Deref for BankWithScheduler {
    type Target = Bank;

    fn deref(&self) -> &Bank {
        &self.0
    }
}
