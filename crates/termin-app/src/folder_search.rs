//! Top-bar folder search: candidate collection, fuzzy ranking and key handling.
//!
//! "Only folders for now": the dropdown lists directories gathered from open
//! tabs, workspaces, the persisted session snapshot, shell history and a
//! bounded shallow scan of `$HOME` (done once, off the main thread).

use rustc_hash::FxHashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use termin_core::{Key, KeyEvent, Mods, NamedKey, TabId, WakeHandle, WindowId};
use termin_persist::{Snapshot, TreeSnapshot};
use termin_ui::widgets::folder_search::MAX_ROWS;
use termin_ui::widgets::text_field::{FieldOutcome, TextField};

use crate::controllers::palette::fuzzy_score;
use crate::model::AppModel;

/// Maximum ranked results kept.
const MAX_RESULTS: usize = 50;
/// Cap on directories collected by the `$HOME` scan.
pub const SCAN_CAP: usize = 2000;
/// Directory names never descended into by the `$HOME` scan.
const SKIP_DIRS: [&str; 3] = ["Library", "node_modules", "target"];

/// One folder candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Absolute path.
    pub path: String,
    /// Path with the home prefix shown as `~`.
    pub display: String,
    /// Byte offset of the basename inside `display`.
    name_start: usize,
    /// Source priority (insertion order; lower = more recent / more relevant).
    rank: u32,
}

impl Candidate {
    /// Folder basename (`~` for home itself).
    pub fn name(&self) -> &str {
        &self.display[self.name_start..]
    }
}

/// Result of feeding a key to the search.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SearchKeyOutcome {
    pub redraw: bool,
    /// Folder the user chose to open.
    pub open: Option<String>,
}

/// Folder-search state owned by the app.
pub struct FolderSearch {
    pub open: bool,
    pub field: TextField,
    pub selected: usize,
    home: String,
    candidates: Vec<Candidate>,
    seen: FxHashSet<String>,
    /// Indices into `candidates`, best first.
    results: Vec<usize>,
    /// Paths found by the background job; kept across opens.
    bg_paths: Vec<String>,
    scan: Option<Receiver<Vec<String>>>,
    scan_started: bool,
}

impl Default for FolderSearch {
    fn default() -> Self {
        Self::new(std::env::var("HOME").unwrap_or_default())
    }
}

