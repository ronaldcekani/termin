# Spec: App Model and UI (`termin-app`, `termin-ui`)

Visual specs (sizes, colors, states) live in `design/components.md`. This doc defines **structure and behavior**.

## 1. Model hierarchy (D13, D14)

```
App
 └─ Window (NSWindow)                     many
     ├─ sidebar: ordered Vec<WorkspaceId>
     ├─ active_workspace: WorkspaceId
     └─ Workspace                         many per window
         ├─ name: Option<String>          None → auto name (see §3.2)
         ├─ tree: SplitTree               leaves are PaneIds
         ├─ focused_pane: PaneId
         ├─ zoomed_pane: Option<PaneId>
         └─ Pane                          many per workspace
             ├─ tabs: SmallVec<[TabId; 4]> ordered
             ├─ active_tab: TabId
             └─ Tab (Session)             many per pane
                 ├─ session: Arc<Session> (PTY + Terminal)
                 ├─ title: user override | OSC title | foreground process name
                 ├─ mode: Block | Classic   (+ alt-screen override)
                 ├─ editor: Option<EditorState>
                 ├─ view: TabView { scroll, selection, search, block_selection }
                 └─ attention: Attention
```

Invariants:
- A window always has ≥ 1 workspace; a workspace always has ≥ 1 pane; a pane always has ≥ 1 tab. Closing the last child closes the parent (last window closing does **not** quit the app — standard macOS; configurable `app.quit_on_last_window_close`).
- Ids are never reused. All references are ids into `Slab`s (`architecture.md §5`).

## 2. Split tree

```rust
enum Node { Leaf(PaneId), Split { dir: Dir /* Right=horizontal row, Down=vertical column */, ratio: f32 /* 0.1..0.9 of first child */, a: NodeIdx, b: NodeIdx } }
struct SplitTree { nodes: Vec<Node>, root: NodeIdx }   // arena; compacted on remove
```

Operations (all are `Action`s):
- `SplitRight` / `SplitDown` on focused pane: replace leaf with `Split{dir, 0.5, old, new}`; new pane gets a new tab whose cwd = focused tab's cwd (OSC 7) when `split.inherit_cwd = true` (default).
- `ClosePane`: remove leaf; sibling takes the parent's place.
- `ResizeSplit { dir, delta }`: adjust the nearest ancestor split in that direction (keyboard: ⌘⌃ + arrows, 5 % steps); mouse drag on divider changes that split's ratio directly. Minimum pane size: 12 cols × 3 rows (+ chrome).
- `EqualizeSplits`: set all ratios so leaves get equal space per direction.
- `FocusPane(direction)`: geometric navigation — choose the pane whose rect is adjacent in that direction with the greatest overlap on the perpendicular axis, tie-break by most-recently-focused.
- `ZoomPane` toggle: render only the focused pane over the workspace area; tree unchanged.
- `SwapPane(direction)` — **should**.

Layout: recursive rect subdivision every time the window size or tree changes; results cached in `Workspace.layout: Vec<(PaneId, Rect)>` plus `dividers: Vec<Divider{rect, node}>` for hit-testing. Divider thickness 1 pt visual, 6 pt hit area.

## 3. Sidebar (D5)

### 3.1 Content

Vertical list of workspace cards for the window. Each card shows (design: `components.md §Sidebar`):

1. **Title** — workspace name.
2. **Status line** (optional) — latest attention message (e.g. "Claude is waiting for your input"), or running command of the focused tab if long-running (> 3 s), dimmed.
3. **Git line** (optional) — branch of focused tab's cwd + `*` if dirty.
4. **Paths** — distinct cwds of the workspace's tabs (max 3, `~`-abbreviated, middle-truncated), focused one first.
5. **Port chips** (optional) — TCP ports in LISTEN state owned by processes in the workspace's tab process trees (max 4, `+N` overflow). Click → open `http://localhost:<port>` in default browser.

### 3.2 Auto naming

When `name` is `None`: name = basename of focused tab's git repo root if inside a repo, else `~`-abbreviated cwd. User rename (double-click title or `RenameWorkspace`) sets a fixed name; empty string resets to auto.

### 3.3 Metadata collection (worker thread)

| Data | Source | Trigger | Cost control |
|---|---|---|---|
| cwd | OSC 7 (integration). Fallback: `proc_pidinfo(PROC_PIDVNODEPATHINFO)` of foreground pid | every OSC 7; fallback poll on command finish | — |
| git branch | read `<repo>/.git/HEAD` directly (walk up from cwd to find `.git`, handle `gitdir:` files for worktrees) | cwd change, command finish | cache per repo root, stat mtime of HEAD |
| git dirty | `git status --porcelain=v1 -uno -z` spawned on worker, 2 s timeout | command finish in that repo, ≤ once per 5 s per repo | skip if `.git/index` mtime unchanged since last check and no command ran |
| foreground process | `tcgetpgrp` + `proc_name` | on output burst end (debounced 300 ms) | main thread, cheap syscalls |
| listening ports | `proc_listchildpids` recursive from shell pid → `proc_pidinfo(PROC_PIDLISTFDS)` → `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)` for TCP LISTEN | every 5 s only for workspaces in visible windows' sidebars; immediately after command start + 2 s | stop when sidebar hidden / window occluded |

