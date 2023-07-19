//! A doubly linked list, backed by a vector.

use std::{
    convert::TryFrom,
    fmt::{Debug, Formatter},
    mem,
    num::NonZeroU32,
};

/// Vector index for the elements in the list. They are typically not
/// squential.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Index(Option<NonZeroU32>);

impl Index {
    #[inline]
    fn new() -> Index {
        Index(None)
    }
    #[inline]
    /// Returns `true` for a valid index.
    ///
    /// A valid index can be used in IndexList method calls.
    fn is_some(&self) -> bool {
        self.0.is_some()
    }
    #[inline]
    /// Returns `true` for an invalid index.
    ///
    /// An invalid index will always be ignored and have `None` returned from
    /// any IndexList method call that returns something.
    fn is_none(&self) -> bool {
        self.0.is_none()
    }
    #[inline]
    fn get(&self) -> Option<usize> {
        Some(self.0?.get() as usize - 1)
    }
    #[inline]
    fn set(mut self, index: Option<usize>) -> Self {
        if let Some(n) = index {
            if let Ok(num) = NonZeroU32::try_from(n as u32 + 1) {
                self.0 = Some(num);
            } else {
                self.0 = None;
            }
        } else {
            self.0 = None;
        }
        self
    }
}

impl From<usize> for Index {
    fn from(index: usize) -> Index {
        Index::new().set(Some(index))
    }
}

#[derive(Clone, Default)]
struct IndexNode {
    next: Index,
    prev: Index,
}

impl IndexNode {
    #[inline]
    fn new() -> IndexNode {
        IndexNode {
            next: Index::new(),
            prev: Index::new(),
        }
    }
    #[inline]
    fn new_next(&mut self, next: Index) -> Index {
        mem::replace(&mut self.next, next)
    }
    #[inline]
    fn new_prev(&mut self, prev: Index) -> Index {
        mem::replace(&mut self.prev, prev)
    }
}

#[derive(Clone, Default)]
struct IndexEnds {
    head: Index,
    tail: Index,
}

impl IndexEnds {
    #[inline]
    fn new() -> Self {
        IndexEnds {
            head: Index::new(),
            tail: Index::new(),
        }
    }
    #[inline]
    fn clear(&mut self) {
        self.new_both(Index::new());
    }
    #[inline]
    fn is_empty(&self) -> bool {
        self.head.is_none()
    }
    #[inline]
    fn new_head(&mut self, head: Index) -> Index {
        mem::replace(&mut self.head, head)
    }
    #[inline]
    fn new_tail(&mut self, tail: Index) -> Index {
        mem::replace(&mut self.tail, tail)
    }
    #[inline]
    fn new_both(&mut self, both: Index) {
        self.head = both;
        self.tail = both;
    }
}

/// Doubly-linked list implemented in safe Rust.
pub(crate) struct IndexList<T> {
    elems: Vec<Option<T>>,
    nodes: Vec<IndexNode>,
    used: IndexEnds,
    free: IndexEnds,
    size: usize,
}

impl<T> Default for IndexList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Debug for IndexList<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "IndexList")?;
        Ok(())
    }
}