/// Replace a leading `home` with `~`.
pub fn tilde(home: &str, path: &str) -> String {
    if home.is_empty() {
        return path.to_string();
    }
    if path == home {
        return "~".to_string();
    }
    match path.strip_prefix(home) {
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}

impl FolderSearch {
    pub fn new(home: String) -> Self {
        let home = home.trim_end_matches('/').to_string();
        Self {
            open: false,
            field: TextField::new(""),
            selected: 0,
            home,
            candidates: Vec::new(),
            seen: FxHashSet::default(),
            results: Vec::new(),
            bg_paths: Vec::new(),
            scan: None,
            scan_started: false,
        }
    }

    /// Query text.
    pub fn query(&self) -> &str {
        self.field.text()
    }

    /// Whether the background job is still running.
    pub fn loading(&self) -> bool {
        self.scan.is_some()
    }

    /// Ranked results, best first.
    pub fn results(&self) -> impl ExactSizeIterator<Item = &Candidate> + '_ {
        self.results.iter().map(|&i| &self.candidates[i])
    }

    /// Number of ranked results.
    pub fn result_count(&self) -> usize {
        self.results.len()
    }

    /// Index of the first visible result row (keeps the selection on screen).
    pub fn visible_start(&self) -> usize {
        (self.selected + 1).saturating_sub(MAX_ROWS)
    }

    /// Number of rows currently visible.
    pub fn visible_rows(&self) -> usize {
        self.results
            .len()
            .saturating_sub(self.visible_start())
            .min(MAX_ROWS)
    }

    /// Absolute path of result `i`.
    pub fn path_at(&self, i: usize) -> Option<&str> {
        self.results
            .get(i)
            .map(|&c| self.candidates[c].path.as_str())
    }

    /// Add one folder if it is new. `~` prefixes expand; relative paths are dropped.
    fn add(&mut self, raw: &str, rank: &mut u32) {
        let expanded;
        let mut p = raw.trim();
        if p == "~" || p.starts_with("~/") {
            if self.home.is_empty() {
                return;
            }
            expanded = format!("{}{}", self.home, &p[1..]);
            p = &expanded;
        }
        if !p.starts_with('/') {
            return;
        }
        let p = if p.len() > 1 {
            p.trim_end_matches('/')
        } else {
            p
        };
        let p = if p.is_empty() { "/" } else { p };
        if !self.seen.insert(p.to_string()) {
            return;
        }
        let display = tilde(&self.home, p);
        let name_start = if display == "~" || display == "/" {
            0
        } else {
            display.rfind('/').map_or(0, |i| i + 1)
        };
        self.candidates.push(Candidate {
            path: p.to_string(),
            display,
            name_start,
            rank: *rank,
        });
        *rank += 1;
    }

    /// Open the dropdown with the synchronous `local` sources (best first).
    pub fn begin(&mut self, local: &[String]) {
        self.open = true;
        self.field = TextField::new("");
        self.selected = 0;
        self.candidates.clear();
        self.seen.clear();
        let mut rank = 0;
        for p in local {
            self.add(p, &mut rank);
        }
        let bg = std::mem::take(&mut self.bg_paths);
        for p in &bg {
            self.add(p, &mut rank);
        }
        self.bg_paths = bg;
        self.refilter();
    }

    /// Close the dropdown and drop the query.
    pub fn close(&mut self) {
        self.open = false;
        self.field = TextField::new("");
        self.selected = 0;
    }

    /// Start the one-time background job (persisted-session cwds + `$HOME` scan).
    pub fn start_scan(&mut self, wake: WakeHandle, state_dir: Option<PathBuf>) {
        if self.scan_started || self.home.is_empty() {
            return;
        }
        self.scan_started = true;
        let (tx, rx) = channel();
        let home = PathBuf::from(&self.home);
        let spawned = std::thread::Builder::new()
            .name("folder-scan".into())
            .spawn(move || {
                let mut out = Vec::new();
                if let Some(dir) = state_dir {
                    if let Ok(Some(snap)) = termin_persist::load(&dir) {
                        snapshot_cwds(&snap, &mut out);
                    }
                }
                scan_home(&home, SCAN_CAP, &mut out);
                let _ = tx.send(out);
                wake.wake();
            });
        if spawned.is_ok() {
            self.scan = Some(rx);
        }
    }

    /// Merge finished background results. Returns `true` if the view changed.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = self.scan.as_ref() else {
            return false;
        };
        match rx.try_recv() {
            Ok(paths) => {
                self.scan = None;
                let mut rank = self.candidates.len() as u32;
                for p in &paths {
                    self.add(p, &mut rank);
                }
                self.bg_paths = paths;
                if self.open {
                    self.refilter();
                }
                self.open
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.scan = None;
                self.open
            }
        }
    }

    fn refilter(&mut self) {
        let query = self.field.text().trim();
        let mut scored: Vec<(i32, u32, usize)> = Vec::with_capacity(self.candidates.len().min(256));
        for (i, c) in self.candidates.iter().enumerate() {
            let by_name = fuzzy_score(query, c.name()).map(|s| s + 30);
            let by_path = fuzzy_score(query, &c.display);
            if let Some(score) = by_name.max(by_path) {
                scored.push((score, c.rank, i));
            }
        }
        scored.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        scored.truncate(MAX_RESULTS);
        self.results.clear();
        self.results.extend(scored.into_iter().map(|(_, _, i)| i));
        self.selected = self.selected.min(self.results.len().saturating_sub(1));
    }

    /// Insert committed text (typing / IME / paste) into the query.
    pub fn insert_text(&mut self, text: &str) -> bool {
        if !self.field.insert(text) {
            return false;
        }
        self.selected = 0;
        self.refilter();
        true
    }

    fn open_selected(&mut self) -> Option<String> {
        let path = self.path_at(self.selected)?.to_string();
        self.close();
        Some(path)
    }

    /// Feed a key. `None` when the search is not open (the key is not consumed).
    pub fn key(&mut self, ev: &KeyEvent) -> Option<SearchKeyOutcome> {
        if !self.open {
            return None;
        }
        let mut out = SearchKeyOutcome::default();
        if !ev.pressed {
            return Some(out);
        }
        let ctrl = ev.mods.contains(Mods::CTRL);
        let cmd = ev.mods.contains(Mods::CMD);
        match ev.key {
            Key::Named(NamedKey::Escape) => {
                self.close();
                out.redraw = true;
            }
            Key::Named(NamedKey::Enter) => {
                out.open = self.open_selected();
                out.redraw = true;
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.step(1);
                out.redraw = true;
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.step(-1);
                out.redraw = true;
            }
            Key::Char('n') | Key::Char('j') if ctrl => {
                self.step(1);
                out.redraw = true;
            }
            Key::Char('p') | Key::Char('k') if ctrl => {
                self.step(-1);
                out.redraw = true;
            }
            // Cmd-P toggles the search closed again.
            Key::Char('p') | Key::Char('P') if cmd => {
                self.close();
                out.redraw = true;
            }
            Key::Named(NamedKey::Tab) => {}
            _ => match self.field.handle_key(ev) {
                FieldOutcome::Edited => {
                    self.selected = 0;
                    self.refilter();
                    out.redraw = true;
                }
                FieldOutcome::Ignored | FieldOutcome::Commit | FieldOutcome::Cancel => {}
            },
        }
        Some(out)
    }

    fn step(&mut self, delta: i32) {
        let n = self.results.len();
        if n == 0 {
            return;
        }
        self.selected = (self.selected as i64 + i64::from(delta)).rem_euclid(n as i64) as usize;
    }
}