Results posted back to main via the reactor wake mechanism (`WorkerEvent` queue).

### 3.4 Interaction

- Click card → activate workspace. ⌘1–9 → workspace by index.
- Drag card → reorder; drag onto another window's sidebar → move workspace to that window (both are `Action::MoveWorkspace { to_window, index }`).
- Right-click → native context menu: Rename, Move to New Window, Close Workspace, Clear Attention.
- `+` button in sidebar header → `NewWorkspace` (cwd = `$HOME` or `workspace.default_cwd`).
- Sidebar width draggable (180–400 pt), toggled with ⌘B, persisted per window.

## 4. Tab strip (per pane)

- One strip at the top of each pane: tabs left-aligned, actions right-aligned: **New Tab** (`+`), **Split Right**, **Split Down** (icons), as in cmux.
- Tab label: title (user override > OSC title > foreground process name > shell name) + optional attention dot + close `×` on hover/active.
- Click activates; middle-click closes; double-click renames inline; drag to reorder within pane; drag to another pane's strip → move tab; drag onto a pane body edge (left/right/top/bottom 25 % zones, highlighted) → split that pane and move tab into new pane — **should** (Phase 4).
- Overflow: tabs shrink to min width, then strip scrolls horizontally (trackpad/shift-wheel) with fade edges.
- `⌘T` new tab in focused pane (cwd inherit), `⌘W` close tab, `⌘⇧]/[` next/prev tab, `⌃Tab` cycle.

## 5. Pane body

### 5.1 Classic view

The terminal grid fills the pane body (minus padding). Mouse: selection unless the app has mouse reporting on (hold ⌥ to force selection). Scrollback via wheel/trackpad; in alt screen with `?1007`, wheel sends arrow keys.

### 5.2 Block view (D4)

```
┌ pane body ─────────────────────────────────────────────┐
│  ┌ block ───────────────────────────────────────────┐   │
│  │ ~/proj  ·  git:main  ·  1.34s                ⋯   │ ◀ header (meta line)
│  │ cargo build --release                            │ ◀ command line (highlighted)
│  │ <output rows from grid>                          │   │
│  └──────────────────────────────────────────────────┘   │
│  ┌ block (running) ─────────────────────────────────┐   │
│  │ ...                                              │   │
│  └──────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────┤
│ [~/proj] [main] [✕ 1]                                   │ ◀ context chips
│ ▍git push origin ma█in                     ghost text   │ ◀ input editor
└─────────────────────────────────────────────────────────┘
```

- Rendering maps blocks to grid rows: header + command line are **chrome** drawn by the UI; rows in `[prompt_start, output_start)` (prompt + echoed command) are hidden; rows `[output_start, end)` are rendered as the block body.
- **Visual line map**: `Vec<Segment { block, kind: Header|Command|Rows(AbsLine range)|Gap, y: f32, h: f32 }>` built lazily for the scroll window, used for drawing, hit-testing and pixel↔row mapping. Rebuilt incrementally when blocks change.
- Scrolling is pixel-based (smooth trackpad) over the visual line map; auto-scroll to bottom while output streams unless the user scrolled up (then show "↓ New output" pill).
- Running block: header shows spinner + live elapsed time; the cursor is drawn inside the block's rows; raw keys go to PTY.
- Finished block: exit code ≠ 0 → failed style (`design-system.md` danger tint + left bar).
- **Sticky header**: when a block's header scrolls off the top while its body is visible, pin its header at the top of the pane.
- Block selection: click on header/gutter selects block (`↑/↓` move selection when editor empty and block selected; `⌘↑/⌘↓` jump blocks); `⌘C` with a selected block copies command + output; `⌘⇧C` copies output only; `⌘⌥C` copies command only. Hover shows header actions: copy, filter (Phase 4), `⋯` menu (Copy command, Copy output, Re-run, Share as text file → save dialog, Scroll to top of block).
- Text selection works across blocks as in classic (selection coordinates are grid `AbsPos`).
- `⌘K` clear: hides all finished blocks (Warp behavior) and sends nothing to the shell; in classic mode sends `ED 2` + `ED 3` equivalent locally (clears scrollback) and a form-feed `\x0c` only if configured.
- Empty state (new tab, before first prompt): no blocks; editor shown once the first `B` arrives. If no `A` within 3 s → classic mode (no integration).

### 5.3 Input editor

Specified in `input-editor.md`. Visible only in block mode at prompt (or disabled-running state). Height grows with content (1–12 lines) and shrinks pane rows accordingly — the PTY is resized only when the editor height changes **between commands**, not while typing, to avoid SIGWINCH storms. Implementation: PTY rows = pane body height minus editor max(current, last-submitted) height, debounced 150 ms.

