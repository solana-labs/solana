#![allow(rustdoc::private_intra_doc_links)]
//! The task (transaction) scheduling code for the unified scheduler
//!
//! ### High-level API and design
//!
//! The most important type is [`SchedulingStateMachine`]. It takes new tasks (= transactons) and
//! may return back them if runnable via
//! [`::schedule_task()`](SchedulingStateMachine::schedule_task) while maintaining the account
//! readonly/writable lock rules. Those returned runnable tasks are guaranteed to be safe to
//! execute in parallel. Lastly, `SchedulingStateMachine` should be notified about the completion
//! of the exeuciton via [`::deschedule_task()`](SchedulingStateMachine::deschedule_task), so that
//! conflicting tasks can be returned from
//! [`::schedule_unblocked_task()`](SchedulingStateMachine::schedule_unblocked_task) as
//! newly-unblocked runnable ones.
//!
//! The design principle of this crate (`solana-unified-scheduler-logic`) is simplicity for the
//! separation of concern. It is interacted only with a few of its public API by
//! `solana-unified-scheduler-pool`. This crate doesn't know about banks, slots, solana-runtime,
//! threads, crossbeam-channel at all. Becasue of this, it's deterministic, easy-to-unit-test, and
//! its perf footprint is well understood. It really focuses on its single job: sorting
//! transactions in executable order.
//!
//! ### Algorithm
//!
//! The algorithm can be said it's based on per-address FIFO queues, which are updated every time
//! both new task is coming (= called _scheduling_) and runnable (= _post-scheduling_) task is
//! finished (= called _descheduling_).
//!
//! For the _non-conflicting scheduling_ case, the story is very simple; it just remembers that all
//! of accessed addresses are write-locked or read-locked with the number of active (=
//! _currently-scheduled-and-not-descheduled-yet_) tasks. Correspondingly, descheduling does the
//! opposite book-keeping process, regardless whether a finished task has been conflicted or not.
//!
//! For the _conflicting scheduling_ case, it remembers that each of **non-conflicting addresses**
//! like the non-conflicting case above. As for **conflicting addresses**, each task is recorded to
//! respective FIFO queues attached to the (conflicting) addresses. Importantly, the number of
//! conflicting addresses of the conflicting task is also remembered.
//!
//! The last missing piece is that the scheduler actually tries to reschedule previously blocked
//! tasks while deschduling, in addition to the above-mentioned book-keeping processing. Namely,
//! when given address is ready for new fresh locking resulted from descheduling a task (i.e. write
//! lock is released or read lock count is reached to zero), it pops out the first element of the
//! FIFO blocked-task queue of the address. Then, it immediately marks the address as relocked. It
//! also decrements the number of conflicting addresses of the popped-out task. As the final step,
//! if the number reaches to the zero, it means the task has fully finished locking all of its
//! addresses and is directly routed to be runnable.
//!
//! Put differently, this algorigthm tries to gradually lock all of addresses of tasks at different
//! timings while not deviating the execution order from the original task ingestion order. This
//! implies there's no locking retries in general, which is the primary source of non-linear perf.
//! degration.
//!
//! As a ballpark number from a synthesized micro benchmark on usual CPU for `mainnet-beta`
//! validators, it takes roughly 100ns to schedule and deschedule a transaction with 10 accounts.
//! And 1us for a transaction with 100 accounts. Note that this excludes crossbeam communication
//! overhead at all. That's said, it's not unrealistic to say the whole unified scheduler can
//! attain 100k-1m tps overall, assuming those transaction executions aren't bottlenecked.
//!
//! ### Runtime performance characteristics and data structure arrangement
//!
//! Its algorithm is very fast for high throughput, real-time for low latency. The whole
//! unified-scheduler architecture is designed from grounds up to support the fastest execution of
//! this scheduling code. For that end, unified scheduler pre-loads address-specific locking state
//! data structures (called [`Page`]) for all of transaction's accounts, in order to offload the
//! job to other threads from the scheduler thread. This preloading is done inside
//! [`create_task()`](SchedulingStateMachine::create_task). In this way, task scheduling
//! computational complexity is basically reduced to several word-sized loads and stores in the
//! schduler thread (i.e.  constant; no allocations nor syscalls), while being proportional to the
//! number of addresses in a given transaction. Note that this statement is held true, regardless
//! of conflicts. This is because the preloading also pre-allocates some scratch-pad area
//! ([`blocked_tasks`](PageInner::blocked_tasks)) to stash blocked ones. So, a conflict only incurs
//! some additional fixed number of mem stores, within error magin of the constant complexity. And
//! additional memory allocation for the scratchpad could said to be amortized, if such unsual
//! event should occur.
//!
//! [`Arc`] is used to implement this preloading mechanism, because `Page`s are shared across tasks
//! accessing the same account, and among threads due to the preloading. Also, interior mutability
//! is needed. However, `SchedulingStateMachine` doesn't use conventional locks like RwLock.
//! Leveraving the fact it's the only state-mutating exclusive thread, it instead uses
//! `UnsafeCell`, which is sugar-coated by a tailored wrapper called [`TokenCell`]. `TokenCell`
//! improses an overly restrictive aliasing rule via rust type system to maintain the memory
//! safety. By localizing any synchronization to the message passing, the scheduling code itself
//! attains maximally possible single-threaed execution without stalling cpu pipelines at all, only
//! constrained to mem access latency, while efficiently utilzing L1-L3 cpu cache with full of
//! `Page`s.
//!
//! ### Buffer bloat insignificance
//!
//! The scheduler code itself doesn't care about the buffer bloat problem, which can occur in
//! unified scheduler, where a run of heavily linearized and blocked tasks could severely hampered
//! by very large number of interleaved runnable tasks along side.  The reason is again for
//! separation of concerns. This is acceptable because the scheduling code itself isn't susceptible
//! to the buffer bloat problem by itself as explained by the description and validated by the
//! mentioned benchmark above. Thus, this should be solved elsewhere, specifically at the scheduler
//! pool.
use {
    crate::utils::{ShortCounter, Token, TokenCell},
    solana_sdk::{pubkey::Pubkey, transaction::SanitizedTransaction},
    static_assertions::const_assert_eq,
    std::{collections::VecDeque, mem, sync::Arc},
};