/// Synchronous folder sources for `win`, best first: the focused tab's cwd,
/// other tabs' cwds, workspace paths, then `history` cwds.
pub fn local_sources(model: &AppModel, win: WindowId, history: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let focused = model.focused_tab();
    if let Some(cwd) = focused
        .and_then(|t| model.tabs.get(t))
        .and_then(|t| t.cwd.as_ref())
    {
        out.push(cwd.clone());
    }
    let mut tab_cwds = |ws_id| {
        let Some(ws) = model.workspaces.get(ws_id) else {
            return;
        };
        for pane_id in ws.tree.leaves() {
            let Some(pane) = model.panes.get(pane_id) else {
                continue;
            };
            let active = std::iter::once(pane.active_tab);
            for tab_id in active.chain(pane.tabs.iter().copied().filter(|t| *t != pane.active_tab))
            {
                if let Some(cwd) = model.tabs.get(tab_id).and_then(|t| t.cwd.as_ref()) {
                    out.push(cwd.clone());
                }
            }
        }
    };
    if let Some(window) = model.windows.get(win) {
        tab_cwds(window.active_workspace);
        for &ws in &window.workspaces {
            if ws != window.active_workspace {
                tab_cwds(ws);
            }
        }
        for &ws_id in &window.workspaces {
            if let Some(ws) = model.workspaces.get(ws_id) {
                out.extend(ws.meta.paths.iter().cloned());
            }
        }
    }
    out.extend(history.iter().cloned());
    out
}

/// Open `path` as a new tab (with that cwd) in the focused pane. Returns the new tab.
pub fn open_folder(model: &mut AppModel, path: &str) -> Option<TabId> {
    let pane = model.focused_pane()?;
    model.new_tab_with_cwd(pane, Some(path.to_string()))
}

