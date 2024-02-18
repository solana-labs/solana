#![allow(rustdoc::private_intra_doc_links)]
//! The task (transaction) scheduling code for the unified scheduler
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
//! Its algorithm is very fast for high throughput, real-time for low latency. The whole
//! unified-scheduler architecture is designed from grounds up to support the fastest execution of
//! this scheduling code. For that end, unified scheduler pre-loads address-specific locking state
//! data structures (called [`Page`]) for all of transaction's accounts, in order to offload the
//! job to other threads from the scheduler thread. This preloading is done inside
//! [`create_task()`](SchedulingStateMachine::create_task). In this way, task scheduling complexity
//! is basically reduced to several word-sized loads and stores in the schduler thread (i.e.
//! constant; no allocations nor syscalls), while being proportional to the number of addresses in
//! a given transaction. Note that this statement is held true, regardless of conflicts. This is
//! because the preloading also pre-allocates some scratch-pad area
//! ([`blocked_tasks`](PageInner::blocked_tasks)) to stash blocked ones. So, a conflict only incurs
//! some additional fixed number of mem stores, within error magin of constant complexity. And
//! additional memory allocation for the scratchpad could said to be amortized, if such unsual
//! event should occur.
//!
//! `Arc` is used to implement this preloading mechanism, because `Page`s are shared across tasks
//! accessing the same account, and among threads. Also, interior mutability is needed. However,
//! `SchedulingStateMachine` doesn't use conventional locks like RwLock. Leveraving the fact it's
//! the only state-mutating exclusive thread, it instead uses `UnsafeCell`, which is sugar-coated
//! by a tailored wrapper called [`TokenCell`]. `TokenCell` improses an overly restrictive aliasing
//! rule via rust type system to maintain the memory safety. By localizing any synchronization to
//! the message passing, the scheduling code itself attains maximally possible single-threaed
//! execution without stalling cpu pipelines at all, only constrained to mem access latency, while
//! efficiently utilzing L1-L3 cpu cache with full of `Page`s.
//!
//! generic high-level algorithm description
//! address-level , rather than transaction. fifo for each address. no retry
//!
//! X ns for the usual case.
//!
//! Ignorance of buffer bloat

#[cfg(feature = "dev-context-only-utils")]
use qualifier_attr::field_qualifiers;
use {
    crate::utils::{ShortCounter, Token, TokenCell},
    solana_sdk::{pubkey::Pubkey, transaction::SanitizedTransaction},
    static_assertions::const_assert_eq,
    std::{collections::VecDeque, mem, sync::Arc},
};

mod utils {
    #[cfg(feature = "dev-context-only-utils")]
    use qualifier_attr::qualifiers;
    use std::{
        any::{self, TypeId},
        cell::{RefCell, UnsafeCell},
        collections::BTreeSet,
        marker::PhantomData,
        thread,
    };

    /// A really tiny counter to hide `.checked_{add,sub}` all over the place.
    ///
    /// it's caller's reponsibility to ensure this (backed by u32) never overflow.
    #[cfg_attr(feature = "dev-context-only-utils", qualifiers(pub))]
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

