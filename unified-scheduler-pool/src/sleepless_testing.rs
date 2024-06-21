use std::{
    io,
    thread::{JoinHandle, Scope, ScopedJoinHandle},
};

#[allow(dead_code)]
pub(crate) trait ScopeTracked<'scope>: Sized {
    fn spawn_tracked<T: Send + 'scope>(
        &'scope self,
        f: impl FnOnce() -> T + Send + 'scope,
    ) -> ScopedJoinHandle<'scope, T>;
}

pub(crate) trait BuilderTracked: Sized {
    fn spawn_tracked<T: Send + 'static>(
        self,
        f: impl FnOnce() -> T + Send + 'static,
    ) -> io::Result<JoinHandle<T>>;

    #[allow(dead_code)]
    fn spawn_scoped_tracked<'scope, 'env, T: Send + 'scope>(
        self,
        scope: &'scope Scope<'scope, 'env>,
        f: impl FnOnce() -> T + Send + 'scope,
    ) -> io::Result<ScopedJoinHandle<'scope, T>>;
}

#[cfg(not(test))]
pub(crate) use dummy::*;
#[cfg(test)]
pub(crate) use real::*;

#[cfg(test)]
mod real {
    use {
        lazy_static::lazy_static,
        log::trace,
        std::{
            cmp::Ordering::{Equal, Greater, Less},
            collections::HashMap,
            fmt::Debug,
            sync::{Arc, Condvar, Mutex},
            thread::{current, panicking, JoinHandle, ThreadId},
        },
    };

    #[derive(Debug)]
    struct Progress {
        _name: String,
        check_points: Vec<String>,
        current_index: Mutex<usize>,
        condvar: Condvar,
    }

    #[derive(Debug)]
    struct JustCreated;

    impl Progress {
        fn new(check_points: impl Iterator<Item = String>, name: String) -> Self {
            let initial_check_point = format!("{JustCreated:?}");
            let check_points = [initial_check_point.clone()]
                .into_iter()
                .chain(check_points)
                .collect::<Vec<_>>();
            Self {
                _name: name,
                check_points,
                current_index: Mutex::new(0),
                condvar: Condvar::new(),
            }
        }

        fn change_current_check_point(&self, anchored_check_point: String) {
            let mut current_index = self.current_index.lock().unwrap();

            let Some(anchored_index) = self.anchored_index(*current_index, &anchored_check_point)
            else {
                trace!("Ignore {} at {:?}", anchored_check_point, current());
                return;
            };

            let next_index = self.expected_next_index(*current_index);
            let should_change = match anchored_index.cmp(&next_index) {
                Equal => true,
                Greater => {
                    trace!("Blocked on {} at {:?}", anchored_check_point, current());
                    // anchor is one of future check points; block the current thread until
                    // that happens
                    current_index = self
                        .condvar
                        .wait_while(current_index, |&mut current_index| {
                            let Some(anchored_index) =
                                self.anchored_index(current_index, &anchored_check_point)
                            else {
                                // don't wait. seems the progress is made by other threads
                                // anchored to the same checkpoint.
                                return false;
                            };
                            let next_index = self.expected_next_index(current_index);

                            // determine we should wait further or not
                            match anchored_index.cmp(&next_index) {
                                Equal => false,
                                Greater => {
                                    trace!(
                                        "Re-blocked on {} ({} != {}) at {:?}",
                                        anchored_check_point,
                                        anchored_index,
                                        next_index,
                                        current()
                                    );
                                    true
                                }
                                Less => unreachable!(),
                            }
                        })
                        .unwrap();
                    true
                }
                Less => unreachable!(),
            };

            if should_change {
                if *current_index != anchored_index {
                    trace!("Progressed to: {} at {:?}", anchored_check_point, current());
                    *current_index = anchored_index;
                }

                self.condvar.notify_all();
            }
        }

        fn expected_next_index(&self, current_index: usize) -> usize {
            current_index.checked_add(1).unwrap()
        }

        fn anchored_index(
            &self,
            current_index: usize,
            anchored_check_point: &String,
        ) -> Option<usize> {
            self.check_points[current_index..]
                .iter()
                .position(|check_point| check_point == anchored_check_point)
                .map(|subslice_index| subslice_index.checked_add(current_index).unwrap())
        }
    }

