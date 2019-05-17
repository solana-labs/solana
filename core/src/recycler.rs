use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct Recycler<T> {
    gc: Arc<Mutex<Vec<T>>>,
}

impl<T: Default> Default for Recycler<T> {
    fn default() -> Recycler<T> {
        Recycler {
            gc: Arc::new(Mutex::new(vec![])),
        }
    }
}

impl<T: Default> Clone for Recycler<T> {
    fn clone(&self) -> Recycler<T> {
        Recycler {
            gc: self.gc.clone(),
        }
    }
}

pub trait Reset {
    fn reset(&mut self);
}

impl<T: Default + Reset> Recycler<T> {
    pub fn allocate(&self) -> T {
        let new = self
            .gc
            .lock()
            .expect("recycler lock in pb fn allocate")
            .pop();

        if let Some(mut x) = new {
            x.reset();
            return x;
        } else {
            return T::default();
        }
    }
    pub fn recycle(&self, x: T) {
        let mut gc = self.gc.lock().expect("recycler lock in pub fn recycle");
        gc.push(x);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Reset for u64 {
        fn reset(&mut self) {
            *self = 10;
        }
    }

    #[test]
    fn test_recycler() {
        let recycler = Recycler::default();
        let mut y: u64 = recycler.allocate();
        assert_eq!(y, 0);
        y = 20;
        let recycler2 = recycler.clone();
        recycler2.recycle(y);
        assert_eq!(recycler.gc.lock().unwrap().len(), 1);
        let z = recycler.allocate();
        assert_eq!(z, 10);
        assert_eq!(recycler.gc.lock().unwrap().len(), 0);
    }
}
