use std::sync::{Arc, Mutex};

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
        let mut gc = self.gc.lock().expect("recycler lock in pb fn allocate");

        if let Some(mut x) = gc.pop() {
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
