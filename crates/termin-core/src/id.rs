//! Identifiers and monotonic allocator.
//!
//! See `docs/architecture.md §3.1`.

use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_WINDOW_ID: AtomicU32 = AtomicU32::new(1);
static NEXT_WORKSPACE_ID: AtomicU32 = AtomicU32::new(1);
static NEXT_PANE_ID: AtomicU32 = AtomicU32::new(1);
static NEXT_TAB_ID: AtomicU32 = AtomicU32::new(1);
static NEXT_BLOCK_ID: AtomicU32 = AtomicU32::new(1);

/// Window identifier (monotonically allocated, unique per process).
///
/// See `docs/architecture.md §3.1`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct WindowId(pub u32);

impl WindowId {
    /// Allocate the next monotonic `WindowId`.
    pub fn next() -> Self {
        Self(NEXT_WINDOW_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Access the underlying `u32` value.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for WindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "win:{}", self.0)
    }
}

impl From<u32> for WindowId {
    fn from(val: u32) -> Self {
        Self(val)
    }
}

impl From<WindowId> for u32 {
    fn from(id: WindowId) -> Self {
        id.0
    }
}

/// Workspace identifier (monotonically allocated, unique per process).
///
/// See `docs/architecture.md §3.1`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct WorkspaceId(pub u32);

impl WorkspaceId {
    /// Allocate the next monotonic `WorkspaceId`.
    pub fn next() -> Self {
        Self(NEXT_WORKSPACE_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Access the underlying `u32` value.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ws:{}", self.0)
    }
}

impl From<u32> for WorkspaceId {
    fn from(val: u32) -> Self {
        Self(val)
    }
}

impl From<WorkspaceId> for u32 {
    fn from(id: WorkspaceId) -> Self {
        id.0
    }
}

/// Pane identifier (monotonically allocated, unique per process).
///
/// See `docs/architecture.md §3.1`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PaneId(pub u32);

impl PaneId {
    /// Allocate the next monotonic `PaneId`.
    pub fn next() -> Self {
        Self(NEXT_PANE_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Access the underlying `u32` value.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for PaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pane:{}", self.0)
    }
}

impl From<u32> for PaneId {
    fn from(val: u32) -> Self {
        Self(val)
    }
}

impl From<PaneId> for u32 {
    fn from(id: PaneId) -> Self {
        id.0
    }
}

/// Tab identifier (session id; monotonically allocated, unique per process).
///
/// See `docs/architecture.md §3.1`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct TabId(pub u32);

impl TabId {
    /// Allocate the next monotonic `TabId`.
    pub fn next() -> Self {
        Self(NEXT_TAB_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Access the underlying `u32` value.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for TabId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tab:{}", self.0)
    }
}

impl From<u32> for TabId {
    fn from(val: u32) -> Self {
        Self(val)
    }
}

impl From<TabId> for u32 {
    fn from(id: TabId) -> Self {
        id.0
    }
}

/// Block identifier (unique per tab).
///
/// See `docs/architecture.md §3.1` and `docs/specs/terminal-core.md §6`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct BlockId(pub u32);

impl BlockId {
    /// Allocate the next monotonic `BlockId`.
    pub fn next() -> Self {
        Self(NEXT_BLOCK_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Access the underlying `u32` value.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "block:{}", self.0)
    }
}

impl From<u32> for BlockId {
    fn from(val: u32) -> Self {
        Self(val)
    }
}

impl From<BlockId> for u32 {
    fn from(id: BlockId) -> Self {
        id.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id_allocations_are_monotonic() {
        let w1 = WindowId::next();
        let w2 = WindowId::next();
        assert!(w2.0 > w1.0);

        let t1 = TabId::next();
        let t2 = TabId::next();
        assert!(t2.0 > t1.0);
    }

    #[test]
    fn test_id_display() {
        let wid = WindowId(42);
        assert_eq!(wid.to_string(), "win:42");
        let bid = BlockId(7);
        assert_eq!(bid.to_string(), "block:7");
    }
}
