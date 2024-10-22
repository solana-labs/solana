//! Ensure RefCell has a stable layout

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        mem::{align_of, size_of},
    };

    #[test]
    fn test_memory_layout() {
        assert_eq!(align_of::<RefCell<i32>>(), 8);
        assert_eq!(size_of::<RefCell<i32>>(), 8 + 4 + /* padding */4);

        let value = 42;
        let refcell = RefCell::new(value);
        let _borrow = refcell.borrow(); // used to increment borrow count

        let addr_refcell = &refcell as *const _ as usize;
        let addr_borrow = addr_refcell;
        let addr_value = addr_refcell + 8;
        assert_eq!(unsafe { *(addr_borrow as *const isize) }, 1);
        assert_eq!(unsafe { *(addr_value as *const i32) }, 42);
    }
}