/// Internal utilities. Namely this contains [`ShortCounter`] and [`TokenCell`].
mod utils {
    use std::{
        any::{self, TypeId},
        cell::{RefCell, UnsafeCell},
        collections::BTreeSet,
        marker::PhantomData,
        thread,
    };

    /// A really tiny counter to hide `.checked_{add,sub}` all over the place.
    ///
    /// It's caller's reponsibility to ensure this (backed by [`u32`]) never overflow.
    #[derive(Debug, Clone, Copy)]
    pub(super) struct ShortCounter(u32);

    impl ShortCounter {
        pub(super) fn zero() -> Self {
            Self(0)
        }

        pub(super) fn one() -> Self {
            Self(1)
        }

        pub(super) fn is_one(&self) -> bool {
            self.0 == 1
        }

        pub(super) fn is_zero(&self) -> bool {
            self.0 == 0
        }

        pub(super) fn current(&self) -> u32 {
            self.0
        }

        #[must_use]
        pub(super) fn increment(self) -> Self {
            Self(self.0.checked_add(1).unwrap())
        }

        #[must_use]
        pub(super) fn decrement(self) -> Self {
            Self(self.0.checked_sub(1).unwrap())
        }

        pub(super) fn increment_self(&mut self) -> &mut Self {
            *self = self.increment();
            self
        }

        pub(super) fn decrement_self(&mut self) -> &mut Self {
            *self = self.decrement();
            self
        }

        pub(super) fn reset_to_zero(&mut self) -> &mut Self {
            self.0 = 0;
            self
        }
    }

    /// A conditionally [`Send`]-able and [`Sync`]-able cell leveraging scheduler's one-by-one data
    /// access pattern with zero runtime synchronization cost.
    ///
    /// To comply with Rust's aliasing rules, these cells require a carefully-created [`Token`] to
    /// be passed around to access the inner values. The token is a special-purpose phantom object
    /// to get rid of its inherent `unsafe`-ness in [`UnsafeCell`], which is internally used for
    /// the interior mutability.
    ///
    /// The final objective of [`Token`] is to ensure there's only one mutable reference to the
    /// [`TokenCell`] at most _at any given moment_. To that end, it's `unsafe` to create it,
    /// shifting the responsibility of binding the only singleton instance to a particular thread
    /// and not creating more than one, onto the API consumers. And its constructor is non-`const`,
    /// and the type is `!Clone` (and `!Copy` as well), `!Default`, `!Send` and `!Sync` to make it
    /// relatively hard to cross thread boundaries accidentally.
    ///
    /// In other words, the token semantically _owns_ all of its associated instances of
    /// [`TokenCell`]s. And `&mut Token` is needed to access one of them as if the one is of
    /// [`Token`]'s `*_mut()` getters. Thus, the Rust aliasing rule for `UnsafeCell` can
    /// transitively be proven to be satisfied simply based on the usual borrow checking of the
    /// `&mut` reference of [`Token`] itself via [`::borrow_mut()`](TokenCell::borrow_mut).
    ///
    /// By extension, it's allowed to create _multiple_ tokens in a _single_ process as long as no
    /// instance of [`TokenCell`] is shared by multiple instances of [`Token`].
    ///
    /// Note that this is overly restrictive in that it's forbidden, yet, technically possible
    /// to _have multiple mutable references to the inner values at the same time, if and only
    /// if the respective cells aren't aliased to each other (i.e. different instances)_. This
    /// artificial restriction is acceptable for its intended use by the unified scheduler's code
    /// because its algorithm only needs to access each instance of [`TokenCell`]-ed data once at a
    /// time. Finally, this restriction is traded off for restoration of Rust aliasing rule at zero
    /// runtime cost.  Without this token mechanism, there's no way to realize this.
    #[derive(Debug, Default)]
    pub(super) struct TokenCell<V>(UnsafeCell<V>);

    impl<V> TokenCell<V> {
        /// Creates a new `TokenCell` with the `value` typed as `V`.
        ///
        /// Note that this isn't parametric over the its accompanied `Token`'s lifetime to avoid
        /// complex handling of non-`'static` heaped data in general. Instead, it's manually
        /// required to ensure this instance is accessed only via its associated Token for the
        /// entire lifetime.
        // non-const to forbid unprotected sharing via static variables among threads.
        pub(super) fn new(value: V) -> Self {
            Self(UnsafeCell::new(value))
        }