    lazy_static! {
        static ref THREAD_REGISTRY: Mutex<HashMap<ThreadId, Arc<Progress>>> =
            Mutex::new(HashMap::new());
    }

    #[must_use]
    pub(crate) struct ActiveProgress(Arc<Progress>, ThreadId);

    impl ActiveProgress {
        fn new(progress: Arc<Progress>) -> Self {
            let active_progress = Self(progress, current().id());
            active_progress.activate();
            active_progress
        }

        fn activate(&self) {
            assert!(THREAD_REGISTRY
                .lock()
                .unwrap()
                .insert(self.1, self.0.clone())
                .is_none());
        }

        fn deactivate(&self) {
            if !panicking() {
                assert_eq!(
                    self.0.check_points.len().checked_sub(1).unwrap(),
                    *self.0.current_index.lock().unwrap(),
                    "unfinished progress"
                );
            }
            THREAD_REGISTRY.lock().unwrap().remove(&self.1).unwrap();
        }
    }

    impl Drop for ActiveProgress {
        fn drop(&mut self) {
            self.deactivate();
        }
    }

    /// Enable sleepless_testing with given check points being monitored, until the returned value
    /// is dropped. This guarantees the check points are linearized in the exact order as
    /// specified, among all of tracked threads.
    pub(crate) fn setup(check_points: &[&dyn Debug]) -> ActiveProgress {
        let progress = Arc::new(Progress::new(
            check_points
                .iter()
                .map(|check_point| format!("{check_point:?}")),
            current().name().unwrap_or_default().to_string(),
        ));
        ActiveProgress::new(progress)
    }

    /// Signal about the passage of the given check point. If sleepless_testing is enabled with the
    /// check point monitored, this may block the current thread, not to violate the enforced order
    /// of monitored check points.
    pub(crate) fn at<T: Debug>(check_point: T) {
        let mut registry = THREAD_REGISTRY.lock().unwrap();
        if let Some(progress) = registry.get_mut(&current().id()).cloned() {
            drop(registry);
            progress.change_current_check_point(format!("{check_point:?}"));
        } else if current().name().unwrap_or_default().starts_with("test_") {
            panic!("seems setup() isn't called yet?");
        }
    }

    pub(crate) mod thread {
        pub(crate) use crate::sleepless_testing::{BuilderTracked, ScopeTracked};
        use {
            super::*,
            std::{
                io,
                thread::{current, spawn, Builder, Scope, ScopedJoinHandle},
            },
        };

        struct SpawningThreadTracker(Arc<(Mutex<bool>, Condvar)>);
        struct SpawnedThreadTracker(Arc<(Mutex<bool>, Condvar)>, ThreadId, bool);

        impl SpawningThreadTracker {
            fn ensure_spawned_tracked(self) {
                let (lock, cvar) = &*self.0;
                let lock = lock.lock().unwrap();
                assert!(cvar.wait_while(lock, |&mut tracked| !tracked).is_ok());
            }
        }

        impl SpawnedThreadTracker {
            fn do_track(&mut self) {
                self.2 = {
                    let mut registry = THREAD_REGISTRY.lock().unwrap();
                    if let Some(progress) = registry.get(&self.1).cloned() {
                        assert!(registry.insert(current().id(), progress).is_none());
                        true
                    } else {
                        false
                    }
                };
                let (lock, cvar) = &*self.0;
                *lock.lock().unwrap() = true;
                cvar.notify_one();
            }

            fn do_untrack(self) {
                if self.2 {
                    let mut registry = THREAD_REGISTRY.lock().unwrap();
                    registry.remove(&current().id()).unwrap();
                }
            }

            fn with_tracked<T: Send>(mut self, f: impl FnOnce() -> T + Send) -> T {
                self.do_track();
                let returned = f();
                self.do_untrack();
                returned
            }
        }

        fn prepare_tracking() -> (SpawningThreadTracker, SpawnedThreadTracker) {
            let lock_and_condvar1 = Arc::new((Mutex::new(false), Condvar::new()));
            let lock_and_condvar2 = lock_and_condvar1.clone();
            let spawning_thread_tracker = SpawningThreadTracker(lock_and_condvar1);
            let spawning_thread_id = current().id();
            let spawned_thread_tracker =
                SpawnedThreadTracker(lock_and_condvar2, spawning_thread_id, false);
            (spawning_thread_tracker, spawned_thread_tracker)
        }

