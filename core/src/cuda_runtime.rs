// Module for cuda-related helper functions and wrappers.
//
// cudaHostRegister/cudaHostUnregister -
//    apis for page-pinning memory. Cuda driver/hardware cannot overlap
//    copies from host memory to GPU memory unless the memory is page-pinned and
//    cannot be paged to disk. The cuda driver provides these interfaces to pin and unpin memory.

use rand::{thread_rng, Rng};
use crate::recycler::Reset;

#[cfg(all(feature = "cuda", feature = "pin_gpu_memory"))]
use crate::sigverify::{cuda_host_register, cuda_host_unregister};
use std::ops::{Deref, DerefMut};

#[cfg(all(feature = "cuda", feature = "pin_gpu_memory"))]
use std::os::raw::c_int;

#[cfg(all(feature = "cuda", feature = "pin_gpu_memory"))]
const CUDA_SUCCESS: c_int = 0;

pub enum CudaError {
    PinError,
    UnpinError,
}

type Result<T> = std::result::Result<T, CudaError>;

pub fn pin<T>(_mem: &mut PinnedVec<T>, from: &'static str, id: u32) -> Result<()> {
    #[cfg(feature = "cuda")]
    unsafe {
        info!("pin: {:?} from: {} id: {}", _mem.as_mut_ptr(), from, id);
        let err = cuda_host_register(
            _mem.Pinnedas_mut_ptr() as *mut c_void,
            _mem.capacity() * size_of::<T>(),
            0,
        );
        if err != CUDA_SUCCESS {
            error!(
                "cudaHostRegister error: {} ptr: {:?} bytes: {}",
                err,
                _mem.as_ptr(),
                _mem.capacity() * size_of::<T>()
            );
            return Err(CudaError::PinError);
        }
    }
    Ok(())
}

pub fn unpin<T>(_mem: *mut T, from: &'static str, id: u32) -> Result<()> {
    #[cfg(feature = "cuda")]
    unsafe {
        info!("unpin: {:?} from: {} id: {}", _mem, from, id);
        let err = cuda_host_unregister(_mem as *mut c_void);
        if err != CUDA_SUCCESS {
            error!("cudaHostUnregister returned: {} ptr: {:?}", err, _mem);
            return Err(CudaError::UnpinError);
        }
    }
    Ok(())
}

// A vector wrapper where the underlying memory can be
// page-pinned. Controlled by flags in case user only wants
// to pin in certain circumstances.
/*#[derive(Debug)]
pub struct PinnedVec<T> {
    x: Pin<Vec<T>>,
    pinned: bool,
    pinnable: bool,
    pub id: u32,
}*/

impl Reset for PinnedVec<u8> {
    fn reset(&mut self) {
        self.resize(0, 0u8);
    }
    fn debug(&self) {
    }
}

impl Reset for PinnedVec<u32> {
    fn reset(&mut self) {
        self.resize(0, 0u32);
    }
    fn debug(&self) {
    }
}

impl<T: Clone> Default for PinnedVec<T> {
    fn default() -> Self {
        PinnedVec::new()
    }
}

/*
impl<T: Clone> Default for PinnedVec<T> {
    fn default() -> Self {
        Self {
            x: Vec::new(),
            pinned: false,
            pinnable: false,
            id: thread_rng().gen_range(0, 100_000),
        }
    }
}

impl<T> Deref for PinnedVec<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.x
    }
}

impl<T> DerefMut for PinnedVec<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.x
    }
}*/

pub struct PinnedIter<'a, T: 'a> {
    _buf: &'a PinnedVec<T>,
    current: usize,
}

pub struct PinnedIterMut<'a, T> {
    _buf: &'a mut PinnedVec<T>,
    current: usize,
}

impl<'a, T> Iterator for PinnedIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current < self._buf.len {
            let ret = Some(&self._buf[self.current]);
            self.current += 1;
            ret
        } else {
            None
        }
    }
}

impl<'a, T> Iterator for PinnedIterMut<'a, T> {
    type Item = &'a mut T;

    fn next<'n>(&'n mut self) -> Option<Self::Item> {
        if self.current < self._buf.len {
            let ret = unsafe { Some(mem::transmute(&mut self._buf[self.current])) };
            self.current += 1;
            ret
        } else {
            None
        }
    }
}

impl<'a, T> IntoIterator for &'a mut PinnedVec<T> {
    type Item = &'a T;
    type IntoIter = PinnedIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        PinnedIter { _buf: self, current: 0 }
    }
}