### 5.4 Find (⌘F)

Overlay bar at the top-right of the focused pane: query field, match count `3/27`, prev/next, case toggle, close. Highlights all visible matches (`design` tokens), current match stronger. Uses incremental `Terminal::search_step`. Esc closes.

## 6. Focus and keyboard routing

Focus stack per window: `Overlay (palette/rename/find)` > `Pane(focused) → Tab → {Editor | Terminal}`.

Routing order for a `KeyEvent`:
1. If an overlay is open → overlay handles or swallows.
2. Keymap lookup (`Keymap` = user bindings over defaults, context-aware: `when = "editor" | "terminal" | "block_selected" | "always"`).
3. Block mode at prompt → editor.
4. Otherwise → `Terminal::encode_key` → PTY.

Focus follows mouse? `pane.focus_follows_mouse = false` default.

Unfocused panes: content dimmed by overlaying `bg.app` at `pane.inactive_dim` alpha (default 0.0 — off; tokens allow 0.25). Focused pane gets no border by default; attention ring overrides (§8).

## 7. Actions and keybindings

`Action` enum (serializable as `snake_case` strings for config, palette and IPC). Initial set:

```
new_window, close_window, new_workspace, close_workspace, rename_workspace, next_workspace, prev_workspace,
select_workspace(n), move_workspace(to_window, index), toggle_sidebar,
new_tab, close_tab, rename_tab, next_tab, prev_tab, select_tab(n), move_tab(to_pane, index),
split_right, split_down, close_pane, focus_pane(dir), resize_split(dir, amount), equalize_splits, zoom_pane,
copy, paste, select_all, clear_buffer, find, find_next, find_prev,
scroll_page_up, scroll_page_down, scroll_to_top, scroll_to_bottom, scroll_to_prev_block, scroll_to_next_block,
select_prev_block, select_next_block, copy_block, copy_block_output, copy_block_command, rerun_block,
toggle_block_mode, command_palette, goto_palette, history_search,
font_increase, font_decrease, font_reset, reload_config, open_config,
jump_to_attention, clear_attention, send_text(string), quit
```

Default keymap mirrors the menu in `platform-macos.md §11`, plus:
- `⌘↑ / ⌘↓` → select prev/next block (block mode) / scroll to prev/next prompt (classic with marks).
- `⌃R` → history search (editor context).
- `⌘⌥←/→/↑/↓` → focus pane; `⌘⌃←/→/↑/↓` → resize split.

Config syntax in `config.md §keybindings`.

## 8. Attention and notifications (D5)

```rust
struct Attention { unread: bool, kind: AttentionKind /* Bell | Notify | CommandDone | CommandFailed */, message: Option<Box<str>>, at: Instant }
```

Sources:
- `Effect::Bell` → kind Bell (respect `notifications.bell = "attention" | "sound" | "none"`).
- `Effect::Notify` (OSC 9/777/99) → kind Notify with message.
- Block finished with duration ≥ `notifications.long_command_secs` (default 10) while the tab is **not visible or window not key** → CommandDone/CommandFailed.

Propagation and display:
- Tab: dot on its tab chip.
- Pane: animated **attention ring** (2 pt border in `attention` color, pulse ×3 then steady) if the pane is visible but not focused.
- Workspace card: ring + status line message + unread count badge.
- Dock badge: count of tabs with unread attention.
- System notification (via platform) only if the app is not active or the tab is not visible.

Clearing: focusing the tab clears its attention (after 300 ms dwell to avoid clearing on pass-through). `⌘⇧U` jumps to the most recent unread attention across all windows.

## 9. Command palette (D4)

Two modes, same component:
- **⌘⇧P Actions**: all `Action`s with human titles + keybinding hints, plus dynamic entries ("Switch to workspace X", "Open recent directory …").
- **⌘P Go to**: workspaces, tabs (title + cwd), recent directories (from history), with fuzzy matching (`nucleo-matcher`). Enter switches; `⌘Enter` opens directory in a new tab.

Prefix shortcuts inside palette input: `>` actions, `@` tabs, `#` workspaces, `/` directories. Ranking = fuzzy score + recency boost (frecency store in persist).

## 10. Close confirmation

Closing a tab/pane/workspace/window/app with non-shell foreground processes shows a native sheet (`NSAlert` as sheet) listing processes. "Don't ask again" for a process name is stored in persist state (not in `config.toml`, which is user-authored and never written by the app).

## 11. Testing

- Model tests (pure Rust, no platform): split/close/focus navigation geometry, invariants after random action sequences (proptest), workspace move between windows, attention propagation and clearing.
- Layout snapshot tests: given window size + tree, assert pane rects and dividers.
- Visual line map tests: blocks with hidden rows, sticky header computation, pixel↔row mapping round-trip.
- UI DrawList snapshot tests (counts and key rects) for sidebar, tab strip, block view.