/// Collect tab cwds from a persisted snapshot (every window, every pane).
pub fn snapshot_cwds(snap: &Snapshot, out: &mut Vec<String>) {
    fn walk(t: &TreeSnapshot, out: &mut Vec<String>) {
        match t {
            TreeSnapshot::Pane { pane } => out.extend(pane.tabs.iter().map(|t| t.cwd.clone())),
            TreeSnapshot::Split { a, b, .. } => {
                walk(a, out);
                walk(b, out);
            }
        }
    }
    for w in &snap.windows {
        for ws in &w.workspaces {
            walk(&ws.tree, out);
        }
    }
}

fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || SKIP_DIRS.contains(&name)
}

fn child_dirs(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<(String, PathBuf)> = rd
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            (!skip_dir(&name)).then(|| (name, e.path()))
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// Shallow scan of `home` (depth <= 2, no dot-dirs, `Library`, `node_modules`),
/// appending at most `cap` directories to `out`; depth-1 entries come first.
pub fn scan_home(home: &Path, cap: usize, out: &mut Vec<String>) {
    let start = out.len();
    let level1 = child_dirs(home);
    for (_, p) in &level1 {
        if out.len() - start >= cap {
            return;
        }
        out.push(p.to_string_lossy().into_owned());
    }
    for (_, p) in &level1 {
        for (_, sub) in child_dirs(p) {
            if out.len() - start >= cap {
                return;
            }
            out.push(sub.to_string_lossy().into_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termin_core::Size;

    fn key(k: Key, mods: Mods, text: Option<&str>) -> KeyEvent {
        KeyEvent {
            key: k,
            physical: 0,
            mods,
            text: text.map(Into::into),
            repeat: false,
            pressed: true,
        }
    }

    fn named(k: NamedKey) -> KeyEvent {
        key(Key::Named(k), Mods::empty(), None)
    }

    fn search() -> FolderSearch {
        let mut s = FolderSearch::new("/Users/me".into());
        s.begin(&[
            "/Users/me/code/termin".into(),
            "/Users/me/code/other".into(),
            "~/docs".into(),
            "/Users/me/code/termin/".into(), // duplicate (trailing slash)
            "relative/path".into(),          // dropped
            "/Users/mel/x".into(),           // not under home
        ]);
        s
    }

    #[test]
    fn dedup_tilde_display_and_order() {
        let s = search();
        let shown: Vec<_> = s.results().map(|c| c.display.as_str()).collect();
        assert_eq!(
            shown,
            ["~/code/termin", "~/code/other", "~/docs", "/Users/mel/x"]
        );
        assert_eq!(s.results().next().unwrap().name(), "termin");
        assert_eq!(tilde("/Users/me", "/Users/me"), "~");
        assert_eq!(tilde("/Users/me", "/Users/me2/x"), "/Users/me2/x");
    }

    #[test]
    fn fuzzy_beats_recency_then_recency_breaks_ties() {
        let mut s = search();
        assert!(s.insert_text("oth"));
        assert_eq!(s.results().next().unwrap().display, "~/code/other");
        assert!(s.insert_text("zzz"));
        assert_eq!(s.result_count(), 0);
        // Single-character queries work.
        let mut s = search();
        s.insert_text("d");
        assert!(s.result_count() > 0);
    }

    #[test]
    fn keys_navigate_close_and_open() {
        let mut s = search();
        assert!(s.key(&named(NamedKey::ArrowDown)).unwrap().redraw);
        assert_eq!(s.selected, 1);
        s.key(&named(NamedKey::ArrowUp));
        s.key(&named(NamedKey::ArrowUp)); // wraps
        assert_eq!(s.selected, s.result_count() - 1);
        let out = s.key(&named(NamedKey::Enter)).unwrap();
        assert_eq!(out.open.as_deref(), Some("/Users/mel/x"));
        assert!(!s.open);
        // Closed: keys are not consumed.
        assert!(s.key(&named(NamedKey::Enter)).is_none());

        let mut s = search();
        s.key(&key(Key::Char('d'), Mods::empty(), Some("d")));
        assert_eq!(s.query(), "d");
        assert!(s.key(&named(NamedKey::Escape)).unwrap().redraw);
        assert!(!s.open);
        assert_eq!(s.query(), "");

        let mut s = search();
        s.insert_text("zzz");
        assert_eq!(s.key(&named(NamedKey::Enter)).unwrap().open, None);
        assert!(s.open, "Enter with no match keeps the search open");
        s.key(&key(Key::Char('p'), Mods::CMD, None));
        assert!(!s.open);
    }

    #[test]
    fn enter_opens_new_tab_with_cwd() {
        let mut model = AppModel::new();
        model.new_window(None, Size::new(900.0, 600.0), None);
        model.drain_effects();
        let before = model.tabs.len();
        let mut s = search();
        s.insert_text("docs");
        let path = s.key(&named(NamedKey::Enter)).unwrap().open.unwrap();
        let tab = open_folder(&mut model, &path).expect("tab");
        assert_eq!(model.tabs.len(), before + 1);
        assert_eq!(
            model.tabs.get(tab).unwrap().cwd.as_deref(),
            Some("/Users/me/docs")
        );
        assert_eq!(model.focused_tab(), Some(tab));
        assert!(model.drain_effects().iter().any(|e| matches!(
            e,
            crate::model::ModelEffect::SpawnTab { tab: t, cwd: Some(c), .. }
                if *t == tab && c == "/Users/me/docs"
        )));
    }

    #[test]
    fn local_sources_prefer_focused_tab() {
        let mut model = AppModel::new();
        let win = model.new_window(None, Size::new(900.0, 600.0), Some("/a".into()));
        let pane = model.focused_pane().unwrap();
        model.new_tab_with_cwd(pane, Some("/b".into()));
        let src = local_sources(&model, win, &["/h".into()]);
        assert_eq!(src.first().map(String::as_str), Some("/b"));
        assert!(src.contains(&"/a".to_string()));
        assert_eq!(src.last().map(String::as_str), Some("/h"));
    }

    #[test]
    fn scan_is_bounded_shallow_and_skips_noise() {
        let root = std::env::temp_dir().join(format!("termin-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in [
            "proj/src/deep",
            "proj/node_modules/x",
            ".hidden/a",
            "Library/Caches",
            "docs",
        ] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("file.txt"), b"x").unwrap();
        let mut out = Vec::new();
        scan_home(&root, 100, &mut out);
        let rel: Vec<_> = out
            .iter()
            .map(|p| p.strip_prefix(root.to_str().unwrap()).unwrap().to_string())
            .collect();
        assert_eq!(rel, ["/docs", "/proj", "/proj/src"]);
        let mut capped = Vec::new();
        scan_home(&root, 2, &mut capped);
        assert_eq!(capped.len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn snapshot_cwds_walks_splits() {
        use termin_persist::{PaneSnapshot, TabSnapshot};
        let leaf = |c: &str| {
            TreeSnapshot::pane(PaneSnapshot::new(
                vec![TabSnapshot::new(c, None, "auto")],
                0,
            ))
        };
        let mut snap = Snapshot::fresh();
        snap.windows[0].workspaces[0].tree =
            TreeSnapshot::split("right", 0.5, leaf("/x"), leaf("/y"));
        let mut out = Vec::new();
        snapshot_cwds(&snap, &mut out);
        assert_eq!(out, ["/x", "/y"]);
    }

    #[test]
    fn background_results_merge_after_local() {
        let mut s = search();
        let (tx, rx) = channel();
        s.scan = Some(rx);
        tx.send(vec![
            "/Users/me/code/termin".into(),
            "/Users/me/zeta".into(),
        ])
        .unwrap();
        assert!(s.poll());
        assert!(!s.loading());
        let shown: Vec<_> = s.results().map(|c| c.display.clone()).collect();
        assert_eq!(shown.last().map(String::as_str), Some("~/zeta"));
        assert_eq!(shown.iter().filter(|d| *d == "~/code/termin").count(), 1);
    }
}
