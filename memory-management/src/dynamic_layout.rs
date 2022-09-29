//! Dynamic memory layout containers
use crate::is_memory_aligned;

/// An array of equally spaced values
#[derive(Clone)]
#[repr(C)]
pub struct DynamicLayoutArray<'a, T> {
    _element_type: std::marker::PhantomData<&'a T>,
    element_count: u32,
    values_stride: u32,
    values_offset: u32,
}

impl<'a, T> DynamicLayoutArray<'a, T> {
    pub fn initialize_as_strided(
        &mut self,
        start_offset: usize,
        element_count: usize,
        values_stride: usize,
    ) {
        self.element_count = element_count as u32;
        self.values_stride = values_stride as u32;
        self.values_offset = start_offset as u32;
        debug_assert!(element_count < std::u32::MAX as usize);
        debug_assert!(values_stride <= std::u32::MAX as usize);
        debug_assert!(start_offset <= std::u32::MAX as usize);
        debug_assert!(is_memory_aligned(
            self.as_ptr() as usize,
            std::mem::align_of::<T>(),
        ));
        debug_assert!(is_memory_aligned(values_stride, std::mem::align_of::<T>()));
    }

    pub fn initialize_as_consecutive(&mut self, start_offset: usize, element_count: usize) {
        self.initialize_as_strided(start_offset, element_count, std::mem::size_of::<T>());
    }

    pub fn offset_at_index(&self, element_index: usize) -> usize {
        (self.values_offset as usize)
            .saturating_add((self.values_stride as usize).saturating_mul(element_index))
    }

    pub fn start_offset(&self) -> usize {
        self.values_offset as usize
    }

    pub fn end_offset(&self) -> usize {
        self.offset_at_index(self.element_count as usize)
    }

    pub fn is_empty(&self) -> bool {
        self.element_count == 0
    }

    pub fn len(&self) -> usize {
        self.element_count as usize
    }

    pub fn as_ptr(&self) -> *const T {
        unsafe { (self as *const _ as *const u8).add(self.values_offset as usize) as *const T }
    }

    pub fn as_mut_ptr(&mut self) -> *mut T {
        unsafe { (self as *mut _ as *mut u8).add(self.values_offset as usize) as *mut T }
    }

    pub fn get(&self, element_index: usize) -> Option<&T> {
        if element_index >= self.element_count as usize {
            return None;
        }
        Some(unsafe { &*self.as_ptr().add(element_index) })
    }

    pub fn get_mut(&mut self, element_index: usize) -> Option<&mut T> {
        if element_index >= self.element_count as usize {
            return None;
        }
        Some(unsafe { &mut *self.as_mut_ptr().add(element_index) })
    }

    pub fn as_slice(&self) -> &[T] {
        assert_eq!(self.values_stride as usize, std::mem::size_of::<T>());
        unsafe { std::slice::from_raw_parts(self.as_ptr(), self.element_count as usize) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        assert_eq!(self.values_stride as usize, std::mem::size_of::<T>());
        unsafe { std::slice::from_raw_parts_mut(self.as_mut_ptr(), self.element_count as usize) }
    }

    pub fn iter<'b>(&'b self) -> DynamicLayoutArrayIterator<'a, 'b, T> {
        DynamicLayoutArrayIterator {
            _element_type: std::marker::PhantomData::default(),
            array: self,
            element_index: 0,
        }
    }

    pub fn iter_mut<'b>(&'b mut self) -> DynamicLayoutArrayIteratorMut<'a, 'b, T> {
        DynamicLayoutArrayIteratorMut {
            _element_type: std::marker::PhantomData::default(),
            array: self,
            element_index: 0,
        }
    }
}

impl<'a, T: std::fmt::Debug> std::fmt::Debug for DynamicLayoutArray<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

#[derive(Clone)]
pub struct DynamicLayoutArrayIterator<'a, 'b, T> {
    _element_type: std::marker::PhantomData<T>,
    array: &'b DynamicLayoutArray<'a, T>,
    element_index: usize,
}

impl<'a, 'b, T> Iterator for DynamicLayoutArrayIterator<'a, 'b, T> {
    type Item = &'b T;

    fn next(&mut self) -> Option<Self::Item> {
        let result = self.array.get(self.element_index);
        self.element_index = self.element_index.saturating_add(1);
        result
    }
}

pub struct DynamicLayoutArrayIteratorMut<'a, 'b, T> {
    _element_type: std::marker::PhantomData<T>,
    array: &'b mut DynamicLayoutArray<'a, T>,
    element_index: usize,
}

impl<'a, 'b, T> Iterator for DynamicLayoutArrayIteratorMut<'a, 'b, T> {
    type Item = &'b mut T;

    fn next(&mut self) -> Option<Self::Item> {
        let result = self.array.get_mut(self.element_index);
        self.element_index = self.element_index.saturating_add(1);
        unsafe { std::mem::transmute(result) }
    }
}

/* Do not implement these as they can panic.

impl<T> std::ops::Index<usize> for DynamicLayoutArray<T> {
    type Output = T;
    fn index<'a>(&'a self, element_index: usize) -> &'a T {
        self.get(element_index).unwrap()
    }
}

impl<T> std::ops::IndexMut<usize> for DynamicLayoutArray<T> {
    fn index_mut<'a>(&'a mut self, element_index: usize) -> &'a mut T {
        self.get_mut(element_index).unwrap()
    }
}

*/

#[cfg(test)]
mod tests {
    use {super::*, crate::aligned_memory::AlignedMemory};

    #[test]
    fn test_dynamic_layout_array() {
        type ArrayType<'a> = DynamicLayoutArray<'a, u16>;
        const COUNT: usize = 64;
        let mut buffer = AlignedMemory::<{ std::mem::align_of::<ArrayType>() }>::zero_filled(256);
        let array_header = unsafe { &mut *(buffer.as_slice_mut().as_mut_ptr() as *mut ArrayType) };
        array_header.initialize_as_consecutive(std::mem::size_of::<ArrayType>(), COUNT);
        assert_eq!(
            array_header.start_offset(),
            std::mem::size_of::<ArrayType>()
        );
        assert_eq!(
            array_header.end_offset(),
            std::mem::size_of::<ArrayType>() + std::mem::size_of::<u16>() * COUNT
        );
        assert!(!array_header.is_empty());
        assert_eq!(array_header.len(), COUNT);
        *array_header.get_mut(21).unwrap() = 42;
        assert_eq!(*array_header.get(21).unwrap(), 42);
        array_header.as_mut_slice()[12] = 24;
        assert_eq!(array_header.as_slice()[12], 24);
        *array_header.iter_mut().next().unwrap() = 8;
        assert_eq!(*array_header.iter().next().unwrap(), 8);
    }
}
