use std::fmt;
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// A function that leaves the given type in the same state as Default,
/// but starts with an existing type instead of allocating a new one.
pub trait Reset {
    fn reset(&mut self);
}

/// An value that's returned to its heap once dropped.
pub struct Recyclable<T: Default + Reset> {
    val: Arc<RwLock<T>>,
    landfill: Arc<Mutex<Vec<Arc<RwLock<T>>>>>,
}

impl<T: Default + Reset> Recyclable<T> {
    pub fn read(&self) -> RwLockReadGuard<T> {
        self.val.read().unwrap()
    }
    pub fn write(&self) -> RwLockWriteGuard<T> {
        self.val.write().unwrap()
    }
}

impl<T: Default + Reset> Drop for Recyclable<T> {
    fn drop(&mut self) {
        if Arc::strong_count(&self.val) == 1 {
            // this isn't thread safe, it will allow some concurrent drops to leak and not recycle
            // if that happens the allocator will end up allocating from the heap
            self.landfill.lock().unwrap().push(self.val.clone());
        }
    }
}

impl<T: Default + Reset> Clone for Recyclable<T> {
    fn clone(&self) -> Self {
        Recyclable {
            val: self.val.clone(),
            landfill: self.landfill.clone(),
        }
    }
}

impl<T: fmt::Debug + Default + Reset> fmt::Debug for Recyclable<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Recyclable {:?}", &self.read())
    }
}

/// An object to minimize memory allocations. Use `allocate()`
/// to get recyclable values of type `T`. When those recyclables
/// are dropped, they're returned to the recycler. The next time
/// `allocate()` is called, the value will be pulled from the
/// recycler instead being allocated from memory.

pub struct Recycler<T: Default + Reset> {
    landfill: Arc<Mutex<Vec<Arc<RwLock<T>>>>>,
}
impl<T: Default + Reset> Clone for Recycler<T> {
    fn clone(&self) -> Self {
        Recycler {
            landfill: self.landfill.clone(),
        }
    }
}

impl<T: Default + Reset> Default for Recycler<T> {
    fn default() -> Self {
        Recycler {
            landfill: Arc::new(Mutex::new(vec![])),
        }
    }
}

impl<T: Default + Reset> Recycler<T> {
    pub fn allocate(&self) -> Recyclable<T> {
        let val = self
            .landfill
            .lock()
            .unwrap()
            .pop()
            .map(|val| {
                val.write().unwrap().reset();
                val
            }).unwrap_or_else(|| Arc::new(RwLock::new(Default::default())));
        Recyclable {
            val,
            landfill: self.landfill.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;
    use std::sync::mpsc::channel;

    #[derive(Default)]
    struct Foo {
        x: u8,
    }

    impl Reset for Foo {
        fn reset(&mut self) {
            self.x = 0;
        }
    }

    #[test]
    fn test_allocate() {
        let recycler: Recycler<Foo> = Recycler::default();
        let r = recycler.allocate();
        assert_eq!(r.read().x, 0);
    }

    #[test]
    fn test_recycle() {
        let recycler: Recycler<Foo> = Recycler::default();

        {
            let foo = recycler.allocate();
            foo.write().x = 1;
        }
        assert_eq!(recycler.landfill.lock().unwrap().len(), 1);

        let foo = recycler.allocate();
        assert_eq!(foo.read().x, 0);
        assert_eq!(recycler.landfill.lock().unwrap().len(), 0);
    }
    #[test]
    fn test_channel() {
        let recycler: Recycler<Foo> = Recycler::default();
        let (sender, receiver) = channel();
        {
            let foo = recycler.allocate();
            foo.write().x = 1;
            sender.send(foo).unwrap();
            assert_eq!(recycler.landfill.lock().unwrap().len(), 0);
        }
        {
            let foo = receiver.recv().unwrap();
            assert_eq!(foo.read().x, 1);
            assert_eq!(recycler.landfill.lock().unwrap().len(), 0);
        }
        assert_eq!(recycler.landfill.lock().unwrap().len(), 1);
    }
    #[test]
    fn test_window() {
        let recycler: Recycler<Foo> = Recycler::default();
        let mut window = vec![None];
        let (sender, receiver) = channel();
        {
            // item is in the window while its in the pipeline
            // which is used to serve requests from other threads
            let item = recycler.allocate();
            item.write().x = 1;
            window[0] = Some(item);
            sender.send(window[0].clone().unwrap()).unwrap();
        }
        {
            let foo = receiver.recv().unwrap();
            assert_eq!(foo.read().x, 1);
            let old = mem::replace(&mut window[0], None).unwrap();
            assert_eq!(old.read().x, 1);
        }
        // only one thing should be in the landfill at the end
        assert_eq!(recycler.landfill.lock().unwrap().len(), 1);
    }
}
