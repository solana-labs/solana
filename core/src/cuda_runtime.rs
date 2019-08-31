// Module for cuda-related helper functions and wrappers.
//
// cudaHostRegister/cudaHostUnregister -
//    apis for page-pinning memory. Cuda driver/hardware cannot overlap
//    copies from host memory to GPU memory unless the memory is page-pinned and
//    cannot be paged to disk. The cuda driver provides these interfaces to pin and unpin memory.

use crate::recycler::Reset;
use core::ops::Bound::{Excluded, Included, Unbounded};
use core::slice;
use rand::{thread_rng, Rng};
use std::alloc::{handle_alloc_error, Alloc, Global, Layout};
use std::convert::TryInto;
use std::marker::PhantomData;
use std::mem;
use std::ops::RangeBounds;
use std::ptr::{self, NonNull, Unique};

#[cfg(feature = "cuda")]
use crate::sigverify::{cuda_host_register, cuda_host_unregister};
use std::ops::{Deref, DerefMut};

#[cfg(feature = "cuda")]
use std::os::raw::c_int;

#[cfg(feature = "cuda")]
const CUDA_SUCCESS: c_int = 0;

#[cfg(feature = "cuda")]
use std::ffi::c_void;

#[cfg(feature = "cuda")]
use std::mem::size_of;

pub enum CudaError {
    PinError,
    UnpinError,
}

type Result<T> = std::result::Result<T, CudaError>;

fn pin<T>(_mem: &mut RawVec<T>, _from: &'static str, _id: u32) -> Result<()> {
    #[cfg(feature = "cuda")]
    unsafe {
        info!(
            "pin: {:?} from: {} id: {} bytes: {}",
            _mem.ptr.as_ptr(),
            _from,
            _id,
            _mem.capacity * size_of::<T>()
        );
        let err = cuda_host_register(
            _mem.ptr.as_ptr() as *mut c_void,
            _mem.capacity * size_of::<T>(),
            0,
        );
        if err != CUDA_SUCCESS {
            error!(
                "cudaHostRegister error: {} ptr: {:?} bytes: {}",
                err,
                _mem.ptr.as_ptr(),
                _mem.capacity * size_of::<T>()
            );
            return Err(CudaError::PinError);
        }
    }
    Ok(())
}

fn unpin<T>(_mem: *mut T, _from: &'static str, _id: u32) -> Result<()> {
    #[cfg(feature = "cuda")]
    unsafe {
        info!("unpin: {:?} from: {} id: {}", _mem, _from, _id);
        let err = cuda_host_unregister(_mem as *mut c_void);
        if err != CUDA_SUCCESS {
            error!("cudaHostUnregister returned: {} ptr: {:?}", err, _mem);
            return Err(CudaError::UnpinError);
        }
    }
    Ok(())
}

impl Reset for PinnedVec<u8> {
    fn reset(&mut self) {
        self.resize(0, 0u8);
    }
}

impl Reset for PinnedVec<u32> {
    fn reset(&mut self) {
        self.resize(0, 0u32);
    }
}

impl<T: Clone> Default for PinnedVec<T> {
    fn default() -> Self {
        PinnedVec::new()
    }
}

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
        PinnedIter {
            _buf: self,
            current: 0,
        }
    }
}

impl<'a, T> IntoIterator for &'a PinnedVec<T> {
    type Item = &'a T;
    type IntoIter = PinnedIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        PinnedIter {
            _buf: self,
            current: 0,
        }
    }
}

#[derive(Debug)]
struct RawVec<T> {
    ptr: Unique<T>,
    capacity: usize,
    pinned: bool,
    pinnable: bool,
    id: u32,
}

impl<T: Clone> Clone for RawVec<T> {
    fn clone(&self) -> Self {
        let new = RawVec::with_capacity(self.capacity, self.pinnable, 0);
        unsafe { std::ptr::copy(new.ptr.as_ptr(), self.ptr.as_ptr(), self.capacity) };
        new
    }
}

impl<T> RawVec<T> {
    fn new(id: u32) -> Self {
        // !0 is usize::MAX. This branch should be stripped at compile time.
        let capacity = if mem::size_of::<T>() == 0 { !0 } else { 0 };

        // Unique::empty() doubles as "unallocated" and "zero-sized allocation"
        RawVec {
            ptr: Unique::empty(),
            capacity,
            pinned: false,
            pinnable: false,
            id,
        }
    }

