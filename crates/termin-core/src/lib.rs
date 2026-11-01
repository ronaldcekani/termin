//! Core primitives and contracts for Termin.
//!
//! See `docs/architecture.md §3.1`.

pub mod color;
pub mod geometry;
pub mod id;
pub mod input;
pub mod pty;
pub mod slab;
pub mod small_str;
pub mod trace;
pub mod wake;

pub use color::Rgba;
pub use geometry::{Point, Rect, Size};
pub use id::{BlockId, PaneId, TabId, WindowId, WorkspaceId};
pub use input::{
    Key, KeyEvent, Mods, MouseButton, MouseEvent, MouseKind, NamedKey, ScrollEvent, ScrollPhase,
};
pub use pty::PtyConsumer;
pub use slab::Slab;
pub use small_str::SmallStr;
pub use wake::WakeHandle;