impl<'a, T> IntoIterator for &'a PinnedVec<T> {
    type Item = &'a T;
    type IntoIter = PinnedIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        PinnedIter { _buf: self, current: 0 }
    }
}

/*
impl<T: Clone> PinnedVec<T> {
    pub fn reserve_and_pin(&mut self, size: usize) -> Result<()> {
        if self.x.capacity() < size {
            if self.pinned {
                unpin(&mut self.x, "reserve_and_pin", self.id)?;
                self.pinned = false;
            }
            self.x.reserve(size);
        }
        self.set_pinnable();
        if !self.pinned {
            pin(&mut self.x, "reserve_and_pin", self.id)?;
            self.pinned = true;
        }
        Ok(())
    }


    pub fn from_vec(source: Vec<T>) -> Self {
        Self {
            x: source,
            pinned: false,
            pinnable: false,
            id: thread_rng().gen_range(0, 100_000),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let x = Vec::with_capacity(capacity);
        Self {
            x,
            pinned: false,
            pinnable: false,
            id: thread_rng().gen_range(0, 100_000),
        }
    }

    pub fn iter(&self) -> PinnedIter<T> {
        PinnedIter(self.x.iter())
    }

    pub fn iter_mut(&mut self) -> PinnedIterMut<T> {
        PinnedIterMut(self.x.iter_mut())
    }

    pub fn is_empty(&self) -> bool {
        self.x.is_empty()
    }

    pub fn len(&self) -> usize {
        self.x.len()
    }

    #[cfg(feature = "cuda")]
    pub fn as_ptr(&self) -> *const T {
        self.x.as_ptr()
    }

    #[cfg(feature = "cuda")]
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.x.as_mut_ptr()
    }

    pub fn push(&mut self, x: T) {
        let old_ptr = self.x.as_mut_ptr();
        let old_capacity = self.x.capacity();
        // Predict realloc and unpin
        if self.pinned && self.x.capacity() == self.x.len() {
            unpin(old_ptr, "push", self.id);
            self.pinned = false;
        }
        self.x.push(x);
        self.check_ptr(old_ptr, old_capacity, "push");
    }

    pub fn resize(&mut self, size: usize, elem: T) {
        let old_ptr = self.x.as_mut_ptr();
        let old_capacity = self.x.capacity();
        // Predict realloc and unpin.
        if self.pinned && self.x.capacity() < size {
            unpin(old_ptr, "resize", self.id);
            self.pinned = false;
        }
        self.x.resize(size, elem);
        self.check_ptr(old_ptr, old_capacity, "resize");
    }

    fn check_ptr(&mut self, _old_ptr: *mut T, _old_capacity: usize, _from: &'static str) {
        #[cfg(feature = "cuda")]
        {
            if self.pinnable {
                info!("check_ptr: {} old: ptr: {:?} new: {:?} cap: old: {} new: {}",
                      self.id, _old_ptr, self.x.as_ptr(), _old_capacity, self.x.capacity());
            }
            if self.pinnable && (self.x.as_ptr() != _old_ptr || self.x.capacity() != _old_capacity)
            {
                if self.pinned {
                    unpin(_old_ptr, "check_ptr", self.id);
                }

                info!(
                    "pinning from check_ptr self: {} old: ptr: {:?} {} size: {} from: {}",
                    self.id,
                    _old_ptr,
                    _old_capacity,
                    self.x.capacity(),
                    _from
                );
                pin(&mut self.x, "check_ptr", self.id);
                self.pinned = true;
            }
        }
    }
}

impl<T: Clone> Clone for PinnedVec<T> {
    fn clone(&self) -> Self {
        let mut x = self.x.clone();
        let pinned = if self.pinned {
            pin(&mut x, "clone", self.id);
            true
        } else {
            false
        };
        info!(
            "clone PinnedVec: {} size: {} pinned?: {} pinnable?: {}",
            self.id,
            self.x.capacity(),
            self.pinned,
            self.pinnable
        );
        Self {
            x,
            pinned,
            pinnable: self.pinnable,
            id: thread_rng().gen_range(0, 100_000),
        }
    }
}

impl<T> Drop for PinnedVec<T> {
    fn drop(&mut self) {
        info!("drop pinned {:?} vec: {:?} pinned: {}", self.id, self.x.as_mut_ptr(), self.pinned);
        if self.pinned {
            unpin(self.x.as_mut_ptr(), "drop", self.id);
        }
    }
}
*/

