#[macro_use]
extern crate lazy_static;

use std::sync::RwLock;

//TODO remove this hack when rayon fixes itself
lazy_static! {
    static ref MAX_RAYON_THREADS: RwLock<usize> =
        RwLock::new(sys_info::cpu_num().unwrap() as usize);
}

pub fn get_thread_count() -> usize {
    *MAX_RAYON_THREADS.read().unwrap()
}

pub fn set_thread_count(count: usize) {
    *MAX_RAYON_THREADS.write().unwrap() = count;
}
