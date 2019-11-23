extern crate sys_info;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::{spawn, yield_now, JoinHandle};

struct Job {
    func: Box<dyn Fn(*mut u64, usize, usize)>,
    elems: *mut u64,
    num: usize,
    work_index: AtomicUsize,
    done_index: AtomicUsize,
}
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

pub struct Pool {
    senders: Vec<Sender<Arc<Job>>>,
    threads: Vec<JoinHandle<()>>,
}

impl Job {
    fn execute(&self) {
        loop {
            let index = self.work_index.fetch_add(1, Ordering::Relaxed);
            if index >= self.num {
                self.done_index.fetch_add(1, Ordering::Relaxed);
                break;
            }
            (self.func)(self.elems, self.num, index);
        }
    }
    fn wait(&self, num: usize) {
        loop {
            let guard = self.done_index.load(Ordering::Relaxed);
            if guard >= num {
                break;
            }
            yield_now();
        }
    }
}

impl Default for Pool {
    fn default() -> Self {
        let num_threads = sys_info::cpu_num().unwrap_or(16) - 1;
        let mut pool = Self {
            senders: vec![],
            threads: vec![],
        };
        (0..num_threads).for_each(|_| {
            let (sender, recvr): (Sender<Arc<Job>>, Receiver<Arc<Job>>) = channel();
            let t = spawn(move || {
                for job in recvr.iter() {
                    job.execute()
                }
            });
            pool.senders.push(sender);
            pool.threads.push(t);
        });
        pool
    }
}

impl Pool {
    pub fn dispatch_mut<F, A>(&self, elems: &mut [A], func: F)
    where
        F: Fn(&mut A) + 'static,
    {
        let job = Job {
            elems: elems.as_mut_ptr() as *mut u64,
            num: elems.len(),
            done_index: AtomicUsize::new(0),
            work_index: AtomicUsize::new(0),
            func: Box::new(move |ptr, num, index| {
                let ptr = ptr as *mut A;
                let slice = unsafe { std::slice::from_raw_parts_mut(ptr, num) };
                func(&mut slice[index])
            }),
        };
        let job = Arc::new(job);
        for s in &self.senders {
            s.send(job.clone()).expect("send should never fail");
        }
        job.execute();
        job.wait(self.senders.len() + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_pool() {
        let pool = Pool::default();
        let mut array = [0usize; 100];
        pool.dispatch_mut(&mut array, |val: &mut usize| *val += 1);
        let expected = [1usize; 100];
        for i in 0..100 {
            assert_eq!(array[i], expected[i]);
        }
    }
}