    fn with_capacity(capacity: usize, pinnable: bool, id: u32) -> Self {
        let mut new = RawVec::new(id);
        new.pinnable = pinnable;
        new.allocate(capacity);
        new
    }

    fn allocate(&mut self, new_capacity: usize) {
        self.free();
        let ptr = unsafe { Global.alloc(Layout::array::<T>(new_capacity).unwrap()) };

        // If allocate or reallocate fail, oom
        if ptr.is_err() {
            let elem_size = mem::size_of::<T>();
            unsafe {
                handle_alloc_error(Layout::from_size_align_unchecked(
                    new_capacity * elem_size,
                    mem::align_of::<T>(),
                ))
            }
        }
        let ptr = ptr.unwrap();

        self.ptr = unsafe { Unique::new_unchecked(ptr.as_ptr() as *mut _) };
        self.capacity = new_capacity;
    }

    fn grow(&mut self, old_len: usize) {
        let elem_size = mem::size_of::<T>();

        // since we set the capacity to usize::MAX when elem_size is
        // 0, getting to here necessarily means the Vec is overfull.
        assert!(elem_size != 0, "capacity overflow");

        let (new_capacity, ptr) = if self.capacity == 0 {
            let ptr = self.allocate2(1);
            (1, ptr)
        } else {
            let new_capacity = 2 * self.capacity;
            let ptr = self.allocate2(new_capacity);
            unsafe { std::ptr::copy(self.ptr.as_ptr(), ptr.as_ptr(), old_len) };
            self.free();
            (new_capacity, ptr)
        };

        self.update_ptr(ptr, new_capacity);
    }

    fn update_ptr(&mut self, ptr: Unique<T>, new_capacity: usize) {
        self.ptr = ptr;
        self.capacity = new_capacity;

        if self.pinnable && pin(self, "update_ptr", self.id).is_ok() {
            self.pinned = true;
        }
    }

    fn allocate2(&mut self, new_capacity: usize) -> Unique<T> {
        let ptr = unsafe { Global.alloc(Layout::array::<T>(new_capacity).unwrap()) };

        let elem_size = mem::size_of::<T>();
        // If allocate or reallocate fail, oom
        if ptr.is_err() {
            unsafe {
                handle_alloc_error(Layout::from_size_align_unchecked(
                    new_capacity * elem_size,
                    mem::align_of::<T>(),
                ))
            }
        }

        let ptr = ptr.unwrap();
        unsafe { Unique::new_unchecked(ptr.as_ptr() as *mut _) }
    }

    fn reserve(&mut self, new_capacity: usize, old_len: usize) {
        if new_capacity > self.capacity {
            let ptr = self.allocate2(new_capacity);
            unsafe { std::ptr::copy(self.ptr.as_ptr(), ptr.as_ptr(), old_len) };
            self.free();
            self.update_ptr(ptr, new_capacity);
        }
    }

