use std::alloc::Layout;
use std::fmt;

/// Based loosely on the unstable std::alloc::Alloc trait
pub trait Alloc {
    fn alloc(&mut self, layout: Layout) -> Result<*mut u8, AllocErr>;
    fn dealloc(&mut self, ptr: *mut u8, layout: Layout);
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AllocErr;

impl fmt::Display for AllocErr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Error: Memory allocation failed")
    }
}