        pub(super) fn reset_to_zero(&mut self) {
            self.0 = 0;
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
    /// [`Token`]'s fields. Thus, the Rust aliasing rule for `UnsafeCell` can deductively be proven
    /// to be satisfied simply based on the usual borrow checking of the `&mut` reference of
    /// [`Token`] itself via [`::borrow_mut()`](TokenCell::borrow_mut).
    ///
    /// By extension, it's allowed to create _multiple_ tokens in a _single_ process as long as no
    /// instance of [`TokenCell`] is shared by multiple instances of [`Token`].
    ///
    /// Note that this is overly restrictive in that it's forbidden, yet, technically possible
    /// to _have multiple mutable references to the inner value if and only if_the respective
    /// cells aren't aliased to each other (i.e. different instances)_. This artificial restriction
    /// is acceptable for its intended use by the unified scheduler's code because its algorithm
    /// only needs to access each instance of [`TokenCell`]-ed data once at a time. Finally, this
    /// restriction is traded off for restoration of Rust aliasing rule at zero runtime cost.
    /// Without this token mechanism, there's no way to realize this.
    #[derive(Debug, Default)]
    pub(super) struct TokenCell<V>(UnsafeCell<V>);

    impl<V> TokenCell<V> {
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
    /// existence of mutable access over them by requiring it to be mutably borrowed to get a
    /// mutable reference to the internal value of `TokenCell`
    #[cfg_attr(feature = "dev-context-only-utils", qualifiers(pub))]
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

#[derive(Debug)]
enum LockStatus {
    Succeded(Usage),
    Blocked,
}
const_assert_eq!(mem::size_of::<LockStatus>(), 8);

pub type Task = Arc<TaskInner>;
const_assert_eq!(mem::size_of::<Task>(), 8);

type PageToken = Token<PageInner>;
const_assert_eq!(mem::size_of::<PageToken>(), 0);

type BlockedLockCountToken = Token<ShortCounter>;
const_assert_eq!(mem::size_of::<BlockedLockCountToken>(), 0);

#[cfg_attr(feature = "dev-context-only-utils", field_qualifiers(index(pub)))]
#[derive(Debug)]
pub struct TaskInner {
    transaction: SanitizedTransaction,
    index: usize,
    lock_attempts: Vec<LockAttempt>,
    blocked_lock_count: TokenCell<ShortCounter>,
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

    fn blocked_lock_count_mut<'t>(
        &self,
        blocked_lock_count_token: &'t mut BlockedLockCountToken,
    ) -> &'t mut ShortCounter {
        self.blocked_lock_count.borrow_mut(blocked_lock_count_token)
    }
}

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

#[derive(Copy, Clone, Debug, Default)]
enum Usage {
    #[default]
    Unused,
    Readonly(ShortCounter),
    Writable,
}
const_assert_eq!(mem::size_of::<Usage>(), 8);

impl Usage {
    fn from_requested_usage(requested_usage: RequestedUsage) -> Self {
        match requested_usage {
            RequestedUsage::Readonly => Usage::Readonly(ShortCounter::one()),
            RequestedUsage::Writable => Usage::Writable,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum RequestedUsage {
    Readonly,
    Writable,
}

#[derive(Debug)]
struct PageInner {
    usage: Usage,
    blocked_tasks: VecDeque<(Task, RequestedUsage)>,
}

impl Default for PageInner {
    fn default() -> Self {
        Self {
            usage: Usage::default(),
            blocked_tasks: VecDeque::with_capacity(1024),
        }
    }
}

impl PageInner {
    fn insert_blocked_task(&mut self, task: Task, requested_usage: RequestedUsage) {
        self.blocked_tasks.push_back((task, requested_usage));
    }

    fn has_no_blocked_task(&self) -> bool {
        self.blocked_tasks.is_empty()
    }

    fn pop_blocked_task(&mut self) {
        self.blocked_tasks.pop_front().unwrap();
    }

    fn heaviest_blocked_task(&self) -> Option<(&Task, RequestedUsage)> {
        self.blocked_tasks
            .front()
            .map(|(task, requested_usage)| (task, *requested_usage))
    }
}

const_assert_eq!(mem::size_of::<TokenCell<PageInner>>(), 40);

// very opaque wrapper type; no methods just with .clone() and ::default()
#[derive(Debug, Clone, Default)]
pub struct Page(Arc<TokenCell<PageInner>>);
const_assert_eq!(mem::size_of::<Page>(), 8);

#[cfg_attr(
    feature = "dev-context-only-utils",
    field_qualifiers(blocked_lock_count_token(pub))
)]
pub struct SchedulingStateMachine {
    unblocked_task_queue: VecDeque<Task>,
    active_task_count: ShortCounter,
    handled_task_count: ShortCounter,
    unblocked_task_count: ShortCounter,
    total_task_count: ShortCounter,
    blocked_lock_count_token: BlockedLockCountToken,
    page_token: PageToken,
}
const_assert_eq!(mem::size_of::<SchedulingStateMachine>(), 48);

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
        self.total_task_count.increment_self();
        self.active_task_count.increment_self();
        self.try_lock_for_task(task)
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
        self.active_task_count.decrement_self();
        self.handled_task_count.increment_self();
        self.unlock_after_execution(task);
    }