        /// Returns a mutable reference with its lifetime bound to the mutable reference of the
        /// given token.
        ///
        /// In this way, any additional reborrow can never happen at the same time across all
        /// instances of [`TokenCell<V>`] conceptually owned by the instance of [`Token<V>`] (a
        /// particular thread), unless previous borrow is released. After the release, the used
        /// singleton token should be free to be reused for reborrows.
        pub(super) fn borrow_mut<'t>(&self, _token: &'t mut Token<V>) -> &'t mut V {
            unsafe { &mut *self.0.get() }
        }
    }

    // Safety: Access to TokenCell is assumed to be only from a single thread by proper use of
    // Token once after TokenCell is sent to the thread from other threads; So, both implementing
    // Send and Sync can be thought as safe.
    //
    // In other words, TokenCell is technicall still !Send and !Sync. But there should be no legal
    // use happening which requires !Send or !Sync to avoid undefined behavior.
    unsafe impl<V> Send for TokenCell<V> {}
    unsafe impl<V> Sync for TokenCell<V> {}

    /// A auxiliary zero-sized type to enforce aliasing rule to [`TokenCell`] via rust type system
    ///
    /// Token semantically owns a collection of `TokenCell` objects and governs the _unique_
    /// existence of mutable access over them by requiring the token itself to be mutably borrowed
    /// to get a mutable reference to the internal value of `TokenCell`.
    // *mut is used to make this type !Send and !Sync
    pub(super) struct Token<V: 'static>(PhantomData<*mut V>);

    impl<V> Token<V> {
        // Returns the token to acquire a mutable reference to the inner value of [TokenCell].
        //
        // Safety:
        // This method should be called exactly once for each thread at most.
        #[must_use]
        pub(super) unsafe fn assume_exclusive_mutating_thread() -> Self {
            thread_local! {
                static TOKENS: RefCell<BTreeSet<TypeId>> = const { RefCell::new(BTreeSet::new()) };
            }
            assert!(
                TOKENS.with_borrow_mut(|tokens| tokens.insert(TypeId::of::<Self>())),
                "{:?} is wrongly initialized twice on {:?}",
                any::type_name::<Self>(),
                thread::current()
            );

            Self(PhantomData)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::Token;

        #[test]
        #[should_panic(
            expected = "\"solana_unified_scheduler_logic::utils::Token<usize>\" is wrongly \
                        initialized twice on Thread"
        )]
        fn test_second_creation_of_tokens_in_a_thread() {
            unsafe {
                let _ = Token::<usize>::assume_exclusive_mutating_thread();
                let _ = Token::<usize>::assume_exclusive_mutating_thread();
            }
        }
    }
}

/// [`Result`] for locking a [page](Page) with particular [usage](RequestedUsage).
type LockResult = Result<PageUsage, ()>;
const_assert_eq!(mem::size_of::<LockResult>(), 8);

/// Something to be scheduled; usually a wrapper of [`SanitizedTransaction`].
pub type Task = Arc<TaskInner>;
const_assert_eq!(mem::size_of::<Task>(), 8);

/// [`Token`] for [`Page`].
type PageToken = Token<PageInner>;
const_assert_eq!(mem::size_of::<PageToken>(), 0);

/// [`Token`] for [task](Task)'s [internal mutable data](`TaskInner::blocked_page_count`).
type BlockedPageCountToken = Token<ShortCounter>;
const_assert_eq!(mem::size_of::<BlockedPageCountToken>(), 0);

/// Internal scheduling data about a particular task.
#[derive(Debug)]
pub struct TaskInner {
    transaction: SanitizedTransaction,
    index: usize,
    lock_attempts: Vec<LockAttempt>,
    blocked_page_count: TokenCell<ShortCounter>,
}

impl TaskInner {
    pub fn task_index(&self) -> usize {
        self.index
    }

    pub fn transaction(&self) -> &SanitizedTransaction {
        &self.transaction
    }

    fn lock_attempts(&self) -> &Vec<LockAttempt> {
        &self.lock_attempts
    }

    fn blocked_page_count_mut<'t>(
        &self,
        token: &'t mut BlockedPageCountToken,
    ) -> &'t mut ShortCounter {
        self.blocked_page_count.borrow_mut(token)
    }

    fn set_blocked_page_count(&self, token: &mut BlockedPageCountToken, count: ShortCounter) {
        *self.blocked_page_count_mut(token) = count;
    }

    #[must_use]
    fn try_unblock(self: &Task, token: &mut BlockedPageCountToken) -> Option<Task> {
        self.blocked_page_count_mut(token)
            .decrement_self()
            .is_zero()
            .then(|| self.clone())
    }
}

/// [`Task`]'s per-address attempt to use a [page](Page) with [certain kind of
/// request](RequestedUsage).
#[derive(Debug)]
struct LockAttempt {
    page: Page,
    requested_usage: RequestedUsage,
}
const_assert_eq!(mem::size_of::<LockAttempt>(), 16);

impl LockAttempt {
    fn new(page: Page, requested_usage: RequestedUsage) -> Self {
        Self {
            page,
            requested_usage,
        }
    }

    fn page_mut<'t>(&self, page_token: &'t mut PageToken) -> &'t mut PageInner {
        self.page.0.borrow_mut(page_token)
    }
}

/// Status about how the [`Page`] is used currently. Unlike [`RequestedUsage`], it has additional
/// variant of [`Unused`](`PageUsage::Unused`).
#[derive(Copy, Clone, Debug, Default)]
enum PageUsage {
    #[default]
    Unused,
    Readonly(ShortCounter),
    Writable,
}
const_assert_eq!(mem::size_of::<PageUsage>(), 8);

impl PageUsage {
    fn from_requested_usage(requested_usage: RequestedUsage) -> Self {
        match requested_usage {
            RequestedUsage::Readonly => PageUsage::Readonly(ShortCounter::one()),
            RequestedUsage::Writable => PageUsage::Writable,
        }
    }
}

/// Status about how a task is requesting to use a particular [`Page`]. Unlike [`PageUsage`],
/// it has only two unit variants.
#[derive(Clone, Copy, Debug)]
enum RequestedUsage {
    Readonly,
    Writable,
}

/// Internal scheduling data about a particular address.
///
/// Specifially, it holds the current [`PageUsage`] (or no usage with [`PageUsage::Unused`]) and
/// which [`Task`]s are blocked to be executed after the current task is notified to be finished
/// via [`::deschedule_task`](`SchedulingStateMachine::deschedule_task`)
#[derive(Debug)]
struct PageInner {
    usage: PageUsage,
    blocked_tasks: VecDeque<(Task, RequestedUsage)>,
}

