//! Provides an iterator interface that create non-conflicting batches of elements to process.
//!
//! The problem that this structure is targetting is as following:
//!     We have a slice of transactions we want to process in batches where transactions
//!     in the same batch do not conflict with each other. This allows us process them in
//!     parallel. The original slice is ordered by priority, and it is often the case
//!     that transactions with high-priority are conflicting with each other. This means
//!     we cannot simply grab chunks of transactions.
//! The solution is to create a MultiIteratorScanner that will use multiple iterators, up
//! to the desired batch size, and create batches of transactions that do not conflict with
//! each other. The MultiIteratorScanner stores state for the current positions of each iterator,
//! as well as which transactions have already been handled. If a transaction is invalid it can
//! also be skipped without being considered for future batches.
//!

/// Output from the element checker used in `MultiIteratorScanner::iterate`.
#[derive(Debug)]
pub enum ProcessingDecision {
    /// Should be processed by the scanner.
    Now,
    /// Should be skipped by the scanner on this pass - process later.
    Later,
    /// Should be skipped and marked as handled so we don't try processing it again.
    Never,
}

/// Iterates over a slice creating valid non-self-conflicting batches of elements to process,
/// elements between batches are not guaranteed to be non-conflicting.
/// Conflicting elements are guaranteed to be processed in the order they appear in the slice,
/// as long as the `should_process` function is appropriately marking resources as used.
/// It is also guaranteed that elements within the batch are in the order they appear in
/// the slice. The batch size is not guaranteed to be `max_iterators` - it can be smaller.
///
/// # Example:
///
/// Assume transactions with same letter conflict with each other. A typical priority ordered
/// buffer might look like:
///
/// ```text
/// [A, A, B, A, C, D, B, C, D]
/// ```
///
/// If we want to have batches of size 4, the MultiIteratorScanner will proceed as follows:
///
/// ```text
/// [A, A, B, A, C, D, B, C, D]
///  ^     ^     ^  ^
///
/// [A, A, B, A, C, D, B, C, D]
///     ^              ^  ^  ^
///
/// [A, A, B, A, C, D, B, C, D]
///           ^
/// ```
///
/// The iterator will iterate with batches:
///
/// ```text
/// [[A, B, C, D], [A, B, C, D], [A]]
/// ```
///
pub struct MultiIteratorScanner<'a, T, U, F>
where
    F: FnMut(&T, &mut U) -> ProcessingDecision,
{
    /// Maximum number of iterators to use.
    max_iterators: usize,
    /// Slice that we're iterating over
    slice: &'a [T],
    /// Payload - used to store shared mutable state between scanner and the processing function.
    payload: U,
    /// Function that checks if an element should be processed. This function is also responsible
    /// for marking resources, such as locks, as used.
    should_process: F,
    /// Store whether an element has already been handled
    already_handled: Vec<bool>,
    /// Current indices inside `slice` for multiple iterators
    current_positions: Vec<usize>,
    /// Container to store items for iteration - Should only be used in `get_current_items()`
    current_items: Vec<&'a T>,
    /// Initialized
    initialized: bool,
}

pub struct PayloadAndAlreadyHandled<U> {
    pub payload: U,
    pub already_handled: Vec<bool>,
}

