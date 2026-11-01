//! Atomic persistence I/O and recovery operations.
//!
//! See `docs/specs/persistence.md §2, §4`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::PersistError;
use crate::snapshot::{Snapshot, SNAPSHOT_VERSION};
use crate::state::AppStateFile;

/// Atomically write bytes to file by writing to a temporary file, fsyncing, and renaming.
fn atomic_write(dest: &Path, bytes: &[u8]) -> Result<(), PersistError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let pid = std::process::id();
    let tmp_path = dest.with_extension(format!("tmp.{}", pid));

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp_path)?;

    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);

    fs::rename(&tmp_path, dest)?;
    Ok(())
}

/// Find a non-colliding bad filename: `<filename>.bad-<ts>`
fn find_bad_path(original: &Path) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let ts = now.as_secs();
    let millis = now.subsec_millis();

    let parent = original.parent().unwrap_or_else(|| Path::new("."));
    let file_name = original
        .file_name()
        .map(|f| f.to_string_lossy())
        .unwrap_or_else(|| "file".into());

    let candidate = parent.join(format!("{}.bad-{}", file_name, ts));
    if !candidate.exists() {
        return candidate;
    }

    let candidate_with_ms = parent.join(format!("{}.bad-{}-{}", file_name, ts, millis));
    if !candidate_with_ms.exists() {
        return candidate_with_ms;
    }

    let mut counter = 1;
    loop {
        let p = parent.join(format!("{}.bad-{}-{}-{}", file_name, ts, millis, counter));
        if !p.exists() {
            return p;
        }
        counter += 1;
    }
}

/// Read-only variant of [`load`]: parses `session.json` without quarantining, renaming or
/// writing anything. Returns `None` if the file is missing, unreadable, malformed or of an
/// unsupported version. Safe to call from background threads.
pub fn load_readonly(dir: &Path) -> Option<Snapshot> {
    let data = fs::read(dir.join("session.json")).ok()?;
    let snapshot: Snapshot = serde_json::from_slice(&data).ok()?;
    (snapshot.version == SNAPSHOT_VERSION).then_some(snapshot)
}

/// Load session snapshot from the given directory (e.g. `Application Support/Termin/`).
///
/// If `session.json` is missing, returns `Ok(None)`.
/// If `session.json` is corrupt or invalid, renames it to `session.json.bad-<ts>`
/// and returns a fresh fallback snapshot (`Ok(Some(Snapshot::fresh()))`).
///
/// See `docs/specs/persistence.md §4`.
pub fn load(dir: &Path) -> Result<Option<Snapshot>, PersistError> {
    let path = dir.join("session.json");
    if !path.exists() {
        return Ok(None);
    }

    let read_and_parse = || -> Result<Snapshot, PersistError> {
        let data = fs::read(&path)?;
        let snapshot: Snapshot = serde_json::from_slice(&data)?;
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(PersistError::UnsupportedVersion(snapshot.version));
        }
        Ok(snapshot)
    };

    match read_and_parse() {
        Ok(snapshot) => Ok(Some(snapshot)),
        Err(err) => {
            let bad_path = find_bad_path(&path);
            log::warn!(
                "Failed to load session snapshot ({:?}); quarantining to {:?}",
                err,
                bad_path
            );

            // Attempt to rename the bad file to isolate it
            let _ = fs::rename(&path, &bad_path);

            // Return fresh fallback per spec §4
            Ok(Some(Snapshot::fresh()))
        }
    }
}

/// Atomically save session snapshot to `session.json` in the given directory.
///
/// See `docs/specs/persistence.md §2, §4`.
pub fn save(dir: &Path, snapshot: &Snapshot) -> Result<(), PersistError> {
    let path = dir.join("session.json");
    let data = serde_json::to_vec_pretty(snapshot)?;
    atomic_write(&path, &data)
}

/// Load app auxiliary state (`state.json`) from the given directory.
///
/// If `state.json` is missing, returns `Ok(None)`.
/// If `state.json` is corrupt, renames it to `state.json.bad-<ts>` and returns `Ok(Some(AppStateFile::default()))`.
///
/// See `docs/specs/persistence.md §2`.
pub fn load_state(dir: &Path) -> Result<Option<AppStateFile>, PersistError> {
    let path = dir.join("state.json");
    if !path.exists() {
        return Ok(None);
    }

    let read_and_parse = || -> Result<AppStateFile, PersistError> {
        let data = fs::read(&path)?;
        let state: AppStateFile = serde_json::from_slice(&data)?;
        Ok(state)
    };

    match read_and_parse() {
        Ok(state) => Ok(Some(state)),
        Err(err) => {
            let bad_path = find_bad_path(&path);
            log::warn!(
                "Failed to load app state ({:?}); quarantining to {:?}",
                err,
                bad_path
            );
            let _ = fs::rename(&path, &bad_path);
            Ok(Some(AppStateFile::default()))
        }
    }
}

/// Atomically save app auxiliary state (`state.json`) in the given directory.
///
/// See `docs/specs/persistence.md §2`.
pub fn save_state(dir: &Path, state: &AppStateFile) -> Result<(), PersistError> {
    let path = dir.join("state.json");
    let data = serde_json::to_vec_pretty(state)?;
    atomic_write(&path, &data)
}