impl Default for PageInner {
    fn default() -> Self {
        Self {
            usage: PageUsage::default(),
            blocked_tasks: VecDeque::with_capacity(1024),
        }
    }
}

impl PageInner {
    fn push_blocked_task(&mut self, task: Task, requested_usage: RequestedUsage) {
        self.blocked_tasks.push_back((task, requested_usage));
    }

    fn has_no_blocked_task(&self) -> bool {
        self.blocked_tasks.is_empty()
    }

    #[must_use]
    fn pop_unblocked_next_task(&mut self) -> Option<(Task, RequestedUsage)> {
        self.blocked_tasks.pop_front()
    }

    #[must_use]
    fn blocked_next_task(&self) -> Option<(&Task, RequestedUsage)> {
        self.blocked_tasks
            .front()
            .map(|(task, requested_usage)| (task, *requested_usage))
    }

    #[must_use]
    fn pop_blocked_next_readonly_task(&mut self) -> Option<(Task, RequestedUsage)> {
        if matches!(
            self.blocked_next_task(),
            Some((_, RequestedUsage::Readonly))
        ) {
            self.pop_unblocked_next_task()
        } else {
            None
        }
    }
}

const_assert_eq!(mem::size_of::<TokenCell<PageInner>>(), 40);

/// Scheduler's internal data for each address ([`Pubkey`](`solana_sdk::pubkey::Pubkey`)). Very
/// opaque wrapper type; no methods just with [`::clone()`](Clone::clone) and
/// [`::default()`](Default::default).
#[derive(Debug, Clone, Default)]
pub struct Page(Arc<TokenCell<PageInner>>);
const_assert_eq!(mem::size_of::<Page>(), 8);

/// A high-level `struct`, managing the overall scheduling of [tasks](Task), to be used by
/// `solana-unified-scheduler-pool`.
pub struct SchedulingStateMachine {
    last_task_index: Option<usize>,
    unblocked_task_queue: VecDeque<Task>,
    active_task_count: ShortCounter,
    handled_task_count: ShortCounter,
    unblocked_task_count: ShortCounter,
    total_task_count: ShortCounter,
    count_token: BlockedPageCountToken,
    page_token: PageToken,
}
const_assert_eq!(mem::size_of::<SchedulingStateMachine>(), 64);

impl SchedulingStateMachine {
    pub fn has_no_active_task(&self) -> bool {
        self.active_task_count.is_zero()
    }

    pub fn unblocked_task_queue_count(&self) -> usize {
        self.unblocked_task_queue.len()
    }

    pub fn active_task_count(&self) -> u32 {
        self.active_task_count.current()
    }

    pub fn handled_task_count(&self) -> u32 {
        self.handled_task_count.current()
    }

    pub fn unblocked_task_count(&self) -> u32 {
        self.unblocked_task_count.current()
    }

    pub fn total_task_count(&self) -> u32 {
        self.total_task_count.current()
    }

    #[must_use]
    pub fn schedule_task(&mut self, task: Task) -> Option<Task> {
        let new_task_index = task.task_index();
        if let Some(old_task_index) = self.last_task_index.replace(new_task_index) {
            assert!(
                new_task_index > old_task_index,
                "bad new task index: {new_task_index} > {old_task_index}"
            );
        }
        self.total_task_count.increment_self();
        self.active_task_count.increment_self();
        self.attempt_lock_for_task(task)
    }

    pub fn has_unblocked_task(&self) -> bool {
        !self.unblocked_task_queue.is_empty()
    }

    #[must_use]
    pub fn schedule_unblocked_task(&mut self) -> Option<Task> {
        self.unblocked_task_queue.pop_front().map(|task| {
            self.unblocked_task_count.increment_self();
            task
        })
    }

    pub fn deschedule_task(&mut self, task: &Task) {
        let descheduled_task_index = task.task_index();
        let largest_task_index = self
            .last_task_index
            .expect("task should have been scheduled");
        assert!(
            descheduled_task_index <= largest_task_index,
            "bad descheduled task index: {descheduled_task_index} <= {largest_task_index}"
        );
        self.active_task_count.decrement_self();
        self.handled_task_count.increment_self();
        self.unlock_for_task(task);
    }

    #[must_use]
    fn attempt_lock_pages(&mut self, task: &Task) -> ShortCounter {
        let mut blocked_page_count = ShortCounter::zero();

        for attempt in task.lock_attempts() {
            let page = attempt.page_mut(&mut self.page_token);
            let lock_status = if page.has_no_blocked_task() {
                Self::attempt_lock_page(page, attempt.requested_usage)
            } else {
                LockResult::Err(())
            };
            match lock_status {
                LockResult::Ok(PageUsage::Unused) => unreachable!(),
                LockResult::Ok(new_usage) => {
                    page.usage = new_usage;
                }
                LockResult::Err(()) => {
                    blocked_page_count.increment_self();
                    page.push_blocked_task(task.clone(), attempt.requested_usage);
                }
            }
        }

        blocked_page_count
    }

    fn attempt_lock_page(page: &PageInner, requested_usage: RequestedUsage) -> LockResult {
        match page.usage {
            PageUsage::Unused => LockResult::Ok(PageUsage::from_requested_usage(requested_usage)),
            PageUsage::Readonly(count) => match requested_usage {
                RequestedUsage::Readonly => LockResult::Ok(PageUsage::Readonly(count.increment())),
                RequestedUsage::Writable => LockResult::Err(()),
            },
            PageUsage::Writable => LockResult::Err(()),
        }
    }

