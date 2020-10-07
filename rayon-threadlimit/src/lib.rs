#[macro_use]
extern crate lazy_static;

use std::env;
//TODO remove this hack when rayon fixes itself

lazy_static! {
    // reduce the number of threads each pool is allowed to half the cpu core count, to avoid rayon
    // hogging cpu
    static ref MAX_RAYON_THREADS: usize =
            env::var("SOLANA_RAYON_THREADS")
                .map(|x| x.parse().unwrap_or(num_cpus::get() as usize / 2))
                .unwrap_or(num_cpus::get() as usize / 2);
}

pub fn get_thread_count() -> usize {
    *MAX_RAYON_THREADS
}
