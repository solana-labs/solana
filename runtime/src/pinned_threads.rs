use rayon::ThreadPool;
use rayon_core::{ThreadBuilder, ThreadPoolBuilder};
use std::{io, thread};

const NUM_THREADS_PER_CORE: usize = 8;
const MAX_NUM_OF_THREADS: usize = 128;

#[derive(Debug, Default)]
pub struct PinnedSpawn {
    cores: Vec<core_affinity::CoreId>,
    len: usize,
    core_id_pointer: usize,
}

impl PinnedSpawn {
    // pub fn new(num_cores: usize) -> Self {
    //     let core_ids = core_affinity::get_core_ids().unwrap();
    //     if num_cores > core_ids.len() {
    //         panic!("More cores requested than available");
    //     }
    //     Self {
    //         cores: core_ids.into_iter().rev().take(num_cores).collect(),
    //         len: num_cores,
    //         core_id_pointer: 0,
    //     }
    // }

    // Pins as many threads as the ceil of the fraction times the total number of cores
    // This ensures that at least 1 core would be pinned
    pub fn new_frac_of_cores(num: usize, denom: usize) -> Self {
        if num > denom {
            panic!("fraction must be <= 1");
        }
        let core_ids = core_affinity::get_core_ids().unwrap();
        let num_cores = (num * core_ids.len() - 1) / denom + 1;
        Self {
            cores: core_ids.into_iter().rev().take(num_cores).collect(),
            len: num_cores,
            core_id_pointer: 0,
        }
    }

    // Spawn threads pinned to core in a round robin fashion
    pub fn spawn(&mut self, thread: ThreadBuilder) -> io::Result<()> {
        let mut b = thread::Builder::new();
        if let Some(name) = thread.name() {
            b = b.name(name.to_owned());
        }
        if let Some(stack_size) = thread.stack_size() {
            b = b.stack_size(stack_size);
        }
        let id_for_spawn = self.cores[self.core_id_pointer];
        b.spawn(move || {
            core_affinity::set_for_current(id_for_spawn);
            thread.run()
        })?;
        self.core_id_pointer += 1;
        self.core_id_pointer %= self.len;
        Ok(())
    }
}

pub fn pinned_spawn_handler_frac(num: usize, denom: usize) -> ThreadPool {
    let mut spawner = PinnedSpawn::new_frac_of_cores(num, denom);
    ThreadPoolBuilder::new()
        .thread_name(|i| format!("pinned-thread-for-parallel-load-{}", i))
        .num_threads(std::cmp::min(
            spawner.len * NUM_THREADS_PER_CORE,
            MAX_NUM_OF_THREADS,
        ))
        .spawn_handler(|thread| spawner.spawn(thread))
        .build()
        .unwrap()
}