    #[must_use]
    fn unlock_page(page: &mut PageInner, attempt: &LockAttempt) -> Option<(Task, RequestedUsage)> {
        let mut is_unused_now = false;
        match &mut page.usage {
            PageUsage::Readonly(ref mut count) => match attempt.requested_usage {
                RequestedUsage::Readonly => {
                    if count.is_one() {
                        is_unused_now = true;
                    } else {
                        count.decrement_self();
                    }
                }
                RequestedUsage::Writable => unreachable!(),
            },
            PageUsage::Writable => match attempt.requested_usage {
                RequestedUsage::Writable => {
                    is_unused_now = true;
                }
                RequestedUsage::Readonly => unreachable!(),
            },
            PageUsage::Unused => unreachable!(),
        }

        if is_unused_now {
            page.usage = PageUsage::Unused;
            page.pop_unblocked_next_task()
        } else {
            None
        }
    }

    #[must_use]
    fn attempt_lock_for_task(&mut self, task: Task) -> Option<Task> {
        let blocked_page_count = self.attempt_lock_pages(&task);

        if blocked_page_count.is_zero() {
            // succeeded
            Some(task)
        } else {
            // failed
            task.set_blocked_page_count(&mut self.count_token, blocked_page_count);
            None
        }
    }

    fn unlock_for_task(&mut self, task: &Task) {
        for unlock_attempt in task.lock_attempts() {
            let page = unlock_attempt.page_mut(&mut self.page_token);
            let mut unblocked_task_from_page = Self::unlock_page(page, unlock_attempt);

            while let Some((task_with_unblocked_page, requested_usage)) = unblocked_task_from_page {
                if let Some(task) = task_with_unblocked_page.try_unblock(&mut self.count_token) {
                    self.unblocked_task_queue.push_back(task);
                }

                match Self::attempt_lock_page(page, requested_usage) {
                    LockResult::Ok(PageUsage::Unused) => unreachable!(),
                    LockResult::Ok(new_usage) => {
                        page.usage = new_usage;
                        // Try to further schedule blocked task for parallelism in the case of
                        // readonly usages
                        unblocked_task_from_page = if matches!(new_usage, PageUsage::Readonly(_)) {
                            page.pop_blocked_next_readonly_task()
                        } else {
                            None
                        };
                    }
                    LockResult::Err(_) => panic!("should never fail in this context"),
                }
            }
        }
    }

    /// Creates a new task with [`SanitizedTransaction`] with all of its corresponding [`Page`]s
    /// preloaded.
    ///
    /// Closure (`page_loader`) is used to delegate the (possibly multi-threaded)
    /// implementation of [`Page`] look-up by [`pubkey`](Pubkey) to callers. It's the caller's
    /// responsibility to ensure the same instance is returned from the closure, given a particular
    /// pubkey.
    pub fn create_task(
        transaction: SanitizedTransaction,
        index: usize,
        page_loader: &mut impl FnMut(Pubkey) -> Page,
    ) -> Task {
        // this is safe bla bla
        let locks = transaction.get_account_locks_unchecked();

        let writable_locks = locks
            .writable
            .iter()
            .map(|address| (address, RequestedUsage::Writable));
        let readonly_locks = locks
            .readonly
            .iter()
            .map(|address| (address, RequestedUsage::Readonly));

        let lock_attempts = writable_locks
            .chain(readonly_locks)
            .map(|(address, requested_usage)| {
                LockAttempt::new(page_loader(**address), requested_usage)
            })
            .collect();

        Task::new(TaskInner {
            transaction,
            index,
            lock_attempts,
            blocked_page_count: TokenCell::new(ShortCounter::zero()),
        })
    }

    /// Rewind the inactive state machine to be initialized
    ///
    /// This isn't called _reset_ to indicate this isn't safe to call this at any given moment.
    /// This panics if the state machine hasn't properly been finished (i.e.  there should be no
    /// active task) to uphold invariants of [`Page`]s.
    ///
    /// This method is intended to reuse SchedulingStateMachine instance (to avoid its `unsafe`
    /// [constructor](SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling)
    /// as much as possible) and its (possbily cached) associated [`Page`]s for processing other
    /// slots.
    pub fn reinitialize(&mut self) {
        assert!(self.has_no_active_task());
        assert_eq!(self.unblocked_task_queue.len(), 0);
        self.last_task_index = None;
        self.active_task_count.reset_to_zero();
        self.handled_task_count.reset_to_zero();
        self.unblocked_task_count.reset_to_zero();
        self.total_task_count.reset_to_zero();
    }