    fn free(&self) {
        let elem_size = mem::size_of::<T>();
        if self.capacity != 0 && elem_size != 0 {
            unsafe {
                let c: NonNull<T> = self.ptr.into();
                if self.pinned {
                    let _unused = unpin(c.as_ptr(), "rawbuf::free", self.id);
                }
                Global.dealloc(c.cast(), Layout::array::<T>(self.capacity).unwrap());
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
    pinnable: bool,
    pub id: u32,
}

impl<T: Clone> Clone for PinnedVec<T> {
    fn clone(&self) -> Self {
        let mut buf = self.buf.clone();
        let id = thread_rng().gen_range(1, 100_000);
        buf.id = id;
        Self {
            buf,
            len: self.len,
            pinnable: self.pinnable,
            id,
        }
    }
}

impl<T: Clone> PinnedVec<T> {
    pub fn resize(&mut self, len: usize, elem: T) {
        self.buf.reserve(len, self.len);
        if self.len < len {
            for i in 0..(len - self.len) {
                unsafe {
                    ptr::write(self.ptr().offset((i + self.len) as isize), elem.clone());
                }
            }
        }
        self.len = len;
    }
}

impl<T: std::fmt::Debug> PinnedVec<T> {
    pub fn push(&mut self, elem: T) {
        if self.len == self.capacity() {
            self.buf.grow(self.len);
        }

        unsafe {
            ptr::write(self.ptr().offset(self.len as isize), elem);
        }

        // Can't fail, we'll OOM first.
        self.len += 1;
    }
}

impl<T> PinnedVec<T> {
    pub fn reserve_and_pin(&mut self, size: usize) {
        if self.buf.capacity < size {
            self.buf.reserve(size, self.len);
        }
        self.set_pinnable();
    }

    pub fn from_vec(source: Vec<T>) -> Self {
        let mut new = PinnedVec::with_capacity(source.len());
        new.len = source.len();
        unsafe { ptr::copy(source.as_ptr(), new.ptr(), source.len()) };
        new
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let id = thread_rng().gen_range(1, 100_000);
        let buf = RawVec::with_capacity(capacity, false, id);
        Self {
            buf,
            len: 0,
            pinnable: false,
            id,
        }
    }

    pub fn iter(&self) -> PinnedIter<T> {
        PinnedIter {
            _buf: self,
            current: 0,
        }
    }

    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        let mut len = self.len as isize - 1;
        while len >= 0 {
            if (f)(&mut self[len as usize]) {
                self.remove(len as usize);
                len -= 1;
            }
            len -= 1;
        }
    }

    pub fn iter_mut(&mut self) -> PinnedIterMut<'_, T> {
        PinnedIterMut {
            _buf: self,
            current: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn truncate(&mut self, len: usize) {
        if len < self.len {
            self.len = len
        }
    }

    pub fn append(&mut self, other: &mut PinnedVec<T>) {
        let new_len = self.len() + other.len();
        if self.buf.capacity < new_len {
            let new_buf = RawVec::with_capacity(new_len, self.pinnable, self.id);
            unsafe { std::ptr::copy(self.ptr(), new_buf.ptr.as_ptr(), self.len()) };
            self.buf = new_buf;
        }
        unsafe {
            std::ptr::copy(
                other.ptr(),
                self.ptr().offset(self.len().try_into().unwrap()),
                other.len(),
            )
        };
        other.len = 0;
        self.len = new_len
    }

    pub fn set_pinnable(&mut self) {
        self.pinnable = true;
        self.buf.pinnable = true;
    }

    fn ptr(&self) -> *mut T {
        self.buf.ptr.as_ptr()
    }

    fn capacity(&self) -> usize {
        self.buf.capacity
    }

    pub fn new() -> Self {
        let id = thread_rng().gen_range(1, 100_000);
        PinnedVec {
            buf: RawVec::new(id),
            pinnable: false,
            len: 0,
            id,
        }
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            unsafe { Some(ptr::read(self.ptr().offset(self.len as isize))) }
        }
    }

    pub fn insert(&mut self, index: usize, elem: T) {
        assert!(index <= self.len, "index out of bounds");
        if self.capacity() == self.len {
            self.buf.grow(self.len);
        }

        unsafe {
            if index < self.len {
                ptr::copy(
                    self.ptr().offset(index as isize),
                    self.ptr().offset(index as isize + 1),
                    self.len - index,
                );
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
            ptr::copy(
                self.ptr().offset(index as isize + 1),
                self.ptr().offset(index as isize),
                self.len - index,
            );
            result
        }
    }

    pub fn into_iter(self) -> IntoIter<T> {
        unsafe {
            let iter = RawValIter::new(&self);
            let buf = ptr::read(&self.buf);
            mem::forget(self);

            IntoIter { iter, _buf: buf }
        }
    }

    pub fn drain<R>(&mut self, range: R) -> Drain<T>
    where
        R: RangeBounds<usize>,
    {
        let len = self.len();
        let start = match range.start_bound() {
            Included(&n) => n,
            Excluded(&n) => n + 1,
            Unbounded => 0,
        };
        let end = match range.end_bound() {
            Included(&n) => n + 1,
            Excluded(&n) => n,
            Unbounded => len,
        };
        assert!(start <= end);
        assert!(end <= len);

        unsafe {
            // this is a mem::forget safety thing. If Drain is forgotten, we just
            // leak the whole Vec's contents. Also we need to do this *eventually*
            // anyway, so why not do it now?
            self.len = start;
            let range_slice = slice::from_raw_parts_mut(self.as_mut_ptr().add(start), end - start);
            let iter = RawValIter::new(range_slice);
            Drain {
                tail_len: len - end,
                tail_start: end,
                iter,
                phantom: PhantomData,
                vec: NonNull::from(self),
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
        unsafe { ::std::slice::from_raw_parts(self.ptr(), self.len) }
    }
}

impl<T> DerefMut for PinnedVec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        unsafe { ::std::slice::from_raw_parts_mut(self.ptr(), self.len) }
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
            },
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
        let len =
            (self.end as usize - self.start as usize) / if elem_size == 0 { 1 } else { elem_size };
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
    fn next(&mut self) -> Option<T> {
        self.iter.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<T> DoubleEndedIterator for IntoIter<T> {
    fn next_back(&mut self) -> Option<T> {
        self.iter.next_back()
    }
}

impl<T> Drop for IntoIter<T> {
    fn drop(&mut self) {
        for _ in &mut *self {}
    }
}

pub struct Drain<'a, T: 'a> {
    tail_start: usize,
    tail_len: usize,
    phantom: PhantomData<&'a mut PinnedVec<T>>,
    vec: NonNull<PinnedVec<T>>,
    iter: RawValIter<T>,
}

impl<'a, T> Iterator for Drain<'a, T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        self.iter.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<'a, T> DoubleEndedIterator for Drain<'a, T> {
    fn next_back(&mut self) -> Option<T> {
        self.iter.next_back()
    }
}

impl<'a, T> Drop for Drain<'a, T> {
    fn drop(&mut self) {
        // pre-drain the iter
        for _ in &mut self.iter {}

        if self.tail_len > 0 {
            unsafe {
                let source_vec = self.vec.as_mut();
                // memmove back untouched tail, update to new length
                let start = source_vec.len();
                let tail = self.tail_start;
                if tail != start {
                    let src = source_vec.as_ptr().add(tail);
                    let dst = source_vec.as_mut_ptr().add(start);
                    ptr::copy(src, dst, self.tail_len);
                }
                source_vec.len = start + self.tail_len;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pinned_vec() {
        solana_logger::setup();
        let mut mem = PinnedVec::with_capacity(10);
        mem.set_pinnable();
        mem.push(50);
        assert_eq!(mem[0], 50);
        mem.resize(2, 10);
        assert_eq!(mem[0], 50);
        assert_eq!(mem[1], 10);
        assert_eq!(mem.len(), 2);
        assert_eq!(mem.is_empty(), false);
        let mut iter = mem.iter();
        assert_eq!(*iter.next().unwrap(), 50);
        assert_eq!(*iter.next().unwrap(), 10);
        assert_eq!(iter.next(), None);
        let mut sum = 0;
        for x in &mem {
            sum += x;
        }
        assert_eq!(sum, 60);
        let mut iter = mem.iter_mut();
        assert_eq!(*iter.next().unwrap(), 50);
        assert_eq!(*iter.next().unwrap(), 10);
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn create_push_pop() {
        let mut v = PinnedVec::new();
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
        let mut v = PinnedVec::new();
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
        let mut v = PinnedVec::new();
        for i in 0..10 {
            v.push(Box::new(i))
        }
        {
            let mut drain = v.drain(..);
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
    fn test_drain_range() {
        let mut v = PinnedVec::new();
        for i in 0..10 {
            v.push(Box::new(i))
        }
        {
            let mut drain = v.drain(1..3);
            assert_eq!(*drain.next().unwrap(), 1);
            assert_eq!(*drain.next().unwrap(), 2);
            assert_eq!(drain.next(), None);
        }
        assert_eq!(8, v.len());
        v.push(Box::new(1));
        assert_eq!(1, *v.pop().unwrap());
    }

    #[test]
    fn test_zst() {
        let mut v = PinnedVec::new();
        for _i in 0..10 {
            v.push(())
        }

        let mut count = 0;

        for _ in v.into_iter() {
            count += 1
        }

        assert_eq!(10, count);
    }

    #[test]
    fn test_append() {
        solana_logger::setup();
        let mut v = PinnedVec::new();
        for i in 0..3 {
            v.push(i)
        }

        let mut w = PinnedVec::new();
        w.push(100);
        w.append(&mut v);
        assert_eq!(v.len(), 0);
        assert_eq!(w[..], [100, 0, 1, 2]);
    }

    #[test]
    fn test_truncate() {
        let mut w = PinnedVec::new();
        w.push(100);
        w.push(10);
        w.truncate(10);
        assert_eq!(w[..], [100, 10]);
        w.truncate(1);
        assert_eq!(w[..], [100]);
        w.truncate(0);
        let ans = [0; 0];
        assert_eq!(w[..], ans);
    }

    #[test]
    fn test_retain() {
        let mut w = PinnedVec::new();
        w.push(10);
        w.push(100);

        w.retain(|x| *x < 50);
        assert_eq!(w[..], [100]);

        w.push(10);
        w.push(20);
        w.push(20);
        w.retain(|x| *x == 10);
        assert_eq!(w[..], [100, 20, 20]);
    }
}
