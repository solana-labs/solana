/// Output from the element checker used in `MultiIteratorScanner::iterate`.
pub enum MultiIteratorScannerElementCheck {
    /// Should be processed by the scanner.
    Process,
    /// Should be skipped by the scanner on this pass - process later.
    ProcessLater,
    /// Should be skipped and marked as processed so we don't process it again.
    NeverProcess,
}

/// Single-pass scan over a slice with multiple iterators.
pub struct MultiIteratorScanner<'a, T> {
    /// Slice that we're iterating over
    slice: &'a [T],
    /// Store whether an element has already been processed
    already_processed: Vec<bool>,
    /// The indices used for iteration
    indices: Vec<usize>,
    /// Container to store items for iteration - Should only be used in `get_current_items()`
    current_items: Vec<&'a T>,
    /// Initialized
    initialized: bool,
}

impl<'a, T> MultiIteratorScanner<'a, T> {
    pub fn new(slice: &'a [T], num_iterators: usize) -> Self {
        assert!(num_iterators > 0);
        Self {
            slice,
            already_processed: vec![false; slice.len()],
            indices: Vec::with_capacity(num_iterators),
            current_items: Vec::with_capacity(num_iterators),
            initialized: false,
        }
    }

    /// Returns the next set of indices to use for iteration if any.
    /// `should_process` - a callback that returns true if the index should be processed.
    ///                    It should update state to reflect the index being processed,
    ///                    so that multiple iterators do not grab the same index.
    pub fn iterate<F>(&mut self, should_process: &mut F) -> Option<&[&'a T]>
    where
        F: FnMut(&T) -> MultiIteratorScannerElementCheck,
    {
        if !self.initialized {
            return self.initialize_iterators(should_process);
        }

        assert!(!self.indices.is_empty()); // shouldn't iterator after we're done - user error

        let prev_index = *self.indices.first().unwrap();
        let mut first_index_reached_end = None;
        for (iterator_index, iterator) in self.indices.iter_mut().enumerate() {
            // If the previous iterator has passed this iterator, we should start at it's position + 1.
            let start_index = (iterator.saturating_add(1)).max(prev_index.saturating_add(1));
            match Self::march_iterator(
                self.slice,
                &mut self.already_processed,
                start_index,
                should_process,
            ) {
                Some(index) => {
                    *iterator = index;
                }
                None => {
                    first_index_reached_end = Some(iterator_index);
                    break;
                }
            }
        }

        // Drop indices that would pass the end
        if let Some(first_index_reached_end) = first_index_reached_end {
            self.indices.truncate(first_index_reached_end);
        }

        self.get_current_items()
    }

    fn initialize_iterators<F>(&mut self, should_process: &mut F) -> Option<&[&'a T]>
    where
        F: FnMut(&T) -> MultiIteratorScannerElementCheck,
    {
        let mut last_index = 0;
        let num_iterators = self.indices.capacity();
        for _iterator_index in 0..num_iterators {
            match Self::march_iterator(
                self.slice,
                &mut self.already_processed,
                last_index,
                should_process,
            ) {
                Some(index) => {
                    self.indices.push(index);
                    last_index = index.saturating_add(1);
                }
                None => break,
            }
        }
        self.initialized = true;

        self.get_current_items()
    }

    /// Get the current items for `self.indices` in `self.indices`
    fn get_current_items(&mut self) -> Option<&[&'a T]> {
        self.current_items.clear();
        for index in &self.indices {
            self.current_items.push(&self.slice[*index]);
        }
        (!self.indices.is_empty()).then_some(&self.current_items[..])
    }

    /// Moves the iterator to its' next position. If we've reached the end of the slice, we return None
    fn march_iterator<F>(
        slice: &[T],
        already_processed: &mut [bool],
        mut index: usize,
        should_process: &mut F,
    ) -> Option<usize>
    where
        F: FnMut(&T) -> MultiIteratorScannerElementCheck,
    {
        // Check length and `already_processed` before calling into `should_process`
        let length = slice.len();
        while index < length {
            if !already_processed[index] {
                match should_process(&slice[index]) {
                    MultiIteratorScannerElementCheck::Process => {
                        already_processed[index] = true;
                        return Some(index);
                    }
                    MultiIteratorScannerElementCheck::ProcessLater => {}
                    MultiIteratorScannerElementCheck::NeverProcess => {
                        already_processed[index] = true;
                    }
                }
            }
            index += 1;
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use {
        super::MultiIteratorScanner,
        crate::multi_iterator_scanner::MultiIteratorScannerElementCheck,
        std::{cell::RefCell, rc::Rc},
    };

    #[test]
    fn test_multi_iterator_scanner_iterate() {
        let slice = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        let mut should_process = move |_item: &i32| MultiIteratorScannerElementCheck::Process;

        let mut scanner = MultiIteratorScanner::new(&slice, 2);
        // [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        //  ^  ^
        assert_eq!(
            scanner.iterate(&mut should_process),
            Some(&vec![&1, &2][..])
        );
        // [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        //        ^  ^
        assert_eq!(
            scanner.iterate(&mut should_process),
            Some(&vec![&3, &4][..])
        );
        // [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        //              ^  ^
        assert_eq!(
            scanner.iterate(&mut should_process),
            Some(&vec![&5, &6][..])
        );
        // [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        //                    ^  ^
        assert_eq!(
            scanner.iterate(&mut should_process),
            Some(&vec![&7, &8][..])
        );
        // [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        //                           ^  ^
        assert_eq!(
            scanner.iterate(&mut should_process),
            Some(&vec![&9, &10][..])
        );
        // [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        //                                  ^
        assert_eq!(scanner.iterate(&mut should_process), Some(&vec![&11][..]));
        // [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        //                                     ^ (done)
        assert_eq!(scanner.iterate(&mut should_process), None);
    }

    #[test]
    fn test_multi_iterator_scanner_iterate_with_gaps() {
        let slice = [0, 0, 0, 1, 2, 3, 1];
        let resource_locked = Rc::new(RefCell::new(vec![false; 4]));
        let resource_locked_clone = resource_locked.clone();
        let mut should_process = move |item: &i32| {
            // Resource locked => skip
            if resource_locked.borrow()[*item as usize] {
                return MultiIteratorScannerElementCheck::ProcessLater;
            }

            // Lock the resource - this needs to be in the `should_skip`
            resource_locked.borrow_mut()[*item as usize] = true;

            MultiIteratorScannerElementCheck::Process
        };

        // Batch 1: [0, 0, 0, 1, 2, 3, 4]
        //           ^        ^
        // Batch 2: [0, 0, 0, 1, 2, 3, 4]
        //              ^        ^
        // Batch 3: [0, 0, 0, 1, 2, 3, 4]
        //                 ^        ^
        // Batch 4: [0, 0, 0, 1, 2, 3, 4]
        //                  -----------^ (--- indicates where the 0th iterator marched from)
        let expected_batches = vec![vec![&0, &1], vec![&0, &2], vec![&0, &3], vec![&1]];
        let mut index = 0;
        let mut scanner = MultiIteratorScanner::new(&slice, 2);
        while let Some(batch) = scanner.iterate(&mut should_process) {
            assert_eq!(batch, &expected_batches[index][..]);
            // free the resources
            for item in batch {
                resource_locked_clone.borrow_mut()[**item as usize] = false;
            }

            index += 1;
        }
    }

    #[test]
    fn test_multi_iterator_scanner_iterate_with_never_process() {
        let slice = [0, 4, 1, 2];
        let mut should_process = move |item: &i32| match item {
            4 => MultiIteratorScannerElementCheck::NeverProcess,
            _ => MultiIteratorScannerElementCheck::Process,
        };

        // Batch 1: [0, 4, 1, 2]
        //           ^     ^
        // Batch 1: [0, 4, 1, 2]
        //                    ^
        let expected_batches = vec![vec![&0, &1], vec![&2]];
        let mut index = 0;
        let mut scanner = MultiIteratorScanner::new(&slice, 2);
        while let Some(batch) = scanner.iterate(&mut should_process) {
            assert_eq!(batch, &expected_batches[index][..]);
            index += 1;
        }
    }
}