    /// Creates a new instance of [`SchedulingStateMachine`] with its `unsafe` fields created as
    /// well, thus carrying over `unsafe`.
    ///
    /// # Safety
    /// Call this exactly once for each thread. See [`TokenCell`] for details.
    #[must_use]
    pub unsafe fn exclusively_initialize_current_thread_for_scheduling() -> Self {
        Self {
            last_task_index: None,
            unblocked_task_queue: VecDeque::with_capacity(1024),
            active_task_count: ShortCounter::zero(),
            handled_task_count: ShortCounter::zero(),
            unblocked_task_count: ShortCounter::zero(),
            total_task_count: ShortCounter::zero(),
            count_token: unsafe { BlockedPageCountToken::assume_exclusive_mutating_thread() },
            page_token: unsafe { PageToken::assume_exclusive_mutating_thread() },
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        assert_matches::assert_matches,
        solana_sdk::{
            instruction::{AccountMeta, Instruction},
            message::Message,
            pubkey::Pubkey,
            signature::Signer,
            signer::keypair::Keypair,
            transaction::{SanitizedTransaction, Transaction},
        },
        std::{cell::RefCell, collections::HashMap, rc::Rc},
    };

    fn simplest_transaction() -> SanitizedTransaction {
        let payer = Keypair::new();
        let message = Message::new(&[], Some(&payer.pubkey()));
        let unsigned = Transaction::new_unsigned(message);
        SanitizedTransaction::from_transaction_for_tests(unsigned)
    }

    fn transaction_with_readonly_address(address: Pubkey) -> SanitizedTransaction {
        let instruction = Instruction {
            program_id: Pubkey::default(),
            accounts: vec![AccountMeta::new_readonly(address, false)],
            data: vec![],
        };
        let message = Message::new(&[instruction], Some(&Pubkey::new_unique()));
        let unsigned = Transaction::new_unsigned(message);
        SanitizedTransaction::from_transaction_for_tests(unsigned)
    }

    fn transaction_with_writable_address(address: Pubkey) -> SanitizedTransaction {
        let instruction = Instruction {
            program_id: Pubkey::default(),
            accounts: vec![AccountMeta::new(address, false)],
            data: vec![],
        };
        let message = Message::new(&[instruction], Some(&Pubkey::new_unique()));
        let unsigned = Transaction::new_unsigned(message);
        SanitizedTransaction::from_transaction_for_tests(unsigned)
    }

    fn create_address_loader(
        pages: Option<Rc<RefCell<HashMap<Pubkey, Page>>>>,
    ) -> impl FnMut(Pubkey) -> Page {
        let pages = pages.unwrap_or_default();
        move |address| pages.borrow_mut().entry(address).or_default().clone()
    }

    #[test]
    fn test_debug() {
        // these are almost meaningless just to see eye-pleasing coverage report....
        assert_eq!(
            format!(
                "{:?}",
                LockResult::Ok(PageUsage::Readonly(ShortCounter::one()))
            ),
            "Ok(Readonly(ShortCounter(1)))"
        );
        let sanitized = simplest_transaction();
        let task = SchedulingStateMachine::create_task(sanitized, 0, &mut |_| Page::default());
        assert!(format!("{:?}", task).contains("TaskInner"));

        assert_eq!(
            format!("{:?}", PageInner::default()),
            "PageInner { usage: Unused, blocked_tasks: [] }"
        )
    }

    #[test]
    fn test_scheduling_state_machine_creation() {
        let state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_eq!(state_machine.active_task_count(), 0);
        assert_eq!(state_machine.total_task_count(), 0);
        assert!(state_machine.has_no_active_task());
    }

    #[test]
    fn test_scheduling_state_machine_reinitialization() {
        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        state_machine.total_task_count.increment_self();
        assert_eq!(state_machine.total_task_count(), 1);
        state_machine.last_task_index = Some(1);
        state_machine.reinitialize();
        assert_eq!(state_machine.total_task_count(), 0);
        assert_eq!(state_machine.last_task_index, None);
    }

    #[test]
    fn test_create_task() {
        let sanitized = simplest_transaction();
        let task =
            SchedulingStateMachine::create_task(sanitized.clone(), 3, &mut |_| Page::default());
        assert_eq!(task.task_index(), 3);
        assert_eq!(task.transaction(), &sanitized);
    }

    #[test]
    fn test_non_conflicting_task_related_counts() {
        let sanitized = simplest_transaction();
        let address_loader = &mut create_address_loader(None);
        let task = SchedulingStateMachine::create_task(sanitized.clone(), 3, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        let task = state_machine.schedule_task(task).unwrap();
        assert_eq!(state_machine.active_task_count(), 1);
        assert_eq!(state_machine.total_task_count(), 1);
        state_machine.deschedule_task(&task);
        assert_eq!(state_machine.active_task_count(), 0);
        assert_eq!(state_machine.total_task_count(), 1);
        assert!(state_machine.has_no_active_task());
    }

    #[test]
    fn test_conflicting_task_related_counts() {
        let sanitized = simplest_transaction();
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized.clone(), 101, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized.clone(), 102, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized.clone(), 103, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(
            state_machine
                .schedule_task(task1.clone())
                .map(|t| t.task_index()),
            Some(101)
        );
        assert_matches!(state_machine.schedule_task(task2.clone()), None);

        state_machine.deschedule_task(&task1);
        assert!(state_machine.has_unblocked_task());
        assert_eq!(state_machine.unblocked_task_queue_count(), 1);
        assert_eq!(
            state_machine
                .schedule_unblocked_task()
                .unwrap()
                .task_index(),
            task2.task_index()
        );
        assert!(!state_machine.has_unblocked_task());
        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task2);

        assert_matches!(
            state_machine
                .schedule_task(task3.clone())
                .map(|task| task.task_index()),
            Some(103)
        );
        state_machine.deschedule_task(&task3);
        assert!(state_machine.has_no_active_task());
    }

    #[test]
    fn test_unblocked_task_related_counts() {
        let sanitized = simplest_transaction();
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized.clone(), 101, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized.clone(), 102, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(
            state_machine
                .schedule_task(task1.clone())
                .map(|t| t.task_index()),
            Some(101)
        );
        assert_matches!(state_machine.schedule_task(task2.clone()), None);

        state_machine.deschedule_task(&task1);

        assert_eq!(state_machine.unblocked_task_count(), 0);
        assert_matches!(
            state_machine
                .schedule_unblocked_task()
                .map(|t| t.task_index()),
            Some(102)
        );
        assert_eq!(state_machine.unblocked_task_count(), 1);
        // there's no blocked task anymore; calling schedule_unblocked_task should be noop and
        // shouldn't increment the unblocked_task_count().
        assert_matches!(state_machine.schedule_unblocked_task(), None);
        assert_eq!(state_machine.unblocked_task_count(), 1);

        state_machine.deschedule_task(&task2);
        assert!(state_machine.has_no_active_task());
    }

    #[test]
    fn test_existing_blocking_task_then_newly_scheduled_task() {
        let sanitized = simplest_transaction();
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized.clone(), 101, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized.clone(), 102, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized.clone(), 103, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(
            state_machine
                .schedule_task(task1.clone())
                .map(|t| t.task_index()),
            Some(101)
        );
        assert_matches!(state_machine.schedule_task(task2.clone()), None);

        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task1);
        assert_eq!(state_machine.unblocked_task_queue_count(), 1);

        // new task is arriving after task1 is already descheduled and task2 got unblocked
        assert_matches!(state_machine.schedule_task(task3.clone()), None);

        assert_eq!(state_machine.unblocked_task_count(), 0);
        assert_matches!(
            state_machine
                .schedule_unblocked_task()
                .map(|t| t.task_index()),
            Some(102)
        );
        assert_eq!(state_machine.unblocked_task_count(), 1);

        state_machine.deschedule_task(&task2);

        assert_matches!(
            state_machine
                .schedule_unblocked_task()
                .map(|t| t.task_index()),
            Some(103)
        );
        assert_eq!(state_machine.unblocked_task_count(), 2);

        state_machine.deschedule_task(&task3);
        assert!(state_machine.has_no_active_task());
    }

