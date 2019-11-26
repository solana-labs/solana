extern crate sys_info;

use job::Job;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, Arc};
use std::thread::{spawn};

#[derive(Debug)]
pub struct Pool {
    senders: Mutex<Vec<Sender<Arc<Job>>>>,
}

impl Default for Pool {
    fn default() -> Self {
        let num_threads = sys_info::cpu_num().unwrap_or(16) - 1;
        let mut senders = vec![];
        (0..num_threads).for_each(|_| {
            let (sender, recvr): (Sender<Arc<Job>>, Receiver<Arc<Job>>) = channel();
            let _ = spawn(move || {
                for job in recvr.iter() {
                    job.execute()
                }
            });
            senders.push(sender);
        });
        Self {
            senders: Mutex::new(senders),
        }
    }
}

impl Pool {
    pub fn dispatch_mut<F, A>(&self, elems: &mut [A], func: F)
    where
        F: Fn(&mut A) + Send + Sync,
    {
        // Job must be destroyed in the frame that its created
        let job = unsafe { Job::new(elems, func) };
        let job = Arc::new(job);
        let len = self.notify_all(job.clone());
        job.execute();
        job.wait(len + 1);
    }
    pub fn map<F, A, B>(&self, inputs: &[A], func: F) -> Vec<B>
    where
        B: Default + Clone,
        F: (Fn(&A) -> B) + Send + Sync,
    {
        let mut outs = Vec::new();
        outs.resize(inputs.len(), B::default());
        let mut elems: Vec<(&A, &mut B)> = inputs.iter().zip(outs.iter_mut()).collect();
        self.dispatch_mut(&mut elems, move |item: &mut (&A, &mut B)| {
            *item.1 = func(item.0);
        });
        outs
    }
    fn notify_all(&self, job: Arc<Job>) -> usize {
        let senders = self.senders.lock().unwrap();
        let len = senders.len();
        for s in senders.iter() {
            s.send(job.clone()).expect("send should never fail");
        }
        len
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_pool() {
        let pool = Pool::default();
        let mut array = [0usize; 100];
        pool.dispatch_mut(&mut array, Box::new(|val: &mut usize| *val += 1));
        let expected = [1usize; 100];
        for i in 0..100 {
            assert_eq!(array[i], expected[i]);
        }
    }
    #[test]
    fn test_map() {
        let pool = Pool::default();
        let array = [0usize; 100];
        let output = pool.map(&array, |val: &usize| val + 1);
        let expected = [1usize; 100];
        for i in 0..100 {
            assert_eq!(expected[i], output[i]);
        }
    }
}
