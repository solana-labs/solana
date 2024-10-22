//! Ensure Rc has a stable memory layout

#[cfg(test)]
mod tests {
    use std::{
        mem::{align_of, size_of},
        rc::Rc,
    };

    #[test]
    fn test_memory_layout() {
        assert_eq!(align_of::<Rc<i32>>(), 8);
        assert_eq!(size_of::<Rc<i32>>(), 8);

        let value = 42;
        let rc = Rc::new(value);
        let _rc2 = Rc::clone(&rc); // used to increment strong count

        let addr_rc = &rc as *const _ as usize;
        let addr_ptr = addr_rc;
        let addr_rcbox = unsafe { *(addr_ptr as *const *const i32) } as usize;
        let addr_strong = addr_rcbox;
        let addr_weak = addr_rcbox + 8;
        let addr_value = addr_rcbox + 16;
        assert_eq!(unsafe { *(addr_strong as *const usize) }, 2);
        assert_eq!(unsafe { *(addr_weak as *const usize) }, 1);
        assert_eq!(unsafe { *(addr_value as *const i32) }, 42);
    }
}
