//! Fast slot map (`Vec<Option<T>>` + free list).
//!
//! Keyed by monotonic indices or id newtypes.
//! See `docs/architecture.md §5`.

/// Slot map data structure with reusable free list.
///
/// See `docs/architecture.md §5`.
#[derive(Clone, Debug)]
pub struct Slab<T> {
    entries: Vec<Option<T>>,
    free_list: Vec<usize>,
    len: usize,
}

impl<T> Default for Slab<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Slab<T> {
    /// Create a new empty slab.
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            free_list: Vec::new(),
            len: 0,
        }
    }

    /// Create an empty slab with allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            free_list: Vec::new(),
            len: 0,
        }
    }

    /// Insert an item into the slab and return its index.
    pub fn insert(&mut self, val: T) -> usize {
        self.len += 1;
        if let Some(idx) = self.free_list.pop() {
            self.entries[idx] = Some(val);
            idx
        } else {
            let idx = self.entries.len();
            self.entries.push(Some(val));
            idx
        }
    }

    /// Insert an item whose value depends on its allocated key/id.
    pub fn insert_with_key<K>(&mut self, f: impl FnOnce(K) -> T) -> K
    where
        K: From<u32>,
    {
        let raw_idx = if let Some(&idx) = self.free_list.last() {
            idx
        } else {
            self.entries.len()
        };
        let key = K::from(raw_idx as u32);
        let val = f(key);
        let inserted_idx = self.insert(val);
        debug_assert_eq!(raw_idx, inserted_idx);
        K::from(inserted_idx as u32)
    }

    /// Get a reference to the item at `idx`.
    pub fn get(&self, idx: usize) -> Option<&T> {
        self.entries.get(idx).and_then(|opt| opt.as_ref())
    }

    /// Get a mutable reference to the item at `idx`.
    pub fn get_mut(&mut self, idx: usize) -> Option<&mut T> {
        self.entries.get_mut(idx).and_then(|opt| opt.as_mut())
    }

    /// Remove an item at `idx`.
    pub fn remove(&mut self, idx: usize) -> Option<T> {
        if idx >= self.entries.len() {
            return None;
        }
        if let Some(val) = self.entries[idx].take() {
            self.free_list.push(idx);
            self.len -= 1;
            Some(val)
        } else {
            None
        }
    }

    /// Check if an entry exists at `idx`.
    pub fn contains(&self, idx: usize) -> bool {
        self.get(idx).is_some()
    }

    /// Total number of occupied slots.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns true if empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Clear all items.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.free_list.clear();
        self.len = 0;
    }

    /// Iterator over `(index, &item)`.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &T)> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(idx, opt)| opt.as_ref().map(|val| (idx, val)))
    }

    /// Iterator over `(index, &mut item)`.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (usize, &mut T)> {
        self.entries
            .iter_mut()
            .enumerate()
            .filter_map(|(idx, opt)| opt.as_mut().map(|val| (idx, val)))
    }

    /// Iterator over `&item`.
    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().filter_map(|opt| opt.as_ref())
    }

    /// Iterator over `&mut item`.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.entries.iter_mut().filter_map(|opt| opt.as_mut())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slab_insert_get_remove_reuse() {
        let mut slab = Slab::new();
        let i0 = slab.insert("apple");
        let i1 = slab.insert("banana");
        assert_eq!(i0, 0);
        assert_eq!(i1, 1);
        assert_eq!(slab.len(), 2);

        assert_eq!(slab.get(i0), Some(&"apple"));
        assert_eq!(slab.get(i1), Some(&"banana"));

        assert_eq!(slab.remove(i0), Some("apple"));
        assert_eq!(slab.len(), 1);
        assert_eq!(slab.get(i0), None);

        // Next insert should reuse slot 0
        let i2 = slab.insert("cherry");
        assert_eq!(i2, 0);
        assert_eq!(slab.get(i2), Some(&"cherry"));
    }
}
