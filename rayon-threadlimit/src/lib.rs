#[macro_use]
extern crate lazy_static;

//TODO remove this hack when rayon fixes itself

lazy_static! {
    // reduce the number of threads each pool is allowed to half the cpu core count, to avoid rayon
    // hogging cpu
    static ref MAX_RAYON_THREADS: usize = num_cpus::get() as usize / 2;
}

pub fn get_thread_count() -> usize {
    *MAX_RAYON_THREADS
}
