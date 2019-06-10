use crate::alloc;

use alloc::{Alloc, AllocErr};
use std::alloc::{self as system_alloc, Layout};

#[derive(Debug)]
pub struct BPFAllocator {
    allocated: usize,
    size: usize,
}

impl BPFAllocator {
    pub fn new(heap: Vec<u8>) -> Self {
        Self {
            allocated: 0,
            size: heap.len(),
        }
    }
}

impl Alloc for BPFAllocator {
    fn alloc(&mut self, layout: Layout) -> Result<*mut u8, AllocErr> {
        if self.allocated + layout.size() <= self.size {
            let ptr = unsafe { system_alloc::alloc(layout) };
            if !ptr.is_null() {
                self.allocated += layout.size();
                return Ok(ptr);
            }
        }
        Err(AllocErr)
    }

    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        self.allocated -= layout.size();
        unsafe {
            system_alloc::dealloc(ptr, layout);
        }
    }
}