    #[must_use]
    fn attempt_lock_for_execution(&mut self, task: &Task) -> ShortCounter {
        let mut blocked_lock_count = ShortCounter::zero();

        for attempt in task.lock_attempts() {
            let page = attempt.page_mut(&mut self.page_token);
            let lock_status = if page.has_no_blocked_task() {
                Self::attempt_lock_address(page, attempt.requested_usage)
            } else {
                LockStatus::Blocked
            };
            match lock_status {
                LockStatus::Succeded(Usage::Unused) => unreachable!(),
                LockStatus::Succeded(usage) => {
                    page.usage = usage;
                }
                LockStatus::Blocked => {
                    blocked_lock_count.increment_self();
                    page.insert_blocked_task(task.clone(), attempt.requested_usage);
                }
            }
        }

        blocked_lock_count
    }

    #[must_use]
    fn attempt_lock_address(page: &PageInner, requested_usage: RequestedUsage) -> LockStatus {
        match page.usage {
            Usage::Unused => LockStatus::Succeded(Usage::from_requested_usage(requested_usage)),
            Usage::Readonly(count) => match requested_usage {
                RequestedUsage::Readonly => {
                    LockStatus::Succeded(Usage::Readonly(count.increment()))
                }
                RequestedUsage::Writable => LockStatus::Blocked,
            },
            Usage::Writable => LockStatus::Blocked,
        }
    }

