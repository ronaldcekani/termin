//! Application auxiliary state file (`state.json`).
//!
//! See `docs/specs/persistence.md §2`.

use serde::{Deserialize, Serialize};

/// Global application auxiliary state file (`state.json`).
///
/// See `docs/specs/persistence.md §2`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AppStateFile {
    /// List of item identifiers that the user chose "Don't ask again" on close confirmation.
    #[serde(default)]
    pub confirm_close_ignore: Vec<String>,

    /// Frecency scores for directories and palette items.
    #[serde(default)]
    pub frecency: FrecencyState,

    /// Last seen application version for release notes/migrations.
    #[serde(default)]
    pub last_seen_version: Option<String>,
}

/// Frecency scores tracking directories and commands.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FrecencyState {
    #[serde(default)]
    pub directories: Vec<FrecencyEntry>,
    #[serde(default)]
    pub palette: Vec<FrecencyEntry>,
}

/// Individual frecency tracking entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrecencyEntry {
    pub item: String,
    pub score: f64,
    pub last_accessed: u64,
}

impl FrecencyEntry {
    pub fn new(item: impl Into<String>, score: f64, last_accessed: u64) -> Self {
        Self {
            item: item.into(),
            score,
            last_accessed,
        }
    }
}