        #[allow(dead_code)]
        pub(crate) fn spawn_tracked<T: Send + 'static>(
            f: impl FnOnce() -> T + Send + 'static,
        ) -> JoinHandle<T> {
            let (spawning_thread_tracker, spawned_thread_tracker) = prepare_tracking();
            let spawned_thread = spawn(move || spawned_thread_tracker.with_tracked(f));
            spawning_thread_tracker.ensure_spawned_tracked();
            spawned_thread
        }

        impl<'scope, 'env> ScopeTracked<'scope> for Scope<'scope, 'env> {
            fn spawn_tracked<T: Send + 'scope>(
                &'scope self,
                f: impl FnOnce() -> T + Send + 'scope,
            ) -> ScopedJoinHandle<'scope, T> {
                let (spawning_thread_tracker, spawned_thread_tracker) = prepare_tracking();
                let spawned_thread = self.spawn(move || spawned_thread_tracker.with_tracked(f));
                spawning_thread_tracker.ensure_spawned_tracked();
                spawned_thread
            }
        }

        impl BuilderTracked for Builder {
            fn spawn_tracked<T: Send + 'static>(
                self,
                f: impl FnOnce() -> T + Send + 'static,
            ) -> io::Result<JoinHandle<T>> {
                let (spawning_thread_tracker, spawned_thread_tracker) = prepare_tracking();
                let spawned_thread_result =
                    self.spawn(move || spawned_thread_tracker.with_tracked(f));
                if spawned_thread_result.is_ok() {
                    spawning_thread_tracker.ensure_spawned_tracked();
                }
                spawned_thread_result
            }

            fn spawn_scoped_tracked<'scope, 'env, T: Send + 'scope>(
                self,
                scope: &'scope Scope<'scope, 'env>,
                f: impl FnOnce() -> T + Send + 'scope,
            ) -> io::Result<ScopedJoinHandle<'scope, T>> {
                let (spawning_thread_tracker, spawned_thread_tracker) = prepare_tracking();
                let spawned_thread_result =
                    self.spawn_scoped(scope, move || spawned_thread_tracker.with_tracked(f));
                if spawned_thread_result.is_ok() {
                    spawning_thread_tracker.ensure_spawned_tracked();
                }
                spawned_thread_result
            }
        }
    }
}

#[cfg(not(test))]
mod dummy {
    use std::fmt::Debug;

    #[inline]
    pub(crate) fn at<T: Debug>(_check_point: T) {}

    pub(crate) mod thread {
        pub(crate) use crate::sleepless_testing::{BuilderTracked, ScopeTracked};
        use std::{
            io,
            thread::{spawn, Builder, JoinHandle, Scope, ScopedJoinHandle},
        };

        #[inline]
        #[allow(dead_code)]
        pub(crate) fn spawn_tracked<T: Send + 'static>(
            f: impl FnOnce() -> T + Send + 'static,
        ) -> JoinHandle<T> {
            spawn(f)
        }

        impl<'scope, 'env> ScopeTracked<'scope> for Scope<'scope, 'env> {
            #[inline]
            fn spawn_tracked<T: Send + 'scope>(
                &'scope self,
                f: impl FnOnce() -> T + Send + 'scope,
            ) -> ScopedJoinHandle<'scope, T> {
                self.spawn(f)
            }
        }

        impl BuilderTracked for Builder {
            #[inline]
            fn spawn_tracked<T: Send + 'static>(
                self,
                f: impl FnOnce() -> T + Send + 'static,
            ) -> io::Result<JoinHandle<T>> {
                self.spawn(f)
            }

            #[inline]
            fn spawn_scoped_tracked<'scope, 'env, T: Send + 'scope>(
                self,
                scope: &'scope Scope<'scope, 'env>,
                f: impl FnOnce() -> T + Send + 'scope,
            ) -> io::Result<ScopedJoinHandle<'scope, T>> {
                self.spawn_scoped(scope, f)
            }
        }
    }
}