    #[must_use]
    fn unlock<'t>(
        page: &'t mut PageInner,
        attempt: &LockAttempt,
    ) -> Option<(&'t Task, RequestedUsage)> {
        let mut is_unused_now = false;

        let requested_usage = attempt.requested_usage;

        match &mut page.usage {
            Usage::Readonly(ref mut count) => match requested_usage {
                RequestedUsage::Readonly => {
                    if count.is_one() {
                        is_unused_now = true;
                    } else {
                        count.decrement_self();
                    }
                }
                RequestedUsage::Writable => unreachable!(),
            },
            Usage::Writable => match requested_usage {
                RequestedUsage::Writable => {
                    is_unused_now = true;
                }
                RequestedUsage::Readonly => unreachable!(),
            },
            Usage::Unused => unreachable!(),
        }

        if is_unused_now {
            page.usage = Usage::Unused;
            page.heaviest_blocked_task()
        } else {
            None
        }
    }

    #[must_use]
    fn try_lock_for_task(&mut self, task: Task) -> Option<Task> {
        let blocked_lock_count = self.attempt_lock_for_execution(&task);

        if blocked_lock_count.is_zero() {
            // succeeded
            Some(task)
        } else {
            *task.blocked_lock_count_mut(&mut self.blocked_lock_count_token) = blocked_lock_count;
            None
        }
    }

    fn unlock_after_execution(&mut self, task: &Task) {
        for unlock_attempt in task.lock_attempts() {
            let page = unlock_attempt.page_mut(&mut self.page_token);
            let mut heaviest_unblocked = Self::unlock(page, unlock_attempt);

            while let Some((unblocked_task, requested_usage)) = heaviest_unblocked {
                if unblocked_task
                    .blocked_lock_count_mut(&mut self.blocked_lock_count_token)
                    .decrement_self()
                    .is_zero()
                {
                    self.unblocked_task_queue.push_back(unblocked_task.clone());
                }
                page.pop_blocked_task();

                match Self::attempt_lock_address(page, requested_usage) {
                    LockStatus::Blocked | LockStatus::Succeded(Usage::Unused) => unreachable!(),
                    LockStatus::Succeded(usage) => {
                        page.usage = usage;
                        heaviest_unblocked = if matches!(usage, Usage::Readonly(_)) {
                            page.heaviest_blocked_task()
                                .filter(|t| matches!(t, (_, RequestedUsage::Readonly)))
                        } else {
                            None
                        };
                    }
                }
            }
        }
    }

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
            blocked_lock_count: TokenCell::new(ShortCounter::zero()),
        })
    }

    pub fn reinitialize(&mut self) {
        assert!(self.has_no_active_task());
        assert_eq!(self.unblocked_task_queue.len(), 0);
        self.active_task_count.reset_to_zero();
        self.handled_task_count.reset_to_zero();
        self.unblocked_task_count.reset_to_zero();
        self.total_task_count.reset_to_zero();
    }

    /// # Safety
    /// Call this exactly once for each thread.
    #[must_use]
    pub unsafe fn exclusively_initialize_current_thread_for_scheduling() -> Self {
        Self {
            unblocked_task_queue: VecDeque::with_capacity(1024),
            active_task_count: ShortCounter::zero(),
            handled_task_count: ShortCounter::zero(),
            unblocked_task_count: ShortCounter::zero(),
            total_task_count: ShortCounter::zero(),
            blocked_lock_count_token: unsafe {
                BlockedLockCountToken::assume_exclusive_mutating_thread()
            },
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
        std::{collections::HashMap, sync::Mutex},
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
        pages: Option<Arc<Mutex<HashMap<Pubkey, Page>>>>,
    ) -> impl FnMut(Pubkey) -> Page {
        let pages = pages.unwrap_or_default();
        move |address| pages.lock().unwrap().entry(address).or_default().clone()
    }

    #[test]
    fn test_debug() {
        // these are almost meaningless just to see eye-pleasing coverage report....
        assert_eq!(
            format!(
                "{:?}",
                LockStatus::Succeded(Usage::Readonly(ShortCounter::one()))
            ),
            "Succeded(Readonly(ShortCounter(1)))"
        );
        let sanitized = simplest_transaction();
        let task = SchedulingStateMachine::create_task(sanitized, 0, &mut |_| Page::default());
        assert!(format!("{:?}", task).contains("TaskInner"));
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
        state_machine.reinitialize();
        assert_eq!(state_machine.total_task_count(), 0);
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
    fn test_schedule_non_conflicting_task() {
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
        drop(task);
    }

    #[test]
    fn test_schedule_conflicting_task() {
        let sanitized = simplest_transaction();
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized.clone(), 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized.clone(), 4, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized.clone(), 5, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(state_machine.schedule_task(task1.clone()), Some(_));
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

        assert_matches!(state_machine.schedule_task(task3.clone()), Some(_));
    }

    #[test]
    fn test_schedule_unblocked_task() {
        let sanitized = simplest_transaction();
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized.clone(), 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized.clone(), 4, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(state_machine.schedule_task(task1.clone()), Some(_));
        assert_matches!(state_machine.schedule_task(task2.clone()), None);

        state_machine.deschedule_task(&task1);

        assert_eq!(state_machine.unblocked_task_count(), 0);
        assert_eq!(
            state_machine
                .schedule_unblocked_task()
                .unwrap()
                .task_index(),
            task2.task_index()
        );
        assert_eq!(state_machine.unblocked_task_count(), 1);
        assert_matches!(state_machine.schedule_unblocked_task(), None);
        assert_eq!(state_machine.unblocked_task_count(), 1);
    }

    #[test]
    fn test_schedule_unblocked_task2() {
        let sanitized = simplest_transaction();
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized.clone(), 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized.clone(), 4, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized.clone(), 5, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(state_machine.schedule_task(task1.clone()), Some(_));
        assert_matches!(state_machine.schedule_task(task2.clone()), None);

        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task1);
        assert_eq!(state_machine.unblocked_task_queue_count(), 1);

        assert_matches!(state_machine.schedule_task(task3.clone()), None);

        assert_eq!(state_machine.unblocked_task_count(), 0);
        assert_matches!(state_machine.schedule_unblocked_task(), Some(_));
        assert_eq!(state_machine.unblocked_task_count(), 1);
        assert_matches!(state_machine.schedule_unblocked_task(), None);
        assert_eq!(state_machine.unblocked_task_count(), 1);

        state_machine.deschedule_task(&task2);

        assert_matches!(state_machine.schedule_unblocked_task(), Some(_));
        assert_eq!(state_machine.unblocked_task_count(), 2);

        state_machine.deschedule_task(&task3);
        assert!(state_machine.has_no_active_task());
    }

    #[test]
    fn test_schedule_unblocked_task3() {
        let sanitized = simplest_transaction();
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized.clone(), 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized.clone(), 4, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized.clone(), 5, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(state_machine.schedule_task(task1.clone()), Some(_));
        assert_matches!(state_machine.schedule_task(task2.clone()), None);

        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task1);
        assert_eq!(state_machine.unblocked_task_queue_count(), 1);

        assert_matches!(state_machine.schedule_task(task3.clone()), None);
    }

    #[test]
    fn test_schedule_multiple_readonly_task() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = transaction_with_readonly_address(conflicting_address);
        let sanitized2 = transaction_with_readonly_address(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized1, 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 4, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(state_machine.schedule_task(task1.clone()), Some(_));
        assert_matches!(state_machine.schedule_task(task2.clone()), Some(_));

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
    }

    #[test]
    fn test_schedule_multiple_writable_tasks() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = transaction_with_readonly_address(conflicting_address);
        let sanitized2 = transaction_with_readonly_address(conflicting_address);
        let sanitized3 = transaction_with_writable_address(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized1, 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 4, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized3, 5, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(
            state_machine
                .schedule_task(task1.clone())
                .map(|t| t.task_index()),
            Some(3)
        );
        assert_matches!(
            state_machine
                .schedule_task(task2.clone())
                .map(|t| t.task_index()),
            Some(4)
        );
        assert_matches!(state_machine.schedule_task(task3.clone()), None);

        assert_eq!(state_machine.active_task_count(), 3);
        assert_eq!(state_machine.handled_task_count(), 0);
        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task1);
        assert_eq!(state_machine.active_task_count(), 2);
        assert_eq!(state_machine.handled_task_count(), 1);
        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task2);
        assert_eq!(state_machine.active_task_count(), 1);
        assert_eq!(state_machine.handled_task_count(), 2);
        assert_eq!(state_machine.unblocked_task_queue_count(), 1);
    }

    #[test]
    fn test_schedule_rw_mixed() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = transaction_with_readonly_address(conflicting_address);
        let sanitized2 = transaction_with_writable_address(conflicting_address);
        let sanitized3 = transaction_with_readonly_address(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized1, 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 4, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized3, 5, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(
            state_machine
                .schedule_task(task1.clone())
                .map(|t| t.task_index()),
            Some(3)
        );
        assert_matches!(state_machine.schedule_task(task2.clone()), None);
        assert_matches!(state_machine.schedule_task(task3.clone()), None);

        assert_eq!(state_machine.active_task_count(), 3);
        assert_eq!(state_machine.handled_task_count(), 0);
        assert_eq!(state_machine.unblocked_task_queue_count(), 0);
        state_machine.deschedule_task(&task1);
        assert_eq!(state_machine.active_task_count(), 2);
        assert_eq!(state_machine.handled_task_count(), 1);
        assert_eq!(state_machine.unblocked_task_queue_count(), 1);
        assert_matches!(
            state_machine
                .schedule_unblocked_task()
                .map(|t| t.task_index()),
            Some(4)
        );
    }

    #[test]
    fn test_schedule_writable_after_readonly() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = transaction_with_readonly_address(conflicting_address);
        let sanitized2 = transaction_with_writable_address(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized1, 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 4, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(
            state_machine
                .schedule_task(task1.clone())
                .map(|t| t.task_index()),
            Some(3)
        );
        assert_matches!(state_machine.schedule_task(task2.clone()), None);

        state_machine.deschedule_task(&task1);
        assert_matches!(
            state_machine
                .schedule_unblocked_task()
                .map(|t| t.task_index()),
            Some(4)
        );
        state_machine.deschedule_task(&task2);
    }

    #[test]
    fn test_schedule_readonly_after_writable() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = transaction_with_writable_address(conflicting_address);
        let sanitized2 = transaction_with_readonly_address(conflicting_address);
        let sanitized3 = transaction_with_readonly_address(conflicting_address);
        let sanitized4 = transaction_with_writable_address(conflicting_address);
        let address_loader = &mut create_address_loader(None);
        let task1 = SchedulingStateMachine::create_task(sanitized1, 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 4, address_loader);
        let task3 = SchedulingStateMachine::create_task(sanitized3, 5, address_loader);
        let task4 = SchedulingStateMachine::create_task(sanitized4, 6, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(state_machine.schedule_task(task1.clone()), Some(_));
        assert_matches!(state_machine.schedule_task(task2.clone()), None);
        assert_matches!(state_machine.schedule_task(task3.clone()), None);
        assert_matches!(state_machine.schedule_task(task4.clone()), None);

        state_machine.deschedule_task(&task1);
        assert_matches!(
            state_machine
                .schedule_unblocked_task()
                .map(|t| t.task_index()),
            Some(4)
        );
        assert_matches!(
            state_machine
                .schedule_unblocked_task()
                .map(|t| t.task_index()),
            Some(5)
        );
        assert_matches!(state_machine.schedule_unblocked_task(), None);
    }

    #[test]
    fn test_rollback() {
        let conflicting_address = Pubkey::new_unique();
        let sanitized1 = transaction_with_writable_address(conflicting_address);
        let sanitized2 = transaction_with_writable_address(conflicting_address);
        let pages = Arc::new(Mutex::new(HashMap::new()));
        let address_loader = &mut create_address_loader(Some(pages.clone()));
        let task1 = SchedulingStateMachine::create_task(sanitized1, 3, address_loader);
        let task2 = SchedulingStateMachine::create_task(sanitized2, 4, address_loader);

        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        assert_matches!(state_machine.schedule_task(task1.clone()), Some(_));
        assert_matches!(state_machine.schedule_task(task2.clone()), None);
        let pages = pages.lock().unwrap();
        let page = pages.get(&conflicting_address).unwrap();
        assert_matches!(
            page.0.borrow_mut(&mut state_machine.page_token).usage,
            Usage::Writable
        );
        let page = pages
            .get(task2.transaction().message().fee_payer())
            .unwrap();
        assert_matches!(
            page.0.borrow_mut(&mut state_machine.page_token).usage,
            Usage::Writable
        );
    }

    #[test]
    #[should_panic(expected = "internal error: entered unreachable code")]
    fn test_unreachable_unlock_conditions() {
        let mut state_machine = unsafe {
            SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling()
        };
        let page = Page::default();
        let _ = SchedulingStateMachine::unlock(
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
        page.0.borrow_mut(&mut state_machine.page_token).usage = Usage::Writable;
        let _ = SchedulingStateMachine::unlock(
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
            Usage::Readonly(ShortCounter::one());
        let _ = SchedulingStateMachine::unlock(
            page.0.borrow_mut(&mut state_machine.page_token),
            &LockAttempt::new(page, RequestedUsage::Writable),
        );
    }
}
