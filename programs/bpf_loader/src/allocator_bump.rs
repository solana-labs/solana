use crate::alloc;

use alloc::{Alloc, AllocErr};
use std::alloc::Layout;

#[derive(Debug)]
pub struct BPFAllocator {
    heap: Vec<u8>,
    start: u64,
    len: u64,
    pos: u64,
}

impl BPFAllocator {
    pub fn new(heap: Vec<u8>, virtual_address: u64) -> Self {
        let len = heap.len() as u64;
        Self {
            heap,
            start: virtual_address,
            len,
            pos: 0,
        }
    }
}

impl Alloc for BPFAllocator {
    fn alloc(&mut self, layout: Layout) -> Result<u64, AllocErr> {
        if self.pos.saturating_add(layout.size() as u64) <= self.len {
            let addr = self.start + self.pos;
            self.pos += layout.size() as u64;
            Ok(addr)
        } else {
            Err(AllocErr)
        }
    }

    fn dealloc(&mut self, _addr: u64, _layout: Layout) {
        // It's a bump allocator, free not supported
    }
}
