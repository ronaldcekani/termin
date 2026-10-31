//! Session snapshot persistence and state restoration (`termin-persist`).
//!
//! See `docs/specs/persistence.md`.

pub mod error;
pub mod io;
pub mod snapshot;
pub mod state;

pub use error::PersistError;
pub use io::{load, load_state, save, save_state};
pub use snapshot::{
    PaneSnapshot, SidebarSnapshot, Snapshot, TabSnapshot, TreeSnapshot, WindowSnapshot,
    WorkspaceSnapshot, SNAPSHOT_VERSION,
};
pub use state::{AppStateFile, FrecencyEntry, FrecencyState};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_spec_schema_round_trip() {
        let json_text = r#"{
  "version": 1,
  "saved_at": 1793476469,
  "focused_window": 0,
  "windows": [{
    "frame": [10.0, 20.0, 1200.0, 800.0],
    "screen": "display-uuid-1234",
    "fullscreen": false,
    "sidebar": {"visible": true, "width": 240.0},
    "active_workspace": 1,
    "workspaces": [{
      "name": "NARUTO",
      "focused_pane": [0, 1],
      "zoomed": null,
      "tree": {
        "split": "right",
        "ratio": 0.5,
        "a": {
          "pane": {
            "tabs": [{"cwd": "~/proj", "title": null, "mode": "auto"}],
            "active": 0
          }
        },
        "b": {
          "split": "down",
          "ratio": 0.6,
          "a": {
            "pane": {
              "tabs": [{"cwd": "~/code", "title": "server", "mode": "block"}],
              "active": 0
            }
          },
          "b": {
            "pane": {
              "tabs": [{"cwd": "~/notes", "title": null, "mode": "classic"}],
              "active": 0
            }
          }
        }
      }
    }]
  }]
}"#;

        let snapshot: Snapshot = serde_json::from_str(json_text).expect("deserialize valid spec schema");
        assert_eq!(snapshot.version, 1);
        assert_eq!(snapshot.saved_at, 1793476469);
        assert_eq!(snapshot.focused_window, 0);
        assert_eq!(snapshot.windows.len(), 1);

        let win = &snapshot.windows[0];
        assert_eq!(win.frame, [10.0, 20.0, 1200.0, 800.0]);
        assert_eq!(win.screen, "display-uuid-1234");
        assert!(!win.fullscreen);
        assert!(win.sidebar.visible);
        assert_eq!(win.sidebar.width, 240.0);
        assert_eq!(win.active_workspace, 1);
        assert_eq!(win.workspaces.len(), 1);

        let ws = &win.workspaces[0];
        assert_eq!(ws.name, "NARUTO");
        assert_eq!(ws.focused_pane, vec![0, 1]);
        assert!(ws.zoomed.is_none());

        match &ws.tree {
            TreeSnapshot::Split { split, ratio, a, b } => {
                assert_eq!(split, "right");
                assert!((ratio - 0.5).abs() < 1e-6);
                match a.as_ref() {
                    TreeSnapshot::Pane { pane } => {
                        assert_eq!(pane.tabs.len(), 1);
                        assert_eq!(pane.tabs[0].cwd, "~/proj");
                        assert_eq!(pane.active, 0);
                    }
                    _ => panic!("expected pane in a"),
                }
                match b.as_ref() {
                    TreeSnapshot::Split { split: s2, ratio: r2, .. } => {
                        assert_eq!(s2, "down");
                        assert!((r2 - 0.6).abs() < 1e-6);
                    }
                    _ => panic!("expected split in b"),
                }
            }
            _ => panic!("expected root split"),
        }

        // Re-serialize and deserialize again
        let serialized = serde_json::to_string_pretty(&snapshot).unwrap();
        let decoded: Snapshot = serde_json::from_str(&serialized).unwrap();
        assert_eq!(snapshot, decoded);
    }

    #[test]
    fn test_atomic_save_and_load() {
        let temp_dir = std::env::temp_dir().join(format!("termin-test-persist-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // Initially missing
        let initial = load(&temp_dir).unwrap();
        assert!(initial.is_none());

        // Save fresh snapshot
        let snap = Snapshot::fresh();
        save(&temp_dir, &snap).unwrap();

        // Load saved snapshot
        let loaded = load(&temp_dir).unwrap().expect("loaded snapshot");
        assert_eq!(loaded.version, snap.version);
        assert_eq!(loaded.windows.len(), snap.windows.len());

        // Clean up
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_corrupted_file_recovery() {
        let temp_dir = std::env::temp_dir().join(format!("termin-test-corrupt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let session_file = temp_dir.join("session.json");
        let bad_content = b"{\"version\": 1, \"corrupt\": true [incomplete json";
        fs::write(&session_file, bad_content).unwrap();

        // Loading should detect corruption, quarantine to bad-<ts>, and return fresh fallback
        let loaded = load(&temp_dir).unwrap().expect("fresh fallback returned");
        assert_eq!(loaded.version, SNAPSHOT_VERSION);
        assert_eq!(loaded.windows.len(), 1);

        // Verify session.json is no longer the corrupt file
        assert!(!session_file.exists());

        // Verify quarantined file exists with .bad- prefix
        let mut found_bad = false;
        for entry in fs::read_dir(&temp_dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("session.json.bad-") {
                found_bad = true;
                let quarantined_bytes = fs::read(entry.path()).unwrap();
                assert_eq!(quarantined_bytes, bad_content);
            }
        }
        assert!(found_bad, "Quarantined bad session file must exist");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_unsupported_version_recovery() {
        let temp_dir = std::env::temp_dir().join(format!("termin-test-ver-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let session_file = temp_dir.join("session.json");
        let future_json = r#"{"version": 999, "saved_at": 1, "focused_window": 0, "windows": []}"#;
        fs::write(&session_file, future_json).unwrap();

        let loaded = load(&temp_dir).unwrap().expect("fresh fallback returned");
        assert_eq!(loaded.version, SNAPSHOT_VERSION);

        let mut found_bad = false;
        for entry in fs::read_dir(&temp_dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("session.json.bad-") {
                found_bad = true;
            }
        }
        assert!(found_bad);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_state_file_atomic_save_load_and_corrupt() {
        let temp_dir = std::env::temp_dir().join(format!("termin-test-state-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let missing = load_state(&temp_dir).unwrap();
        assert!(missing.is_none());

        let mut state = AppStateFile::default();
        state.confirm_close_ignore.push("running_process".to_string());
        state.last_seen_version = Some("0.1.0".to_string());
        state.frecency.directories.push(FrecencyEntry::new("/tmp/proj", 10.5, 1793476469));

        save_state(&temp_dir, &state).unwrap();

        let loaded = load_state(&temp_dir).unwrap().expect("loaded state");
        assert_eq!(loaded, state);

        // Corrupt state file
        let state_file = temp_dir.join("state.json");
        fs::write(&state_file, b"corrupted state json!").unwrap();

        let recovered = load_state(&temp_dir).unwrap().expect("recovered default state");
        assert_eq!(recovered, AppStateFile::default());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
