//! Subsystem action controllers (`termin-app::controllers`).
//!
//! Routes actions to specific controllers for windows, workspaces, panes, tabs,
//! terminal, blocks, editor, palette, find, attention, and config.
//! See `docs/specs/app-model-and-ui.md §7` and `docs/implementation-phases.md Phase 2 / Phase 3`.

pub mod attention;
pub mod blocks;
pub mod cli_install;
pub mod completion;
pub mod config;
pub mod editor;
pub mod find;
pub mod palette;
pub mod panes;
pub mod persist;
pub mod tabs;
pub mod terminal;
pub mod windows;
pub mod workspaces;