impl<'a, T, U, F> MultiIteratorScanner<'a, T, U, F>
where
    F: FnMut(&T, &mut U) -> ProcessingDecision,
{
    pub fn new(slice: &'a [T], max_iterators: usize, payload: U, should_process: F) -> Self {
        assert!(max_iterators > 0);
        Self {
            max_iterators,
            slice,
            payload,
            should_process,
            already_handled: vec![false; slice.len()],
            current_positions: Vec::with_capacity(max_iterators),
            current_items: Vec::with_capacity(max_iterators),
            initialized: false,
        }
    }

    /// Returns a slice of the item references at the current positions of the iterators
    /// and a mutable reference to the payload.
    ///
    /// Returns None if the scanner is done iterating.
    pub fn iterate(&mut self) -> Option<(&[&'a T], &mut U)> {
        if !self.initialized {
            self.initialized = true;
            self.initialize_current_positions();
        } else {
            self.advance_current_positions();
        }
        self.get_current_items()
    }

    /// Consume the iterator. Return the payload, and a vector of booleans
    /// indicating which items have been handled.
    pub fn finalize(self) -> PayloadAndAlreadyHandled<U> {
        PayloadAndAlreadyHandled {
            payload: self.payload,
            already_handled: self.already_handled,
        }
    }

    /// Initialize the `current_positions` vector for the first batch.
    fn initialize_current_positions(&mut self) {
        let mut last_index = 0;
        for _iterator_index in 0..self.max_iterators {
            match self.march_iterator(last_index) {
                Some(index) => {
                    self.current_positions.push(index);
                    last_index = index.saturating_add(1);
                }
                None => break,
            }
        }
    }

    /// March iterators forward to find the next batch of items.
    fn advance_current_positions(&mut self) {
        if let Some(mut prev_index) = self.current_positions.first().copied() {
            for iterator_index in 0..self.current_positions.len() {
                // If the previous iterator has passed this iterator, we should start
                // at it's position + 1 to avoid duplicate re-traversal.
                let start_index = (self.current_positions[iterator_index].saturating_add(1))
                    .max(prev_index.saturating_add(1));
                match self.march_iterator(start_index) {
                    Some(index) => {
                        self.current_positions[iterator_index] = index;
                        prev_index = index;
                    }
                    None => {
                        // Drop current positions that go past the end of the slice
                        self.current_positions.truncate(iterator_index);
                        break;
                    }
                }
            }
        }
    }

    /// Get the current items from the slice using `self.current_positions`.
    /// Returns `None` if there are no more items.
    fn get_current_items(&mut self) -> Option<(&[&'a T], &mut U)> {
        self.current_items.clear();
        for index in &self.current_positions {
            self.current_items.push(&self.slice[*index]);
        }
        (!self.current_items.is_empty()).then_some((&self.current_items, &mut self.payload))
    }

    /// Moves the iterator to its' next position. If we've reached the end of the slice, we return None
    fn march_iterator(&mut self, starting_index: usize) -> Option<usize> {
        let mut found = None;
        for index in starting_index..self.slice.len() {
            if !self.already_handled[index] {
                match (self.should_process)(&self.slice[index], &mut self.payload) {
                    ProcessingDecision::Now => {
                        self.already_handled[index] = true;
                        found = Some(index);
                        break;
                    }
                    ProcessingDecision::Later => {
                        // Do nothing - iterator will try this element in a future batch
                    }
                    ProcessingDecision::Never => {
                        self.already_handled[index] = true;
                    }
                }
            }
        }

        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestScannerPayload {
        locks: Vec<bool>,
    }

    fn test_scanner_locking_should_process(
        item: &i32,
        payload: &mut TestScannerPayload,
    ) -> ProcessingDecision {
        if payload.locks[*item as usize] {
            ProcessingDecision::Later
        } else {
            payload.locks[*item as usize] = true;
            ProcessingDecision::Now
        }
    }

    #[test]
    fn test_multi_iterator_scanner_empty() {
        let slice: Vec<i32> = vec![];
        let mut scanner = MultiIteratorScanner::new(&slice, 2, (), |_, _| ProcessingDecision::Now);
        assert!(scanner.iterate().is_none());
    }

    #[test]
    fn test_multi_iterator_scanner_iterate() {
        let slice = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        let should_process = |_item: &i32, _payload: &mut ()| ProcessingDecision::Now;

        let mut scanner = MultiIteratorScanner::new(&slice, 2, (), should_process);
        let mut actual_batches = vec![];
        while let Some((batch, _payload)) = scanner.iterate() {
            actual_batches.push(batch.to_vec());
        }

        // Batch 1: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        //           ^  ^
        // Batch 2: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        //                 ^  ^
        // Batch 3: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        //                       ^  ^
        // Batch 4: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        //                             ^  ^
        // Batch 5: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        //                                   ^   ^
        // Batch 6: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]
        //                                           ^
        let expected_batches = vec![
            vec![&1, &2],
            vec![&3, &4],
            vec![&5, &6],
            vec![&7, &8],
            vec![&9, &10],
            vec![&11],
        ];
        assert_eq!(actual_batches, expected_batches);
    }

    #[test]
    fn test_multi_iterator_scanner_iterate_with_gaps() {
        let slice = [0, 0, 0, 1, 2, 3, 1];

        let payload = TestScannerPayload {
            locks: vec![false; 4],
        };

        let mut scanner =
            MultiIteratorScanner::new(&slice, 2, payload, test_scanner_locking_should_process);
        let mut actual_batches = vec![];
        while let Some((batch, payload)) = scanner.iterate() {
            // free the resources
            for item in batch {
                payload.locks[**item as usize] = false;
            }

            actual_batches.push(batch.to_vec());
        }

        // Batch 1: [0, 0, 0, 1, 2, 3, 4]
        //           ^        ^
        // Batch 2: [0, 0, 0, 1, 2, 3, 4]
        //              ^        ^
        // Batch 3: [0, 0, 0, 1, 2, 3, 4]
        //                 ^        ^
        // Batch 4: [0, 0, 0, 1, 2, 3, 4]
        //                  -----------^ (--- indicates where the 0th iterator marched from)
        let expected_batches = vec![vec![&0, &1], vec![&0, &2], vec![&0, &3], vec![&1]];
        assert_eq!(actual_batches, expected_batches);

        let PayloadAndAlreadyHandled {
            payload: TestScannerPayload { locks },
            already_handled,
        } = scanner.finalize();
        assert_eq!(locks, vec![false; 4]);
        assert!(already_handled.into_iter().all(|x| x));
    }

    #[test]
    fn test_multi_iterator_scanner_iterate_conflicts_not_at_front() {
        let slice = [1, 2, 3, 0, 0, 0, 3, 2, 1];

        let payload = TestScannerPayload {
            locks: vec![false; 4],
        };

        let mut scanner =
            MultiIteratorScanner::new(&slice, 2, payload, test_scanner_locking_should_process);
        let mut actual_batches = vec![];
        while let Some((batch, payload)) = scanner.iterate() {
            // free the resources
            for item in batch {
                payload.locks[**item as usize] = false;
            }

            actual_batches.push(batch.to_vec());
        }

        // Batch 1: [1, 2, 3, 0, 0, 0, 3, 2, 1]
        //           ^  ^
        // Batch 2: [1, 2, 3, 0, 0, 0, 3, 2, 1]
        //                 ^  ^
        // Batch 3: [1, 2, 3, 0, 0, 0, 3, 2, 1]
        //                       ^     ^
        // Batch 4: [1, 2, 3, 0, 0, 0, 3, 2, 1]
        //                          ^     ^
        // Batch 5: [1, 2, 3, 0, 0, 0, 3, 2, 1]
        //                                   ^
        let expected_batches = vec![
            vec![&1, &2],
            vec![&3, &0],
            vec![&0, &3],
            vec![&0, &2],
            vec![&1],
        ];
        assert_eq!(actual_batches, expected_batches);

        let PayloadAndAlreadyHandled {
            payload: TestScannerPayload { locks },
            already_handled,
        } = scanner.finalize();
        assert_eq!(locks, vec![false; 4]);
        assert!(already_handled.into_iter().all(|x| x));
    }

    #[test]
    fn test_multi_iterator_scanner_iterate_with_never_process() {
        let slice = [0, 4, 1, 2];
        let should_process = |item: &i32, _payload: &mut ()| match item {
            4 => ProcessingDecision::Never,
            _ => ProcessingDecision::Now,
        };

        let mut scanner = MultiIteratorScanner::new(&slice, 2, (), should_process);
        let mut actual_batches = vec![];
        while let Some((batch, _payload)) = scanner.iterate() {
            actual_batches.push(batch.to_vec());
        }

        // Batch 1: [0, 4, 1, 2]
        //           ^     ^
        // Batch 2: [0, 4, 1, 2]
        //                    ^
        let expected_batches = vec![vec![&0, &1], vec![&2]];
        assert_eq!(actual_batches, expected_batches);
    }

    #[test]
    fn test_multi_iterator_scanner_iterate_not_handled() {
        let slice = [0, 1, 2];

        // 0 and 2 will always be marked as later, and never actually handled
        let should_process = |item: &i32, _payload: &mut ()| match item {
            1 => ProcessingDecision::Now,
            _ => ProcessingDecision::Later,
        };

        let mut scanner = MultiIteratorScanner::new(&slice, 2, (), should_process);
        let mut actual_batches = vec![];
        while let Some((batch, _payload)) = scanner.iterate() {
            actual_batches.push(batch.to_vec());
        }

        // Batch 1: [1]
        let expected_batches = vec![vec![&1]];
        assert_eq!(actual_batches, expected_batches);

        let PayloadAndAlreadyHandled {
            already_handled, ..
        } = scanner.finalize();
        assert_eq!(already_handled, vec![false, true, false]);
    }
}
