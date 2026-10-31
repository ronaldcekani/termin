# Spec: Session Restore and Persisted State (`termin-persist`) (D5)

## 1. What is restored

On launch with `window.restore_session = true`:
- Windows: frame, screen, full-screen state, sidebar width/visibility.
- Workspaces: order, name (if user-set), active workspace per window.
- Split tree: structure + ratios; zoomed pane.
- Panes: tab order, active tab.
- Tabs: cwd, user-set title, mode preference (block/classic toggle).
- Focus: focused window/workspace/pane/tab.

**Not** restored: processes, scrollback, running commands (fresh shell in saved cwd). Optional `window.restore_scrollback = false` reserved for later (would store last N lines per tab as compact text).

## 2. Files

`~/Library/Application Support/Termin/`:

| File | Content |
|---|---|
| `session.json` | snapshot (below) |
| `state.json` | small app state: confirm-close "don't ask again" list, frecency for directories/palette, last-seen version |
| `history.tsv` | Termin command history (see `input-editor.md §3`) |

Writes are atomic: write `*.tmp` + `fsync` + `rename`.

## 3. Snapshot schema (`session.json`, version 1)

```json
{
  "version": 1,
  "saved_at": 1793476469,
  "focused_window": 0,
  "windows": [{
    "frame": [x, y, w, h], "screen": "display-uuid", "fullscreen": false,
    "sidebar": {"visible": true, "width": 240},
    "active_workspace": 1,
    "workspaces": [{
      "name": "NARUTO",
      "focused_pane": [0, 1],                  
      "zoomed": null,
      "tree": {"split": "right", "ratio": 0.5,
               "a": {"pane": {"tabs": [{"cwd": "~/proj", "title": null, "mode": "auto"}], "active": 0}},
               "b": {"split": "down", "ratio": 0.6, "a": {...}, "b": {...}}}
    }]
  }]
}
```

Pane addressing inside a snapshot is by tree path (`[0,1]` = a→b), not runtime ids (ids are not stable across launches).

## 4. Lifecycle

- Save triggers: any structural `Action` (split, close, move, rename, resize-split end, window move/resize end) → mark dirty → debounce 1 s → serialize on main (snapshot is tiny; < 0.2 ms) → write on worker thread.
- On quit (`applicationWillTerminate`): synchronous save.
- Crash safety: the 1 s debounce means at most 1 s of layout changes lost.
- Load: parse; on any error, rename to `session.json.bad-<ts>` and start fresh with one window.
- Missing cwd on restore → fall back to nearest existing ancestor, then `$HOME`.
- Windows positioned on a now-missing screen → moved to main screen.

## 5. API

```rust
pub struct Snapshot { ... }            // serde types mirroring §3
pub fn load(dir: &Path) -> Result<Option<Snapshot>, Error>;
pub fn save(dir: &Path, s: &Snapshot) -> Result<(), Error>;
pub struct AppStateFile { ... } pub fn load_state(..); pub fn save_state(..);
```

`termin-app` owns conversion `App ↔ Snapshot`.
