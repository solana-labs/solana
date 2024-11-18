use ahash::AHasher;
use std::borrow::Borrow;
use std::hash::{Hash, Hasher};
use std::mem;
use std::ops::Deref;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Fast concurrent hashmap with fixed capacity.
///
/// This hashmap is designed for use cases where the capacity is known in advance and small. It's a
/// stripped down version of dashmap that guarantees that there are no collisions and so that
/// there's never contention when working with distinct keys.
///
/// The map uses linear probing to resolve collisions, so it works best with a low load factor (i.e.
/// a small number of elements relative to the capacity).
#[derive(Debug)]
pub struct FixedConcurrentMap<K, V> {
    table: Vec<RwLock<Option<(K, V)>>>,
}

impl<K: Eq + Hash + Clone, V> FixedConcurrentMap<K, V> {
    /// Creates a new hashmap with the given capacity.
    pub fn new(capacity: usize) -> Self {
        assert!(
            capacity.is_power_of_two(),
            "capacity must be a power of two"
        );
        Self {
            table: (0..capacity).map(|_| RwLock::new(None)).collect(),
        }
    }

    fn hash<Q: Hash + ?Sized>(&self, key: &Q) -> usize {
        let mask = self.table.len() - 1;
        let mut hasher = AHasher::default();
        key.hash(&mut hasher);
        (hasher.finish() as usize) & mask
    }

    /// Inserts a key-value pair into the hashmap.
    ///
    /// If the key already exists, the value is updated and the old value is returned.
    #[allow(dead_code)]
    pub fn insert(&self, key: K, value: V) -> Result<Option<V>, &'static str> {
        let mask = self.table.len() - 1;
        let index = self.hash(&key);
        for i in 0..self.table.len() {
            let probe_index = (index + i) & mask;
            let slot = &self.table[probe_index];

            let mut entry = slot.write().unwrap();
            match &mut *entry {
                Some((existing_key, existing_value)) if existing_key == &key => {
                    let old_value = mem::replace(existing_value, value);
                    return Ok(Some(old_value));
                }
                Some(_) => continue,
                None => {
                    *entry = Some((key, value));
                    return Ok(None);
                }
            }
        }
        Err("Table is full")
    }

    /// Gets the value associated with the given key.
    ///
    /// Acquires a read lock on the entry, which is released when the returned guard is dropped.
    pub fn get<Q>(&self, key: &Q) -> Option<ReadGuard<K, V>>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let mask = self.table.len() - 1;
        let index = self.hash(key);
        for i in 0..self.table.len() {
            let probe_index = (index + i) & mask;
            let slot = &self.table[probe_index];
            let entry = slot.read().unwrap();
            match &*entry {
                Some((existing_key, _)) if existing_key.borrow() == key => {
                    return Some(ReadGuard { inner: entry });
                }
                Some(_) => continue,
                None => return None,
            }
        }
        None
    }

    /// Gets the value associated with the given key or inserts it using the provided closure.
    ///
    /// The implementation assumes that the key almost always exists in the map,
    /// so it's optimized for that case.
    ///
    /// Always returns a read guard, even if the key was inserted in which case, the write lock is
    /// released and the read lock is re-acquired. This is optimized for the case where the value is
    /// itself a concurrent data structure and so holding the write lock is unnecessary after the
    /// value is inserted and would only cause contention.
    ///
    /// In the case where the value is inserted, there's a small window where the entry could be
    /// removed while the write lock is released and the read lock is re-acquired. In this case, the
    /// function returns an error.
    pub fn get_or_insert_with<F>(&self, key: K, default: F) -> Result<ReadGuard<K, V>, &'static str>
    where
        F: FnOnce() -> V,
    {
        let mask = self.table.len() - 1;
        let index = self.hash(&key);
        for i in 0..self.table.len() {
            let probe_index = (index + i) & mask;
            let slot = &self.table[probe_index];

            {
                // Optimistically try to read the entry without acquiring the write lock.
                let entry = slot.read().unwrap();
                if let Some((existing_key, _)) = &*entry {
                    if existing_key == &key {
                        return Ok(ReadGuard { inner: entry });
                    } else {
                        continue;
                    }
                }
            }

            {
                let mut entry = slot.write().unwrap();
                match &*entry {
                    Some((existing_key, _)) if existing_key == &key => {
                        // The entry was inserted in the window between releasing the read lock and
                        // acquiring the write lock.
                    }
                    Some(_) => continue,
                    None => {
                        // Insert the new entry.
                        *entry = Some((key, default()));
                    }
                }
            }

            // Drop the write lock and re-acquire a read lock.
            let entry = slot.read().unwrap();
            if let Some((_existing_key, _)) = &*entry {
                return Ok(ReadGuard { inner: entry });
            } else {
                return Err("entry removed while in get_or_insert_with");
            }
        }
        Err("map is full")
    }

    /// Removes the value associated with the given key.
    ///
    /// Returns the value if it existed.
    pub fn remove<Q>(&self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let mask = self.table.len() - 1;
        let index = self.hash(key);
        for i in 0..self.table.len() {
            let probe_index = (index + i) & mask;
            let slot = &self.table[probe_index];
            let mut entry = slot.write().unwrap();
            match &*entry {
                Some((existing_key, _)) if existing_key.borrow() == key => {
                    let (_, value) = entry.take().unwrap();
                    return Some(value);
                }
                Some(_) => continue,
                None => return None,
            }
        }
        None
    }

    /// Retains only the elements specified by the predicate.
    pub fn retain<F>(&self, mut f: F)
    where
        F: FnMut(&K, &mut V) -> bool,
    {
        for slot in &self.table {
            let mut entry = slot.write().unwrap();
            if let Some((ref key, ref mut value)) = *entry {
                if !f(key, value) {
                    *entry = None;
                }
            }
        }
    }

    /// Clears the map, removing all key-value pairs.
    #[cfg(feature = "dev-context-only-utils")]
    pub fn clear(&self) {
        for slot in &self.table {
            let mut entry = slot.write().unwrap();
            *entry = None;
        }
    }

    /// Returns an iterator over the key-value pairs in the map.
    pub fn iter(&self) -> impl Iterator<Item = IterGuard<K, V>> {
        self.table.iter().filter_map(|slot| {
            let entry = slot.read().unwrap();
            if entry.is_some() {
                Some(IterGuard { inner: entry })
            } else {
                None
            }
        })
    }
}
pub struct ReadGuard<'a, K, V> {
    inner: RwLockReadGuard<'a, Option<(K, V)>>,
}