use std::ptr::{Unique, NonNull, self};
use std::mem;
use std::marker::PhantomData;
use std::alloc::{Alloc, Layout, Global, handle_alloc_error};

#[derive(Debug)]
struct RawVec<T> {
    ptr: Unique<T>,
    capacity: usize,
}

impl <T: Clone> Clone for RawVec<T> {
    fn clone(&self) -> Self {
        let new = RawVec::new();
        new
    }
}

impl<T> RawVec<T> {
    fn new() -> Self {
        // !0 is usize::MAX. This branch should be stripped at compile time.
        let capacity = if mem::size_of::<T>() == 0 { !0 } else { 0 };

        // Unique::empty() doubles as "unallocated" and "zero-sized allocation"
        RawVec { ptr: Unique::empty(), capacity }
    }

    fn with_capacity(capacity: usize) -> Self {
        let mut new = RawVec::new();
        new.allocate(capacity);
        new
    }

    fn allocate(&mut self, new_capacity: usize) {
        self.free();
        let ptr = unsafe { Global.alloc(Layout::array::<T>(new_capacity).unwrap()) };

        // If allocate or reallocate fail, oom
        if ptr.is_err() {
            let elem_size = mem::size_of::<T>();
            unsafe { handle_alloc_error(Layout::from_size_align_unchecked(
                new_capacity * elem_size,
                mem::align_of::<T>(),
            )) }
        }
        let ptr = ptr.unwrap();

        self.ptr = unsafe { Unique::new_unchecked(ptr.as_ptr() as *mut _) };
        self.capacity = new_capacity;
    }

    fn grow(&mut self) {
        let elem_size = mem::size_of::<T>();

        // since we set the capacity to usize::MAX when elem_size is
        // 0, getting to here necessarily means the Vec is overfull.
        assert!(elem_size != 0, "capacity overflow");

        let (new_capacity, ptr) = if self.capacity == 0 {
            let ptr = unsafe { Global.alloc(Layout::array::<T>(1).unwrap()) };
            (1, ptr)
        } else {
            self.free();
            let new_capacity = 2 * self.capacity;
            let ptr = unsafe { Global.alloc(Layout::array::<T>(new_capacity).unwrap()) };
            (new_capacity, ptr)
        };

        self.update_ptr(ptr, new_capacity);
    }

    fn update_ptr(&mut self, ptr: std::result::Result<std::ptr::NonNull<u8>, std::alloc::AllocErr>, new_capacity: usize) {
        let elem_size = mem::size_of::<T>();
        // If allocate or reallocate fail, oom
        if ptr.is_err() {
            unsafe {handle_alloc_error(Layout::from_size_align_unchecked(
                new_capacity * elem_size,
                mem::align_of::<T>(),
            )) }
        }
        let ptr = ptr.unwrap();

        self.ptr = unsafe { Unique::new_unchecked(ptr.as_ptr() as *mut _) };
        self.capacity = new_capacity;
    }

    fn reserve(&mut self, new_capacity: usize) {
        self.free();

        let ptr = unsafe { Global.alloc(Layout::array::<T>(new_capacity).unwrap()) };
        self.update_ptr(ptr, new_capacity);
    }

    fn free(&self) {
        let elem_size = mem::size_of::<T>();
        if self.capacity != 0 && elem_size != 0 {
            unsafe {
                let c: NonNull<T> = self.ptr.into();
                Global.dealloc(c.cast(),
                               Layout::array::<T>(self.capacity).unwrap());
            }
        }
    }

}

impl<T> Drop for RawVec<T> {
    fn drop(&mut self) {
        self.free();
    }
}

#[derive(Debug)]
pub struct PinnedVec<T> {
    buf: RawVec<T>,
    len: usize,
    pinned: bool,
    pinnable: bool,
    pub id: u32,
}

impl<T: Clone> Clone for PinnedVec<T> {
    fn clone(&self) -> Self {
        let buf = self.buf.clone();
        Self {
            buf,
            len: self.len,
            pinned: false,
            pinnable: self.pinnable,
            id: thread_rng().gen_range(0, 100_000),
        }
    }
}

