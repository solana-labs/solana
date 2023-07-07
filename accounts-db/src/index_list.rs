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
    pub(crate) fn new() -> Index {
        Index(None)
    }
    #[inline]
    /// Returns `true` for a valid index.
    ///
    /// A valid index can be used in IndexList method calls.
    pub(crate) fn is_some(&self) -> bool {
        self.0.is_some()
    }
    #[inline]
    /// Returns `true` for an invalid index.
    ///
    /// An invalid index will always be ignored and have `None` returned from
    /// any IndexList method call that returns something.
    pub(crate) fn is_none(&self) -> bool {
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
    pub(crate) fn new() -> Self {
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
    pub(crate) fn is_index_used(&self, index: Index) -> bool {
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
    pub(crate) fn first_index(&self) -> Index {
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
    pub(crate) fn get(&self, index: Index) -> Option<&T> {
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

    /// Move the element at the index to the end.
    /// The index remains the same.
    pub(crate) fn move_to_last(&mut self, index: Index) {
        if self.is_index_used(index) {
            // unlink where it is
            self.linkout_used(index);
            // insert it as last
            self.linkin_last(index);
        }
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
}