    #[test]
    fn test_multiple_readonly_task_and_counts() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = transaction_with_readonly_address(conflicting_address);
        let sanitized2 = transaction_with_readonly_address(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized1, 101, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 102, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        // both of read-only tasks should be immediately runnable
        assert_matches!(
            state_machine
                .schedule_task(task1.clone())
                .map(|t| t.task_index()),
            Some(101)
        );
        assert_matches!(
            state_machine
                .schedule_task(task2.clone())
                .map(|t| t.task_index()),
            Some(102)
        );

        assert_eq!(state_machine.active_task_count(), 2);
        assert_eq!(state_machine.handled_task_count(), 0);
        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task1);
        assert_eq!(state_machine.active_task_count(), 1);
        assert_eq!(state_machine.handled_task_count(), 1);
        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task2);
        assert_eq!(state_machine.active_task_count(), 0);
        assert_eq!(state_machine.handled_task_count(), 2);
        assert!(state_machine.has_no_active_task());
    }

    #[test]
    fn test_all_blocking_redable_tasks_block_writable_task() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = transaction_with_readonly_address(conflicting_address);
        let sanitized2 = transaction_with_readonly_address(conflicting_address);
        let sanitized3 = transaction_with_writable_address(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized1, 101, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 102, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized3, 103, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(
            state_machine
                .schedule_task(task1.clone())
                .map(|t| t.task_index()),
            Some(101)
        );
        assert_matches!(
            state_machine
                .schedule_task(task2.clone())
                .map(|t| t.task_index()),
            Some(102)
        );
        assert_matches!(state_machine.schedule_task(task3.clone()), None);

        assert_eq!(state_machine.active_task_count(), 3);
        assert_eq!(state_machine.handled_task_count(), 0);
        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task1);
        assert_eq!(state_machine.active_task_count(), 2);
        assert_eq!(state_machine.handled_task_count(), 1);
        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        assert_matches!(state_machine.schedule_unblocked_task(), None);
        state_machine.deschedule_task(&task2);
        assert_eq!(state_machine.active_task_count(), 1);
        assert_eq!(state_machine.handled_task_count(), 2);
        assert_eq!(state_machine.unblocked_task_queue_count(), 1);
        // task3 is finally unblocked after all of readble tasks (task1 and task2) is finished.
        assert_matches!(
            state_machine
                .schedule_unblocked_task()
                .map(|t| t.task_index()),
            Some(103)
        );
        state_machine.deschedule_task(&task3);
        assert!(state_machine.has_no_active_task());
    }

    #[test]
    fn test_readonly_then_writable_then_readonly_linearized() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = transaction_with_readonly_address(conflicting_address);
        let sanitized2 = transaction_with_writable_address(conflicting_address);
        let sanitized3 = transaction_with_readonly_address(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized1, 101, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 102, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized3, 103, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(
            state_machine
                .schedule_task(task1.clone())
                .map(|t| t.task_index()),
            Some(101)
        );
        assert_matches!(state_machine.schedule_task(task2.clone()), None);
        assert_matches!(state_machine.schedule_task(task3.clone()), None);

        assert_matches!(state_machine.schedule_unblocked_task(), None);
        state_machine.deschedule_task(&task1);
        assert_matches!(
            state_machine
                .schedule_unblocked_task()
                .map(|t| t.task_index()),
            Some(102)
        );
        assert_matches!(state_machine.schedule_unblocked_task(), None);
        state_machine.deschedule_task(&task2);
        assert_matches!(
            state_machine
                .schedule_unblocked_task()
                .map(|t| t.task_index()),
            Some(103)
        );
        assert_matches!(state_machine.schedule_unblocked_task(), None);
        state_machine.deschedule_task(&task3);
        assert!(state_machine.has_no_active_task());
    }

    #[test]
    fn test_readonly_then_writable() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = transaction_with_readonly_address(conflicting_address);
        let sanitized2 = transaction_with_writable_address(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized1, 101, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 102, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(
            state_machine
                .schedule_task(task1.clone())
                .map(|t| t.task_index()),
            Some(101)
        );
        assert_matches!(state_machine.schedule_task(task2.clone()), None);

        // descheduling read-locking task1 should equate to unblocking write-locking task2
        state_machine.deschedule_task(&task1);
        assert_matches!(
            state_machine
                .schedule_unblocked_task()
                .map(|t| t.task_index()),
            Some(102)
        );
        state_machine.deschedule_task(&task2);
        assert!(state_machine.has_no_active_task());
    }

    #[test]
    fn test_blocked_tasks_writable_2_readonly_then_writable() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = transaction_with_writable_address(conflicting_address);
        let sanitized2 = transaction_with_readonly_address(conflicting_address);
        let sanitized3 = transaction_with_readonly_address(conflicting_address);
        let sanitized4 = transaction_with_writable_address(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized1, 101, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 102, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized3, 103, address_loader);
        let task4 = SchedulingStateMachine::create_task(sanitized4, 104, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(
            state_machine
                .schedule_task(task1.clone())
                .map(|t| t.task_index()),
            Some(101)
        );
        assert_matches!(state_machine.schedule_task(task2.clone()), None);
        assert_matches!(state_machine.schedule_task(task3.clone()), None);
        assert_matches!(state_machine.schedule_task(task4.clone()), None);

        state_machine.deschedule_task(&task1);
        assert_matches!(
            state_machine
                .schedule_unblocked_task()
                .map(|t| t.task_index()),
            Some(102)
        );
        assert_matches!(
            state_machine
                .schedule_unblocked_task()
                .map(|t| t.task_index()),
            Some(103)
        );
        // the above deschedule_task(task1) call should only unblock task2 and task3 because these
        // are read-locking. And shouldn't unblock task4 because it's write-locking
        assert_matches!(state_machine.schedule_unblocked_task(), None);

        state_machine.deschedule_task(&task2);
        // still task4 is blocked...
        assert_matches!(state_machine.schedule_unblocked_task(), None);

        state_machine.deschedule_task(&task3);
        // finally task4 should be unblocked
        assert_matches!(
            state_machine
                .schedule_unblocked_task()
                .map(|t| t.task_index()),
            Some(104)
        );
        state_machine.deschedule_task(&task4);
        assert!(state_machine.has_no_active_task());
    }

    #[test]
    fn test_gradual_locking() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = transaction_with_writable_address(conflicting_address);
        let sanitized2 = transaction_with_writable_address(conflicting_address);
        let pages = Rc::new(RefCell::new(HashMap::new()));
        let address_loader = &mut create_address_loader(Some(pages.clone()));
        let task1 = SchedulingStateMachine::create_task(sanitized1, 101, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 102, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(
            state_machine
                .schedule_task(task1.clone())
                .map(|t| t.task_index()),
            Some(101)
        );
        assert_matches!(state_machine.schedule_task(task2.clone()), None);
        let pages = pages.borrow_mut();
        let page = pages.get(&conflicting_address).unwrap();
        assert_matches!(
            page.0.borrow_mut(&mut state_machine.page_token).usage,
            PageUsage::Writable
        );
        // task2's fee payer should have been locked already even if task2 is blocked still via the
        // above the schedule_task(task2) call
        let fee_payer = task2.transaction().message().fee_payer();
        let page = pages.get(fee_payer).unwrap();
        assert_matches!(
            page.0.borrow_mut(&mut state_machine.page_token).usage,
            PageUsage::Writable
        );
    }

    #[test]
    #[should_panic(expected = "internal error: entered unreachable code")]
    fn test_unreachable_unlock_conditions1() {
        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        let page = Page::default();
        let _ = SchedulingStateMachine::unlock_page(
            page.0.borrow_mut(&mut state_machine.page_token),
            &LockAttempt::new(page, RequestedUsage::Writable),
        );
    }

    #[test]
    #[should_panic(expected = "internal error: entered unreachable code")]
    fn test_unreachable_unlock_conditions2() {
        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        let page = Page::default();
        page.0.borrow_mut(&mut state_machine.page_token).usage = PageUsage::Writable;
        let _ = SchedulingStateMachine::unlock_page(
            page.0.borrow_mut(&mut state_machine.page_token),
            &LockAttempt::new(page, RequestedUsage::Readonly),
        );
    }

    #[test]
    #[should_panic(expected = "internal error: entered unreachable code")]
    fn test_unreachable_unlock_conditions3() {
        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        let page = Page::default();
        page.0.borrow_mut(&mut state_machine.page_token).usage =
            PageUsage::Readonly(ShortCounter::one());
        let _ = SchedulingStateMachine::unlock_page(
            page.0.borrow_mut(&mut state_machine.page_token),
            &LockAttempt::new(page, RequestedUsage::Writable),
        );
    }

    #[test]
    #[should_panic(expected = "bad new task index: 101 > 101")]
    fn test_schedule_same_task() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized = transaction_with_writable_address(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task = SchedulingStateMachine::create_task(sanitized, 101, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        let _ = state_machine.schedule_task(task.clone());
        let _ = state_machine.schedule_task(task.clone());
    }

    #[test]
    #[should_panic(expected = "bad new task index: 101 > 102")]
    fn test_schedule_task_out_of_order() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized = transaction_with_writable_address(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized.clone(), 101, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized.clone(), 102, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        let _ = state_machine.schedule_task(task2.clone());
        let _ = state_machine.schedule_task(task1.clone());
    }

    #[test]
    #[should_panic(expected = "task should have been scheduled")]
    fn test_deschedule_new_task_wihout_scheduling() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized = transaction_with_writable_address(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task = SchedulingStateMachine::create_task(sanitized.clone(), 101, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        state_machine.deschedule_task(&task);
    }

    #[test]
    #[should_panic(expected = "bad descheduled task index: 102 <= 101")]
    fn test_deschedule_new_task_out_of_order() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized = transaction_with_writable_address(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized.clone(), 101, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized.clone(), 102, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        let _ = state_machine.schedule_task(task1.clone());
        state_machine.deschedule_task(&task2);
    }
}