impl<T> PinnedVec<T> {
    pub fn reserve_and_pin(&mut self, size: usize) -> Result<()> {
        if self.buf.capacity < size {
            if self.pinned {
                unpin(self, "reserve_and_pin", self.id)?;
                self.pinned = false;
            }
            self.buf.reserve(size);
        }
        self.set_pinnable();
        if !self.pinned {
            pin(self, "reserve_and_pin", self.id)?;
            self.pinned = true;
        }
        Ok(())
    }

    pub fn resize(&mut self, len: usize, elem: T) {
        self.buf.reserve(len);
        self.len = len;
    }

    pub fn from_vec(source: Vec<T>) -> Self {
        let mut new = PinnedVec::with_capacity(source.len());
        // TODO: copy source to new.buf here
        new
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let buf = RawVec::with_capacity(capacity);
        Self {
            buf,
            len: 0,
            pinned: false,
            pinnable: false,
            id: thread_rng().gen_range(0, 100_000),
        }
    }

    pub fn iter(&self) -> PinnedIter<T> {
        PinnedIter { _buf: self, current: 0 }
    }

    pub fn iter_mut(&mut self) -> PinnedIterMut<'_, T> {
        PinnedIterMut { _buf: self, current: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn truncate(&mut self, len: usize) {
        self.len = len
    }

    pub fn append(&mut self, other: &mut PinnedVec<T>) {
    }

    pub fn set_pinnable(&mut self) {
        self.pinnable = true;
    }

    fn ptr(&self) -> *mut T { self.buf.ptr.as_ptr() }

    fn capacity(&self) -> usize { self.buf.capacity }

    pub fn new() -> Self {
        PinnedVec {
            buf: RawVec::new(),
            pinned: false,
            pinnable: false,
            len: 0,
            id: thread_rng().gen_range(0, 100_000) 
        }
    }
    pub fn push(&mut self, elem: T) {
        if self.len == self.capacity() { self.buf.grow(); }

        unsafe {
            ptr::write(self.ptr().offset(self.len as isize), elem);
        }

        // Can't fail, we'll OOM first.
        self.len += 1;
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            unsafe {
                Some(ptr::read(self.ptr().offset(self.len as isize)))
            }
        }
    }

    pub fn insert(&mut self, index: usize, elem: T) {
        assert!(index <= self.len, "index out of bounds");
        if self.capacity() == self.len { self.buf.grow(); }

        unsafe {
            if index < self.len {
                ptr::copy(self.ptr().offset(index as isize),
                          self.ptr().offset(index as isize + 1),
                          self.len - index);
            }
            ptr::write(self.ptr().offset(index as isize), elem);
            self.len += 1;
        }
    }

    pub fn remove(&mut self, index: usize) -> T {
        assert!(index < self.len, "index out of bounds");
        unsafe {
            self.len -= 1;
            let result = ptr::read(self.ptr().offset(index as isize));
            ptr::copy(self.ptr().offset(index as isize + 1),
                      self.ptr().offset(index as isize),
                      self.len - index);
            result
        }
    }

    pub fn into_iter(self) -> IntoIter<T> {
        unsafe {
            let iter = RawValIter::new(&self);
            let buf = ptr::read(&self.buf);
            mem::forget(self);

            IntoIter {
                iter: iter,
                _buf: buf,
            }
        }
    }

    pub fn drain(&mut self) -> Drain<T> {
        unsafe {
            let iter = RawValIter::new(&self);

            // this is a mem::forget safety thing. If Drain is forgotten, we just
            // leak the whole Vec's contents. Also we need to do this *eventually*
            // anyway, so why not do it now?
            self.len = 0;

            Drain {
                iter: iter,
                vec: PhantomData,
            }
        }
    }
}

impl<T> Drop for PinnedVec<T> {
    fn drop(&mut self) {
        while let Some(_) = self.pop() {}
        // allocation is handled by RawVec
    }
}

impl<T> Deref for PinnedVec<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        unsafe {
            ::std::slice::from_raw_parts(self.ptr(), self.len)
        }
    }
}

impl<T> DerefMut for PinnedVec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        unsafe {
            ::std::slice::from_raw_parts_mut(self.ptr(), self.len)
        }
    }
}





struct RawValIter<T> {
    start: *const T,
    end: *const T,
}

impl<T> RawValIter<T> {
    unsafe fn new(slice: &[T]) -> Self {
        RawValIter {
            start: slice.as_ptr(),
            end: if mem::size_of::<T>() == 0 {
                ((slice.as_ptr() as usize) + slice.len()) as *const _
            } else if slice.len() == 0 {
                slice.as_ptr()
            } else {
                slice.as_ptr().offset(slice.len() as isize)
            }
        }
    }
}

