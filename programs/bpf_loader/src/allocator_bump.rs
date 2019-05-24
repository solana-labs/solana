use crate::alloc;

use alloc::{Alloc, AllocErr};
use std::alloc::Layout;

#[derive(Debug)]
pub struct BPFAllocator {
    heap: Vec<u8>,
    pos: usize,
}

impl BPFAllocator {
    pub fn new(heap: Vec<u8>) -> Self {
        Self { heap, pos: 0 }
    }
}

impl Alloc for BPFAllocator {
    fn alloc(&mut self, layout: Layout) -> Result<*mut u8, AllocErr> {
        if self.pos + layout.size() <= self.heap.len() {
            let ptr = unsafe { self.heap.as_mut_ptr().add(self.pos) };
            self.pos += layout.size();
            Ok(ptr)
        } else {
            Err(AllocErr)
        }
    }

    fn dealloc(&mut self, _ptr: *mut u8, _layout: Layout) {
        // It's a bump allocator, free not supported
    }
}