impl<T> IndexList<T> {
    /// Creates a new empty index list.
    ///
    /// Example:
    /// ```rust
    /// use index_list::IndexList;
    ///
    /// let list = IndexList::<u64>::new();
    /// ```
    fn new() -> Self {
        IndexList {
            elems: Vec::new(),
            nodes: Vec::new(),
            used: IndexEnds::new(),
            free: IndexEnds::new(),
            size: 0,
        }
    }
    /// Clears the list be removing all elements, making it empty.
    ///
    /// Example:
    /// ```rust
    /// # use index_list::IndexList;
    /// # let mut list = IndexList::<u64>::new();
    /// list.clear();
    /// assert!(list.is_empty());
    /// ```
    #[inline]
    pub(crate) fn clear(&mut self) {
        self.elems.clear();
        self.nodes.clear();
        self.used.clear();
        self.free.clear();
        self.size = 0;
    }
    /// Returns `true` if the index is valid.
    #[inline]
    fn is_index_used(&self, index: Index) -> bool {
        self.get(index).is_some()
    }
    /// Returns the index of the first element, or `None` if the list is empty.
    ///
    /// Example:
    /// ```rust
    /// # use index_list::IndexList;
    /// # let list = IndexList::<u64>::new();
    /// let index = list.first_index();
    /// ```
    #[inline]
    fn first_index(&self) -> Index {
        self.used.head
    }
    /// Get a reference to the first element data, or `None`.
    ///
    /// Example:
    /// ```rust
    /// # use index_list::IndexList;
    /// # let list = IndexList::<u64>::new();
    /// let data = list.get_first();
    /// ```
    #[inline]
    pub(crate) fn get_first(&self) -> Option<&T> {
        self.get(self.first_index())
    }
    /// Get an immutable reference to the element data at the index, or `None`.
    ///
    /// Example:
    /// ```rust
    /// # use index_list::IndexList;
    /// # let list = IndexList::<u64>::new();
    /// # let index = list.first_index();
    /// let data = list.get(index);
    /// ```
    #[inline]
    fn get(&self, index: Index) -> Option<&T> {
        let ndx = index.get().unwrap_or(usize::MAX);
        self.elems.get(ndx)?.as_ref()
    }
    /// Insert a new element at the end.
    ///
    /// It is typically not necessary to store the index, as the data will be
    /// there when walking the list.
    ///
    /// Example:
    /// ```rust
    /// # use index_list::IndexList;
    /// # let mut list = IndexList::<u64>::new();
    /// let index = list.insert_last(42);
    /// ```
    pub(crate) fn insert_last(&mut self, elem: T) -> Index {
        let this = self.new_node(Some(elem));
        self.linkin_last(this);
        this
    }
    /// Remove the element at the index and return its data.
    ///
    /// Example:
    /// ```rust
    /// # use index_list::IndexList;
    /// # let mut list = IndexList::from(&mut vec!["A", "B", "C"]);
    /// # let mut index = list.first_index();
    /// # index = list.next_index(index);
    /// let data = list.remove(index);
    /// # assert_eq!(data, Some("B"));
    /// ```
    pub(crate) fn remove(&mut self, index: Index) -> Option<T> {
        let elem_opt = self.remove_elem_at_index(index);
        if elem_opt.is_some() {
            self.linkout_used(index);
            self.linkin_free(index);
        }
        elem_opt
    }

    pub(crate) fn move_to_last(&mut self, index: Index) {
        // unlink where it is
        self.linkout_used(index);
        self.linkin_last(index);
    }