impl<T> Iterator for RawValIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        if self.start == self.end {
            None
        } else {
            unsafe {
                let result = ptr::read(self.start);
                self.start = if mem::size_of::<T>() == 0 {
                    (self.start as usize + 1) as *const _
                } else {
                    self.start.offset(1)
                };
                Some(result)
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let elem_size = mem::size_of::<T>();
        let len = (self.end as usize - self.start as usize)
                  / if elem_size == 0 { 1 } else { elem_size };
        (len, Some(len))
    }
}

impl<T> DoubleEndedIterator for RawValIter<T> {
    fn next_back(&mut self) -> Option<T> {
        if self.start == self.end {
            None
        } else {
            unsafe {
                self.end = if mem::size_of::<T>() == 0 {
                    (self.end as usize - 1) as *const _
                } else {
                    self.end.offset(-1)
                };
                Some(ptr::read(self.end))
            }
        }
    }
}




pub struct IntoIter<T> {
    _buf: RawVec<T>, // we don't actually care about this. Just need it to live.
    iter: RawValIter<T>,
}

impl<T> Iterator for IntoIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> { self.iter.next() }
    fn size_hint(&self) -> (usize, Option<usize>) { self.iter.size_hint() }
}

impl<T> DoubleEndedIterator for IntoIter<T> {
    fn next_back(&mut self) -> Option<T> { self.iter.next_back() }
}

impl<T> Drop for IntoIter<T> {
    fn drop(&mut self) {
        for _ in &mut *self {}
    }
}




pub struct Drain<'a, T: 'a> {
    vec: PhantomData<&'a mut PinnedVec<T>>,
    iter: RawValIter<T>,
}

impl<'a, T> Iterator for Drain<'a, T> {
    type Item = T;
    fn next(&mut self) -> Option<T> { self.iter.next() }
    fn size_hint(&self) -> (usize, Option<usize>) { self.iter.size_hint() }
}

impl<'a, T> DoubleEndedIterator for Drain<'a, T> {
    fn next_back(&mut self) -> Option<T> { self.iter.next_back() }
}

impl<'a, T> Drop for Drain<'a, T> {
    fn drop(&mut self) {
        // pre-drain the iter
        for _ in &mut self.iter {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pinned_vec() {
        let mut mem = PinnedVec::with_capacity(10);
        mem.set_pinnable();
        mem.push(50);
        mem.resize(2, 10);
        assert_eq!(mem[0], 50);
        assert_eq!(mem[1], 10);
        assert_eq!(mem.len(), 2);
        assert_eq!(mem.is_empty(), false);
        let mut iter = mem.iter();
        assert_eq!(*iter.next().unwrap(), 50);
        assert_eq!(*iter.next().unwrap(), 10);
        assert_eq!(iter.next(), None);
    }


    #[test]
    fn create_push_pop() {
        let mut v = Vec::new();
        v.push(1);
        assert_eq!(1, v.len());
        assert_eq!(1, v[0]);
        for i in v.iter_mut() {
            *i += 1;
        }
        v.insert(0, 5);
        let x = v.pop();
        assert_eq!(Some(2), x);
        assert_eq!(1, v.len());
        v.push(10);
        let x = v.remove(0);
        assert_eq!(5, x);
        assert_eq!(1, v.len());
    }

    #[test]
    fn iter_test() {
        let mut v = Vec::new();
        for i in 0..10 {
            v.push(Box::new(i))
        }
        let mut iter = v.into_iter();
        let first = iter.next().unwrap();
        let last = iter.next_back().unwrap();
        drop(iter);
        assert_eq!(0, *first);
        assert_eq!(9, *last);
    }

    #[test]
    fn test_drain() {
        let mut v = Vec::new();
        for i in 0..10 {
            v.push(Box::new(i))
        }
        {
            let mut drain = v.drain();
            let first = drain.next().unwrap();
            let last = drain.next_back().unwrap();
            assert_eq!(0, *first);
            assert_eq!(9, *last);
        }
        assert_eq!(0, v.len());
        v.push(Box::new(1));
        assert_eq!(1, *v.pop().unwrap());
    }

    #[test]
    fn test_zst() {
        let mut v = Vec::new();
        for _i in 0..10 {
            v.push(())
        }

        let mut count = 0;

        for _ in v.into_iter() {
            count += 1
        }

        assert_eq!(10, count);
    }

}