impl<'a, K, V> Deref for ReadGuard<'a, K, V> {
    type Target = V;

    fn deref(&self) -> &Self::Target {
        &self.inner.as_ref().unwrap().1
    }
}

pub struct WriteGuard<'a, K, V> {
    inner: RwLockWriteGuard<'a, Option<(K, V)>>,
}

impl<'a, K, V> Deref for WriteGuard<'a, K, V> {
    type Target = V;

    fn deref(&self) -> &Self::Target {
        &self.inner.as_ref().unwrap().1
    }
}

pub struct IterGuard<'a, K, V> {
    inner: RwLockReadGuard<'a, Option<(K, V)>>,
}

impl<'a, K, V> Deref for IterGuard<'a, K, V> {
    type Target = (K, V);

    fn deref(&self) -> &Self::Target {
        self.inner.as_ref().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_insert_and_get() {
        let map = Arc::new(FixedConcurrentMap::new(16));
        assert_eq!(map.insert("key1", 10).unwrap(), None);
        assert_eq!(*map.get("key1").unwrap(), 10);
    }

    #[test]
    fn test_insert_existing_key() {
        let map = Arc::new(FixedConcurrentMap::new(16));
        assert!(map.insert("key1", 10).is_ok());
        assert_eq!(map.insert("key1", 20).unwrap(), Some(10));
        assert_eq!(*map.get("key1").unwrap(), 20);
    }

    #[test]
    fn test_remove() {
        let map = Arc::new(FixedConcurrentMap::new(16));
        assert!(map.insert("key1", 10).is_ok());
        assert_eq!(map.remove("key1"), Some(10));
        assert!(map.get("key1").is_none());
    }

    #[test]
    fn test_remove_nonexistent_key() {
        let map = Arc::new(FixedConcurrentMap::<&str, ()>::new(16));
        assert_eq!(map.remove("key1"), None);
    }

    #[test]
    fn test_table_full() {
        let map = Arc::new(FixedConcurrentMap::new(4));
        assert!(map.insert("key1", 10).is_ok());
        assert!(map.insert("key2", 20).is_ok());
        assert!(map.insert("key3", 30).is_ok());
        assert!(map.insert("key4", 40).is_ok());
        assert!(map.insert("key5", 50).is_err());
    }

    #[test]
    fn test_get_or_insert_with() {
        let map = Arc::new(FixedConcurrentMap::new(16));
        assert_eq!(*map.get_or_insert_with("key1", || 10).unwrap(), 10);
        assert_eq!(*map.get_or_insert_with("key1", || 20).unwrap(), 10);
    }

    #[test]
    fn test_retain() {
        let map = Arc::new(FixedConcurrentMap::new(16));
        assert!(map.insert("key1", 10).is_ok());
        assert!(map.insert("key2", 20).is_ok());
        assert!(map.insert("key3", 30).is_ok());

        map.retain(|_, &mut v| v != 20);

        assert_eq!(*map.get("key1").unwrap(), 10);
        assert!(map.get("key2").is_none());
        assert_eq!(*map.get("key3").unwrap(), 30);
    }

    #[test]
    fn test_iter() {
        let map = Arc::new(FixedConcurrentMap::new(16));
        assert!(map.insert("key1", 10).is_ok());
        assert!(map.insert("key2", 20).is_ok());
        assert!(map.insert("key3", 30).is_ok());

        let mut iter = map.iter().collect::<Vec<_>>();
        iter.sort_by_key(|guard| guard.0.clone());

        assert_eq!(iter.len(), 3);
        assert_eq!(*(iter[0]), ("key1", 10));
        assert_eq!(*(iter[1]), ("key2", 20));
        assert_eq!(*(iter[2]), ("key3", 30));
    }

    #[test]
    fn test_clear() {
        let map = Arc::new(FixedConcurrentMap::new(16));
        assert!(map.insert("key1", 10).is_ok());
        assert!(map.insert("key2", 20).is_ok());
        assert!(map.insert("key3", 30).is_ok());

        map.clear();

        assert!(map.get("key1").is_none());
        assert!(map.get("key2").is_none());
        assert!(map.get("key3").is_none());
    }
}
