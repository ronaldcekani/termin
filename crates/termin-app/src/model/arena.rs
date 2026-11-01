//! Id-keyed arena with monotonic, never-reused ids.
//!
//! Entities are stored in a `Vec<(raw_id, T)>` sorted by id (ids are handed out
//! in increasing order, so insertion is a push). Lookup is a binary search.
//! Populations are tiny (tens), so this beats hashing and keeps iteration order
//! deterministic (= creation order). See `docs/specs/app-model-and-ui.md §12`.

use std::marker::PhantomData;
use termin_core::{PaneId, TabId, WindowId, WorkspaceId};

/// Id newtype usable as an [`Arena`] key.
pub trait ArenaId: Copy + Eq {
    /// Build from the raw `u32`.
    fn from_raw(raw: u32) -> Self;
    /// Raw `u32` value.
    fn raw(self) -> u32;
}

macro_rules! arena_id {
    ($($t:ty),*) => {$(
        impl ArenaId for $t {
            fn from_raw(raw: u32) -> Self { Self(raw) }
            fn raw(self) -> u32 { self.0 }
        }
    )*};
}
arena_id!(WindowId, WorkspaceId, PaneId, TabId);

/// Sorted-vector map from id to entity; ids are never reused.
#[derive(Clone, Debug)]
pub struct Arena<I, T> {
    items: Vec<(u32, T)>,
    next: u32,
    _id: PhantomData<fn() -> I>,
}

impl<I, T> Default for Arena<I, T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            next: 1,
            _id: PhantomData,
        }
    }
}

impl<I: ArenaId, T> Arena<I, T> {
    /// Empty arena (first id is 1).
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a value built from its freshly allocated id.
    pub fn insert_with(&mut self, f: impl FnOnce(I) -> T) -> I {
        let raw = self.next;
        self.next += 1;
        let id = I::from_raw(raw);
        self.items.push((raw, f(id)));
        id
    }

    /// Insert under an externally allocated id (e.g. platform window ids).
    /// Returns `false` (and drops `val`) if the id is already present.
    pub fn insert_at(&mut self, id: I, val: T) -> bool {
        let raw = id.raw();
        match self.items.binary_search_by_key(&raw, |e| e.0) {
            Ok(_) => false,
            Err(pos) => {
                self.items.insert(pos, (raw, val));
                self.next = self.next.max(raw.saturating_add(1));
                true
            }
        }
    }

    fn pos(&self, id: I) -> Option<usize> {
        self.items.binary_search_by_key(&id.raw(), |e| e.0).ok()
    }

    /// Shared lookup.
    pub fn get(&self, id: I) -> Option<&T> {
        self.pos(id).map(|p| &self.items[p].1)
    }

    /// Mutable lookup.
    pub fn get_mut(&mut self, id: I) -> Option<&mut T> {
        self.pos(id).map(|p| &mut self.items[p].1)
    }

    /// Remove and return an entity.
    pub fn remove(&mut self, id: I) -> Option<T> {
        self.pos(id).map(|p| self.items.remove(p).1)
    }

    /// Whether `id` is present.
    pub fn contains(&self, id: I) -> bool {
        self.pos(id).is_some()
    }

    /// Number of live entities.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// No live entities.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// `(id, &entity)` in id (creation) order.
    pub fn iter(&self) -> impl Iterator<Item = (I, &T)> {
        self.items.iter().map(|(r, v)| (I::from_raw(*r), v))
    }

    /// `(id, &mut entity)` in id order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (I, &mut T)> {
        self.items.iter_mut().map(|(r, v)| (I::from_raw(*r), v))
    }

    /// Ids in creation order.
    pub fn ids(&self) -> impl Iterator<Item = I> + '_ {
        self.items.iter().map(|(r, _)| I::from_raw(*r))
    }

    /// Entities in id order.
    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.items.iter().map(|(_, v)| v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_monotonic_never_reused() {
        let mut a: Arena<PaneId, &str> = Arena::new();
        let x = a.insert_with(|_| "x");
        let y = a.insert_with(|_| "y");
        assert!(a.remove(x).is_some());
        let z = a.insert_with(|_| "z");
        assert!(z.0 > y.0 && z != x);
        assert!(a.get(x).is_none());
        assert_eq!(a.get(z), Some(&"z"));
    }

    #[test]
    fn external_ids() {
        let mut a: Arena<WindowId, u8> = Arena::new();
        assert!(a.insert_at(WindowId(7), 1));
        assert!(!a.insert_at(WindowId(7), 2));
        assert!(a.insert_at(WindowId(3), 3));
        assert_eq!(a.ids().collect::<Vec<_>>(), vec![WindowId(3), WindowId(7)]);
        assert_eq!(a.insert_with(|_| 9), WindowId(8));
    }
}