    #[inline]
    fn get_mut_indexnode(&mut self, at: usize) -> &mut IndexNode {
        &mut self.nodes[at]
    }
    #[inline]
    fn set_prev(&mut self, index: Index, new_prev: Index) -> Index {
        if let Some(at) = index.get() {
            self.get_mut_indexnode(at).new_prev(new_prev)
        } else {
            index
        }
    }
    #[inline]
    fn set_next(&mut self, index: Index, new_next: Index) -> Index {
        if let Some(at) = index.get() {
            self.get_mut_indexnode(at).new_next(new_next)
        } else {
            index
        }
    }
    #[inline]
    fn insert_elem_at_index(&mut self, this: Index, elem: Option<T>) {
        if let Some(at) = this.get() {
            self.elems[at] = elem;
            self.size += 1;
        }
    }
    #[inline]
    fn remove_elem_at_index(&mut self, this: Index) -> Option<T> {
        this.get().and_then(|at| {
            self.size -= 1;
            self.elems[at].take()
        })
    }
    fn new_node(&mut self, elem: Option<T>) -> Index {
        let reuse = self.free.head;
        if reuse.is_some() {
            self.insert_elem_at_index(reuse, elem);
            self.linkout_free(reuse);
            return reuse;
        }
        let pos = self.nodes.len();
        self.nodes.push(IndexNode::new());
        self.elems.push(elem);
        self.size += 1;
        Index::from(pos)
    }
    fn linkin_free(&mut self, this: Index) {
        debug_assert!(!self.is_index_used(this));
        let prev = self.free.tail;
        self.set_next(prev, this);
        self.set_prev(this, prev);
        if self.free.is_empty() {
            self.free.new_both(this);
        } else {
            let old_tail = self.free.new_tail(this);
            debug_assert_eq!(old_tail, prev);
        }
    }
    fn linkin_last(&mut self, this: Index) {
        debug_assert!(self.is_index_used(this));
        let prev = self.used.tail;
        self.set_next(prev, this);
        self.set_prev(this, prev);
        if self.used.is_empty() {
            self.used.new_both(this);
        } else {
            let old_tail = self.used.new_tail(this);
            debug_assert_eq!(old_tail, prev);
        }
    }
    // prev >< this >< next => prev >< next
    fn linkout_node(&mut self, this: Index) -> (Index, Index) {
        let next = self.set_next(this, Index::new());
        let prev = self.set_prev(this, Index::new());
        let old_prev = self.set_prev(next, prev);
        if old_prev.is_some() {
            debug_assert_eq!(old_prev, this);
        }
        let old_next = self.set_next(prev, next);
        if old_next.is_some() {
            debug_assert_eq!(old_next, this);
        }
        (prev, next)
    }
    fn linkout_used(&mut self, this: Index) {
        let (prev, next) = self.linkout_node(this);
        if next.is_none() {
            let old_tail = self.used.new_tail(prev);
            debug_assert_eq!(old_tail, this);
        }
        if prev.is_none() {
            let old_head = self.used.new_head(next);
            debug_assert_eq!(old_head, this);
        }
    }
    fn linkout_free(&mut self, this: Index) {
        let (prev, next) = self.linkout_node(this);
        if next.is_none() {
            let old_tail = self.free.new_tail(prev);
            debug_assert_eq!(old_tail, this);
        }
        if prev.is_none() {
            let old_head = self.free.new_head(next);
            debug_assert_eq!(old_head, this);
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, std::mem::size_of};

    #[test]
    fn test_struct_sizes() {
        assert_eq!(size_of::<Index>(), 4);
        assert_eq!(size_of::<IndexNode>(), 8);
        assert_eq!(size_of::<IndexEnds>(), 8);
        assert_eq!(size_of::<IndexList<u32>>(), 72);
    }

    // 4 public methods:
    // insert_last
    // remove
    // clear
    // get_first

    fn assert_is_empty(list: &IndexList<u64>) {
        assert!(list.get(Index::new()).is_none());
        assert!(list.get_first().is_none());
    }

    #[test]
    fn test_clear() {
        let mut list = IndexList::default();
        assert_is_empty(&list);
        let a = 3;
        list.insert_last(a);
        list.clear();
        assert_is_empty(&list);
        list.insert_last(a);
        list.insert_last(a);
        list.clear();
        assert_is_empty(&list);
    }

    #[test]
    fn test_move_to_last() {
        let mut list = IndexList::default();
        let a = 3;
        let b = 4;
        let index_a = list.insert_last(a);
        assert_eq!(list.get(index_a).unwrap(), &a);
        assert_eq!(list.get_first().unwrap(), &a);
        let index_b = list.insert_last(b);
        assert_eq!(list.get(index_a).unwrap(), &a);
        assert_eq!(list.get_first().unwrap(), &a);
        assert_eq!(list.get(index_b).unwrap(), &b);
        list.move_to_last(index_b);
        assert_eq!(list.get(index_a).unwrap(), &a);
        assert_eq!(list.get_first().unwrap(), &a);
        assert_eq!(list.get(index_b).unwrap(), &b);
        list.move_to_last(index_a);
        assert_eq!(list.get(index_a).unwrap(), &a);
        assert_eq!(list.get_first().unwrap(), &b);
        assert_eq!(list.get(index_b).unwrap(), &b);
        list.move_to_last(index_b);
        assert_eq!(list.get(index_a).unwrap(), &a);
        assert_eq!(list.get_first().unwrap(), &a);
        assert_eq!(list.get(index_b).unwrap(), &b);
        list.move_to_last(index_b);
        assert_eq!(list.get(index_a).unwrap(), &a);
        assert_eq!(list.get_first().unwrap(), &a);
        assert_eq!(list.get(index_b).unwrap(), &b);
    }

    #[test]
    fn test_insert_last() {
        let mut list = IndexList::default();
        let a = 3;
        let b = 4;
        let c = 5;
        let index_a = list.insert_last(a);
        assert_eq!(list.get(index_a).unwrap(), &a);
        assert_eq!(list.get_first().unwrap(), &a);
        let index_b = list.insert_last(b);
        assert_eq!(list.get(index_a).unwrap(), &a);
        assert_eq!(list.get_first().unwrap(), &a);
        assert_eq!(list.get(index_b).unwrap(), &b);
        // remove a, then insert a at last
        list.remove(index_a);
        assert!(list.get(index_a).is_none());
        assert_eq!(list.get_first().unwrap(), &b);
        assert_eq!(list.get(index_b).unwrap(), &b);
        let index_a = list.insert_last(a);
        assert_eq!(list.get(index_a).unwrap(), &a);
        assert_eq!(list.get_first().unwrap(), &b);
        assert_eq!(list.get(index_b).unwrap(), &b);
        // remove b, then insert b at last
        list.remove(index_b);
        assert!(list.get(index_b).is_none());
        assert_eq!(list.get_first().unwrap(), &a);
        assert_eq!(list.get(index_a).unwrap(), &a);
        let index_b = list.insert_last(b);
        assert_eq!(list.get(index_a).unwrap(), &a);
        assert_eq!(list.get_first().unwrap(), &a);
        assert_eq!(list.get(index_b).unwrap(), &b);
        // add c
        let index_c = list.insert_last(c);
        assert_eq!(list.get(index_a).unwrap(), &a);
        assert_eq!(list.get_first().unwrap(), &a);
        assert_eq!(list.get(index_b).unwrap(), &b);
        assert_eq!(list.get(index_c).unwrap(), &c);
        // remove c
        list.remove(index_c);
        assert_eq!(list.get(index_a).unwrap(), &a);
        assert_eq!(list.get_first().unwrap(), &a);
        assert_eq!(list.get(index_b).unwrap(), &b);
        assert!(list.get(index_c).is_none());

        // add c
        let index_c = list.insert_last(c);
        assert_eq!(list.get(index_a).unwrap(), &a);
        assert_eq!(list.get_first().unwrap(), &a);
        assert_eq!(list.get(index_b).unwrap(), &b);
        assert_eq!(list.get(index_c).unwrap(), &c);
    }

    #[test]
    fn test_remov5e() {
        // remove in all possible orders, with total len of 1, 2, 3
        // then, re-insert and remove differently
        // this tests removing and re-inserting when:
        // - only element is both head and tail with no prev or next,
        // - removing element with prev and next (in middle of list)
        // - removing element that is head with next,
        // - and tail with prev
        let mut list = IndexList::default();
        let values_raw = vec![3u64, 4, 5];
        for len in 1..=values_raw.len() {
            let move_to_last = (0..=len)
                .map(|i| (i < len).then_some(i))
                .collect::<Vec<_>>();
            for move_to_last in move_to_last {
                let mut values = values_raw.clone();
                values.truncate(len);
                let i_values = values
                    .iter()
                    .enumerate()
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>();

                // insert in order
                assert!(list.get_first().is_none());
                let mut indexes = values
                    .iter()
                    .map(|value| list.insert_last(*value))
                    .collect::<Vec<_>>();

                use itertools::Itertools;
                solana_logger::setup();
                for mut perm in i_values.iter().permutations(i_values.len()) {
                    /* perm for len == 3 is:
                    [0, 1, 2]
                    [0, 2, 1]
                    [1, 0, 2]
                    [1, 2, 0]
                    [2, 0, 1]
                    [2, 1, 0]
                    */
                    // verify list is as expected
                    values
                        .iter()
                        .zip(indexes.iter())
                        .enumerate()
                        .for_each(|(i, (value, index))| {
                            log::error!("i: {i}, first: {:?}, values: {values:?}, move_to_last: {move_to_last:?}, len: {len}", values.first());
                            if i == 0 {
                                assert_eq!(list.get_first(), values.first());
                            }
                            assert_eq!(list.get(*index).unwrap(), value);
                        });
                    if let Some(move_to_last) = move_to_last.as_ref() {
                        let move_to_last = *move_to_last;
                        list.move_to_last(indexes[move_to_last]);
                        let remove = perm.remove(move_to_last);
                        perm.push(remove);
                        let remove = values.remove(move_to_last);
                        values.push(remove);
                        let remove = indexes.remove(move_to_last);
                        indexes.push(remove);
                    }
                    // verify list is as expected
                    values
                        .iter()
                        .zip(indexes.iter())
                        .enumerate()
                        .for_each(|(i, (value, index))| {
                            log::error!("i2: {i}, first: {:?}, values: {values:?}, move_to_last: {move_to_last:?}, len: {len}", values.first());
                            if i == 0 {
                                assert_eq!(list.get_first(), values.first(), "move_to_last: {move_to_last:?}, len: {len}");
                            }
                            assert_eq!(list.get(*index).unwrap(), value);
                        });
                    // remove in permutation order, verify it is gone, verify remaining expected are still there
                    perm.iter().enumerate().for_each(|(perm_i, i)| {
                        list.remove(indexes[**i]);
                        assert!(list.get(indexes[**i]).is_none());
                        // make sure remaining elements are present
                        ((perm_i + 1)..perm.len()).for_each(|i| {
                            assert_eq!(list.get(indexes[*perm[i]]).unwrap(), &values[*perm[i]]);
                        });
                    });
                    assert!(list.get_first().is_none());
                    // insert in order to prepare for next iteration
                    indexes = values
                        .iter()
                        .map(|value| list.insert_last(*value))
                        .collect::<Vec<_>>();
                    // verify list is as expected
                    values.iter().zip(indexes.iter()).enumerate().for_each(
                        |(i, (value, index))| {
                            if i == 0 {
                                assert_eq!(list.get_first(), values.first());
                            }
                            assert_eq!(list.get(*index).unwrap(), value);
                        },
                    );
                }
                indexes.iter().for_each(|index| {
                    list.remove(*index);
                })
            }
        }
    }
}
