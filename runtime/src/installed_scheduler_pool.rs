pub trait InstalledScheduler: Send + Sync + std::fmt::Debug {
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

    // drop with exit atomicbool integration??
}

#[derive(Debug, Default)]
struct InstalledSchedulerBox(Option<Box<dyn InstalledScheduler>>);

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl AbiExample for InstalledSchedulerBox {
    fn example() -> Self {
        Self(None)
    }
}

