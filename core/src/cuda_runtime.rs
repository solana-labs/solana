use crate::recycler::Reset;
//use std::vec::IntoIter;
use std::ops::{Deref, DerefMut};
//use std::iter::IntoIterator;
//use std::iter::Iterator;

#[cfg(feature = "cuda")]
use std::mem::size_of;

#[cfg(feature = "cuda")]
use core::ffi::c_void;

#[cfg(feature = "cuda")]
type CudaError = i32;

#[cfg(feature = "cuda")]
#[link(name = "cudart")]
extern "C" {
    fn cudaHostRegister(ptr: *mut c_void, size: usize, flags: u32) -> CudaError;
    fn cudaHostUnregister(ptr: *mut c_void) -> CudaError;
}

#[cfg(feature = "cuda")]
const CUDA_SUCCESS: CudaError = 0;

pub fn pin<T>(_mem: &mut Vec<T>) {
    #[cfg(feature = "cuda")]
    unsafe {
        let err = cudaHostRegister(
            _mem.as_mut_ptr() as *mut c_void,
            _mem.capacity() * size_of::<T>(),
            0,
        );
        if err != CUDA_SUCCESS {
            error!(
                "cudaHostRegister error: {} ptr: {:?} bytes: {}",
                err,
                _mem.as_ptr(),
                _mem.len() * size_of::<T>()
            );
        }
    }
}

pub fn unpin<T>(_mem: *mut T) {
    #[cfg(feature = "cuda")]
    unsafe {
        let err = cudaHostUnregister(_mem as *mut c_void);
        if err != CUDA_SUCCESS {
            error!("cudaHostUnregister returned: {} ptr: {:?}", err, _mem);
        }
    }
}

#[derive(Debug)]
pub struct PinnedVec<T> {
    x: Vec<T>,
    pinned: bool,
    pinnable: bool,
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
        Self {
            x: Vec::new(),
            pinned: false,
            pinnable: false,
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
}

pub struct PinnedIter<'a, T>(std::slice::Iter<'a, T>);

pub struct PinnedIterMut<'a, T>(std::slice::IterMut<'a, T>);

impl<'a, T> Iterator for PinnedIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

impl<'a, T> Iterator for PinnedIterMut<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

impl<'a, T> IntoIterator for &'a mut PinnedVec<T> {
    type Item = &'a T;
    type IntoIter = PinnedIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        PinnedIter(self.iter())
    }
}

impl<'a, T> IntoIterator for &'a PinnedVec<T> {
    type Item = &'a T;
    type IntoIter = PinnedIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        PinnedIter(self.iter())
    }
}

impl<T: Clone> PinnedVec<T> {
    pub fn set_pinnable(&mut self) {
        self.pinnable = true;
    }

    pub fn from_vec(source: Vec<T>) -> Self {
        Self {
            x: source,
            pinned: false,
            pinnable: false,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let x = Vec::with_capacity(capacity);
        Self {
            x,
            pinned: false,
            pinnable: false,
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
        self.x.push(x);
        self.check_ptr(old_ptr, old_capacity);
    }

    pub fn resize(&mut self, size: usize, elem: T) {
        let old_ptr = self.x.as_mut_ptr();
        let old_capacity = self.x.capacity();
        self.x.resize(size, elem);
        self.check_ptr(old_ptr, old_capacity);
    }

    fn check_ptr(&mut self, _old_ptr: *mut T, _old_capacity: usize) {
        #[cfg(feature = "cuda")]
        {
            if self.pinnable && (self.x.as_ptr() != _old_ptr || self.x.capacity() != _old_capacity)
            {
                if self.pinned {
                    unpin(_old_ptr);
                }

                pin(&mut self.x);
                self.pinned = true;
            }
        }
    }
}

impl<T: Clone> Clone for PinnedVec<T> {
    fn clone(&self) -> Self {
        let mut x = self.x.clone();
        let pinned = if self.pinned {
            pin(&mut x);
            true
        } else {
            false
        };
        Self {
            x,
            pinned,
            pinnable: self.pinnable,
        }
    }
}

impl<T> Drop for PinnedVec<T> {
    fn drop(&mut self) {
        if self.pinned {
            unpin(self.x.as_mut_ptr());
        }
    }
}
