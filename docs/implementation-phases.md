# Implementation Phases (parallel plan)

The work is split into **5 phases**. Within a phase, **tracks** run in parallel, one agent per track. Every track owns a disjoint set of files, builds against contracts frozen in the previous phase, and has its own acceptance criteria. A phase ends when all its tracks are merged to `main` and the phase gate passes.

```
Phase 0  Scaffold + contracts            ███ 1 agent
Phase 1  Foundations                     ████████ 8 agents in parallel (1A–1H)
Phase 2  Integration MVP                 ██ 2 agents in parallel (2A, 2B)  → Milestone M1 "it's a terminal"
Phase 3  Features                        █████ 5 agents in parallel (3A–3E)  → Milestone M2 "feature complete"
Phase 4  Hardening + ship                ████ 4 agents in parallel (4A–4D)  → Milestone M3 "v1.0"
```

Critical path: `0 → 1A + 1B + 1C + 1D + 1E → 2A → 3B → 4A`. Tracks 1F, 1G, 1H, 2B have slack.

## Working agreement for all agents

1. **Read first**: `AGENTS.md`, `docs/README.md`, `docs/architecture.md`, your track's spec docs (listed per track), and `docs/specs/performance.md §6`.
2. **Branch**: `track/<id>-<slug>` (e.g. `track/1A-vt-core`) from `main`; use a git worktree per agent. Open a PR into `main` when done.
3. **Stay in your lane**: edit only files your track owns (listed below). Need a change elsewhere? Write it in your PR description under "Requests for other tracks"; don't edit.
4. **Contracts are frozen** after Phase 0 (`architecture.md §3`, plus the Phase 2 model API). To change one: update the doc + code in a separate small PR titled `contract: …`, merged before dependent work. Additive changes (new methods with default impls, new enum variants on `#[non_exhaustive]` enums) are allowed inside a track PR.
5. **Definition of done** for every track: owned code compiles with `cargo clippy --workspace -- -D warnings`, `cargo test --workspace` passes, acceptance criteria below are met and demonstrated in the PR description (test names, bench numbers, screenshots for UI), no new dependency outside the allowlist, `cargo xtask size` delta reported.
6. **Stubs**: when depending on a track not yet merged, code against the Phase 0 stub. Never copy another track's unmerged code.

---

## Phase 0 — Scaffold and contracts (1 agent, sequential)

**Goal:** a compiling workspace where every crate exists, every cross-crate contract from `architecture.md §3` compiles with stub implementations, and CI is green. This is what makes Phase 1 parallel.

**Owns:** everything (repo is empty).

**Deliverables:**
- `git init`, `.gitignore`, `rust-toolchain.toml` (stable, pinned), root `Cargo.toml` with `[workspace]`, `[workspace.dependencies]` (allowlist versions pinned), profiles from `architecture.md §8`.
- All crates from `architecture.md §2` with `lib.rs`; `bin/termin/src/main.rs` prints version; `xtask/` with `size` command implemented (others stubbed).
- `termin-core` **fully implemented**: ids + allocator, geometry, `Rgba`, `Mods`, `Key`/`NamedKey`, `KeyEvent`, mouse/scroll types, `SmallStr`, `Slab<T>`, `WakeHandle`, `PtyConsumer` trait. With unit tests.
- Contract stubs (compiling, minimal behavior, `#[non_exhaustive]` where variants will grow):
  - `termin-vt`: `Terminal`, `TerminalConfig`, `EffectSink`, `Effect`, `BlockEvent`, `Modes`, `Damage`, `GridView`/`RowView`, `Cell`, `CellFlags`, `Style`, `StyleTable`, `BlockList`, `Block`, `AbsLine`, `AbsPos`, `Selection`, `SearchQuery` — types complete, logic stubbed.
  - `termin-pty`: `PtyConsumer`, `Reactor`, `ReactorHandle`, `Session`, `SpawnSpec`, `ReactorEvent`, `SessionState`.
  - `termin-platform`: `Platform`, `AppHandler`, `PlatformEvent`, `PlatformCx` method signatures, `WindowOpts`, `RenderTarget` trait, `MenuSpec`, `SystemNotification`, `CursorIcon`, `Appearance`.
  - `termin-text`: `FontBackend`, `FontSpec`, `FontId`, `FontMetrics`, `GlyphRef`, `ShapedGlyph`, `RasterInfo`, `GlyphCache` shell, `AtlasUpload`.
  - `termin-render`: `DrawList`, `Layer`, `QuadInst`, `GlyphInst` (with `size_of` asserts), `Renderer` trait.
  - `termin-editor`, `termin-config`, `termin-ipc`, `termin-persist`: public types from their specs (data structs + function signatures).
- Keep `AGENTS.md` / `CLAUDE.md` (already written) accurate as the scaffold lands; `budgets.toml` (values from `performance.md`), `.github/workflows/ci.yml` (fmt, clippy, test, size), `resources/` skeleton (`shell/`, `shaders/`, `specs/`, `themes/`, `icons/`).

**Acceptance:** CI green; `cargo doc --workspace` builds; `cargo xtask size` prints a table; every contract item listed above exists with doc comments linking to its spec section.

**Gate to Phase 1:** contracts reviewed by the product owner (one PR).

---

## Phase 1 — Foundations (8 parallel tracks)

### 1A — VT core
- **Spec:** `specs/terminal-core.md` (all).
- **Owns:** `crates/termin-vt/**`, `crates/termin-vt/tests/corpus/**`, `fuzz/fuzz_targets/fuzz_advance.rs`.
- **Deliverables:** `vte::Perform` impl; screens, cursor, modes, SGR, charsets, tab stops, scroll regions; StyleTable/GraphemeTable/HyperlinkTable with compaction; hot ring + cold chunks; reflow; OSC handling table; blocks model; selection text; incremental search; key + mouse encoding (legacy, modifyOtherKeys, kitty); damage + generation; sync output; `termin_core::PtyConsumer` impl for `Terminal`.
- **Acceptance:** golden corpus tests pass (vim, htop, ls --color, progress bars, CJK/emoji, Claude Code recording); proptest suite; fuzz target runs 10 min clean; criterion benches meet `performance.md §3` parse throughput; memory test: 10k lines × 120 cols ≤ 2.5 MiB.

### 1B — PTY, reactor, shell integration
- **Spec:** `specs/pty-and-shell.md`.
- **Owns:** `crates/termin-pty/**`, `crates/termin-shell/**`, `resources/shell/**`.
- **Deliverables:** spawn with env/argv rules; kqueue reactor with all filters listed; write queues; child exit; path watching; unix listener plumbing (bytes only, no protocol); zsh + bash integration scripts with ZDOTDIR / rcfile injection; scripts written to Application Support with version hash.
- **Acceptance:** integration tests: echo round-trip, exit code, `stty size` after resize, 500 MB throughput through a recording consumer without unbounded memory; shell tests assert `A B C D;0 … D;1` + OSC 7 for zsh and bash 3.2 (and 5.x if present); user `.zshrc` still loads with custom ZDOTDIR fixture.

### 1C — macOS platform
- **Spec:** `specs/platform-macos.md`.
- **Owns:** `crates/termin-platform/**`, `crates/termin-platform/examples/**`, `docs/qa/platform.md`.
- **Deliverables:** app delegate, window creation with transparent titlebar, `TerminView` with `CAMetalLayer`, full event translation, `NSTextInputClient`, option-as-alt, display link (CV + CADisplayLink path), `WakeHandle` via CFRunLoopSource, clipboard, menus from `MenuSpec`, context menus, file drop, cursor icons, notifications + dock badge, appearance, minimal accessibility hook points.
- **Acceptance:** `examples/event_log.rs` opens 2 windows, clears with different colors via raw Metal, logs every `PlatformEvent`; IME manual checklist in `docs/qa/platform.md` passes (Japanese, dead keys, emoji picker); display link stops when idle (verified with Instruments/`powermetrics` note in PR); wake from a background thread reaches `AppHandler::wake` during live resize and menu tracking.

### 1D — Text system
- **Spec:** `specs/text-and-rendering.md §2, §4`.
- **Owns:** `crates/termin-text/**`.
- **Deliverables:** CoreText `FontBackend` (resolution, faces, synthesis, fallback chain, metrics), ASCII fast path, ligature shaping + run cache, rasterization A8/BGRA, procedural glyphs (box, blocks, braille, powerline), shelf atlas packer with growth, `GlyphCache` producing `AtlasUpload`s, UI `TextLayout` with end/middle ellipsis.
- **Acceptance:** unit tests for metrics math, packer, procedural pixel fixtures; fallback finds glyphs for CJK, emoji, symbols; ligature test with a ligature font if installed (skipped otherwise); bench: 10k ASCII glyph lookups < 50 µs.

### 1E — Metal renderer
- **Spec:** `specs/text-and-rendering.md §3`.
- **Owns:** `crates/termin-render/**`, `resources/shaders/**`.
- **Deliverables:** device/queue/pipelines, build.rs metallib compile + runtime-source fallback, instance ring buffers with 2-frame semaphore, quad SDF shader (radius, border, shadow), glyph mask/color/icon pipelines with sRGB LUT + linear blending, layers with scissor, atlas texture management (create, grow via blit, upload), offscreen render path for tests, `release_idle_resources`.
- **Acceptance:** offscreen snapshot tests (rounded rect + border + shadow; glyph quads from a synthetic atlas; layered clipping) within tolerance; 100k quads frame encodes < 1 ms CPU; no per-frame allocation (alloc-count test).

### 1F — Editor logic, history, completions
- **Spec:** `specs/input-editor.md`.
- **Owns:** `crates/termin-editor/**`, `resources/specs/**`.
- **Deliverables:** `EditorState` with all editing commands + undo; tokenizer + highlight tokens; history store (Termin TSV + zsh/bash parsers) + prefix nav + autosuggest + fuzzy search API; completion engine with spec walker, built-in generators, async generator interface (returns jobs for the app's worker to run); build.rs spec compiler; the 25 v1 specs.
- **Acceptance:** golden completion tests, tokenizer tables, history parser tests, benches per spec (autosuggest < 0.5 ms over 50k, sync completion < 2 ms), compiled spec size ≤ 150 KiB.

### 1G — Config, themes, design tokens
- **Spec:** `specs/config.md`, `design/design-system.md`.
- **Owns:** `crates/termin-config/**`, `resources/themes/**`, `crates/termin-ui/src/theme/**`.
- **Deliverables:** schema + defaults + diagnostics, `diff`, theme loading + derivation rules, chord parser for keybindings (validated via injected action-name fn), `template()` commented config, `Tokens` struct in termin-ui with all tokens from design-system.md, built-in themes `termin-dark` / `termin-light`, contrast test.
- **Acceptance:** parse tests (valid, unknown keys, wrong types, partial themes), derivation produces tokens passing contrast checks, fuzz target `fuzz_config_parse` clean 5 min.

### 1H — IPC protocol, CLI client, persistence
- **Spec:** `specs/ipc-cli.md`, `specs/persistence.md`.
- **Owns:** `crates/termin-ipc/**`, `crates/termin-persist/**`, `bin/termin/src/cli.rs` (+ `bin/termin/src/main.rs` dispatch only: "subcommand → cli::run else gui").
- **Deliverables:** protocol types, sans-IO server codec, blocking client, full CLI with `--json`, single-instance detection helper; persist snapshot types, atomic save/load, state file, corruption handling.
- **Acceptance:** codec round-trip tests, CLI arg parsing tests (hand-written parser, no `clap` — size), client↔fake-server test over a real socket in a temp dir, snapshot round-trip and corrupted-file fallback tests.

**Phase 1 gate:** all 8 merged; `cargo test --workspace` green; size table recorded as Phase 1 baseline.

---

## Phase 2 — Integration MVP (2 parallel tracks)

### 2B — App model, actions, layout (pure logic) — start first, small
- **Spec:** `specs/app-model-and-ui.md §1, §2, §6, §7, §8 (data only)`.
- **Owns:** `crates/termin-app/src/model/**`, `crates/termin-app/src/action.rs`, `crates/termin-app/src/keymap.rs`, `crates/termin-app/src/dispatch.rs`, `crates/termin-app/src/controllers/mod.rs` (stub modules for 3A–3E).
- **Deliverables:** `Slab`-based model (windows, workspaces, panes, tabs), split tree ops, geometric focus navigation, layout computation (pane rects, dividers), complete `Action` enum (every v1 action), keymap with defaults + config overlay + `when` contexts, `dispatch.rs` routing actions to per-area controller modules (stub functions for Phase 3 areas), `WorkspaceMeta` and `Attention` data types, App↔`termin-persist::Snapshot` conversion.
- **Within first day:** publish the model API used by 2A (`App::focused_tab()`, `App::layout(window)`, etc.) as a `contract:` PR.
- **Acceptance:** proptest random action sequences keep invariants; layout snapshot tests; focus navigation tests; snapshot conversion round-trip.

### 2A — Wiring + classic terminal (critical path)
- **Spec:** `architecture.md §4`, `specs/app-model-and-ui.md §5.1`, `specs/text-and-rendering.md §3.4–3.5`.
- **Owns:** `crates/termin-app/src/{lib.rs, app.rs, run.rs, frame.rs, input.rs, session_ctl.rs}`, `crates/termin-ui/src/{lib.rs, draw/**, terminal_view/**, widgets/mod.rs, widgets/pane.rs, widgets/scrollbar.rs}`, `bin/termin/src/gui.rs`.
- **Deliverables:** `AppHandler` impl; reactor + session lifecycle; wake → effect drain → dirty marking; `PaneRenderCache` row builder with damage/scroll reuse; cursor (styles, blink), selection (mouse, word/line, copy), paste (bracketed), scrollback scroll (precise + wheel), mouse reporting, resize → PTY resize, title updates, bell, font zoom, basic single tab strip (one tab, static), config load at startup.
- **Acceptance (Milestone M1):** daily-drivable single-pane terminal: vim, nvim, htop, less, tmux, ssh, fzf, Claude Code render correctly; `cat` 100 MB meets throughput budget; idle CPU budget; launch-memory budget; key-to-PTY < 0.3 ms.

**Phase 2 gate:** M1 demo recorded (screen capture in PR), budgets met, model API frozen.

---

## Phase 3 — Features (5 parallel tracks)

Shared-file rule for Phase 3: `dispatch.rs`, `action.rs` and `model/**` are frozen (owned by 2B, now merged). Tracks implement their controller module (`controllers/<area>.rs`) and widgets. Additive model fields go through a `contract:` PR.

### 3A — Workspaces, splits, tabs, windows UI
- **Spec:** `app-model-and-ui.md §2–4`, `components.md §0–3, §13`.
- **Owns:** `termin-ui/src/widgets/{sidebar.rs, workspace_card.rs, tabstrip.rs, tab.rs, divider.rs, icon_button.rs, tooltip.rs}`, `termin-ui/src/icons/**` + `resources/icons/**` + `termin-ui/build.rs`, `termin-app/src/controllers/{workspaces.rs, panes.rs, tabs.rs, windows.rs}`.
- **Deliverables:** sidebar with cards (renders whatever `WorkspaceMeta` holds; metadata collection is 3D), rename inline, reorder drag, move workspace across windows, tab strip with all states + actions + drag reorder + move between panes, divider drag resize, keyboard split/focus/resize/equalize/zoom, multiple windows, close confirmation sheet, icon pipeline (SVG → path arrays → atlas).
- **Acceptance:** all states from components.md demonstrated in screenshots; split/close/zoom by keyboard and mouse; workspace switch ≤ 1 frame; per-pane memory budget.

### 3B — Block view and input editor UI (critical path)
- **Spec:** `app-model-and-ui.md §5.2–5.3`, `input-editor.md`, `pty-and-shell.md §4.3–4.4`, `components.md §4–7`.
- **Owns:** `termin-ui/src/block_view/**`, `termin-ui/src/widgets/{editor.rs, chips.rs, completion_menu.rs, history_search.rs, new_output_pill.rs}`, `termin-app/src/controllers/{blocks.rs, editor.rs, completion.rs}`.
- **Deliverables:** visual line map, block chrome (all states, sticky header), hidden prompt rows, pixel scrolling + autoscroll + new-output pill, block selection + copy actions + rerun, `⌘K` clear; editor rendering (syntax, ghost text, selection, caret, multi-line growth, chips); submit protocol (bracketed paste / bash 3.2), type-ahead, raw fallback after 2 s; completion menu with async merge (jobs run on the worker from 3D's worker module — until merged, run generators inline with a `// TODO(3D)` hook); history search panel; mode switching (classic ↔ block ↔ alt screen).
- **Acceptance:** zsh and bash sessions show correct blocks for success/failure/interrupted/`clear`; multi-line commands; vim from block mode returns cleanly; completion goldens visible in UI; frame budget with 1000 blocks in scrollback.

### 3C — Palette, find, banners, toasts
- **Spec:** `app-model-and-ui.md §5.4, §9`, `components.md §8–11`.
- **Owns:** `termin-ui/src/widgets/{palette.rs, keycaps.rs, find_bar.rs, banner.rs, toast.rs, scrim.rs}`, `termin-app/src/controllers/{palette.rs, find.rs, notices.rs}`, `termin-app/src/frecency.rs`.
- **Deliverables:** action palette + go-to palette with prefixes, fuzzy ranking + frecency, keycaps from keymap; find bar with incremental search + highlights; config-error banner; toasts.
- **Acceptance:** palette fuzzy over 10k items ≤ 3 ms; every `Action` reachable from palette; find across 100k-line scrollback stays responsive (no frame > 16 ms).

### 3D — Attention, notifications, sidebar metadata, worker
- **Spec:** `app-model-and-ui.md §3.3, §8`, `platform-macos.md §8`.
- **Owns:** `termin-app/src/worker.rs`, `termin-app/src/metadata/**` (cwd, git, ports via libproc, fg process), `termin-app/src/controllers/attention.rs`, `termin-ui/src/widgets/attention_ring.rs`.
- **Deliverables:** lazy worker thread with job queue (also serves 1F async generators and 3E persistence writes); metadata collectors with the triggers/cost controls in the table; `WorkspaceMeta` population; attention sources, propagation, clearing dwell, jump-to-attention, dock badge, system notifications with click-to-focus; attention ring animation (pulse ×3, reduced-motion aware).
- **Acceptance:** OSC 9/777/99 and BEL raise correct indicators; Claude Code "waiting for input" shows in card status line; ports of a `python -m http.server` appear within 5 s; no metadata polling while occluded (verified via log counters); worker exits after 30 s idle.

### 3E — IPC server, session restore, config hot reload
- **Spec:** `ipc-cli.md`, `persistence.md`, `config.md §4`.
- **Owns:** `termin-app/src/controllers/{ipc.rs, persist.rs, config.rs}`.
- **Deliverables:** IPC dispatch for every method incl. `subscribe` events and `command` param semantics; single-instance hand-off; session save triggers + restore on launch (cwd fallback, screen fallback); config hot reload applying `ConfigDiff` per subsystem; menu regeneration on keymap change.
- **Acceptance:** CLI script creating 3 workspaces with splits and commands works end-to-end; quit + relaunch restores identical layout; editing `config.toml` font size/theme/keybinding applies live without restart; malformed config shows banner and keeps old config.

**Phase 3 gate (Milestone M2):** full feature walkthrough per `docs/README.md` decision log; all budgets met.

---

## Phase 4 — Hardening and ship (4 parallel tracks)

### 4A — Performance and memory
- **Owns:** `xtask/src/{bench.rs, mem.rs, throughput.rs, corpus.rs}`, `benches/**` across crates (bench files only), `docs/qa/profiling.md`; may open targeted PRs in any crate **only** for measured perf fixes, each with before/after numbers.
- **Deliverables:** full `xtask mem`/`throughput` automation, alloc-count CI job, LZ4 cold-tier decision with data, profiling guide, fix list for every budget miss.
- **Acceptance:** every budget in `performance.md` met on the reference machine; CI gates active.

### 4B — Packaging and release
- **Owns:** `xtask/src/bundle.rs`, `resources/Info.plist`, `resources/Termin.icns`, `docs/release.md`, `.github/workflows/release.yml`.
- **Deliverables:** `.app` bundle assembly, arm64 + x86_64 + optional universal, codesign (hardened runtime) + notarization steps (documented; secrets supplied by owner), DMG or zip, version stamping, `termin` CLI symlink install action ("Install CLI in PATH" palette action → `/usr/local/bin/termin` via authorization prompt, or `~/.local/bin`).
- **Acceptance:** notarized build runs on a clean macOS 13 VM; zipped size within budget.

### 4C — Accessibility and polish
- **Owns:** `termin-platform/src/accessibility.rs`, plus Phase 4 items from `components.md §15` in their component files (coordinate with owners' merged code; no other track edits these files in Phase 4).
- **Deliverables:** VoiceOver reading of terminal output and sidebar/tab element tree; drag-tab-to-split zones; block output filter; scrollbar ticks; cwd chip directory menu; reduced-motion audit.
- **Acceptance:** VoiceOver can navigate workspaces/tabs and read the last command output; design QA checklist signed off.

### 4D — Conformance and compatibility
- **Owns:** `crates/termin-vt/tests/conformance/**`, `fuzz/**`, `docs/qa/compat-matrix.md`.
- **Deliverables:** vttest/esctest-derived conformance suite, nightly fuzzing, compatibility matrix (vim, nvim, emacs -nw, tmux, screen, htop, btop, less, man, git (pager + interactive add), fzf, lazygit, ssh, mosh, Claude Code, Codex CLI, Python REPL, node REPL) with issues filed for failures; fixes in `termin-vt` coordinated with 4A (4D owns `termin-vt` fixes in Phase 4; 4A must not touch `termin-vt` without 4D's review).
- **Acceptance:** all matrix rows pass or have a documented, accepted limitation.

**Phase 4 gate (Milestone M3 / v1.0):** release build published to the owner.

---

## File ownership summary

| Path | P0 | P1 | P2 | P3 | P4 |
|---|---|---|---|---|---|
| `termin-core` | 0 | frozen | frozen (contract PRs only) | frozen | frozen |
| `termin-vt` | 0 | 1A | bugfix by 2A via PR to 1A owner review | frozen | 4D |
| `termin-pty`, `termin-shell`, `resources/shell` | 0 | 1B | 2A (bugfix) | frozen | 4A (perf) |
| `termin-platform` | 0 | 1C | 2A (bugfix) | 3D (notifications only), 3A (sheets only) | 4C |
| `termin-text` | 0 | 1D | 2A (bugfix) | 3A (icons rasterization hook) | 4A |
| `termin-render` | 0 | 1E | 2A (bugfix) | frozen | 4A |
| `termin-editor`, `resources/specs` | 0 | 1F | — | 3B (bugfix) | 4A |
| `termin-config`, `resources/themes`, `termin-ui/src/theme` | 0 | 1G | — | 3E (bugfix) | — |
| `termin-ipc`, `termin-persist`, `bin/termin/src/cli.rs` | 0 | 1H | — | 3E (bugfix) | — |
| `termin-app/src/model`, `action.rs`, `keymap.rs`, `dispatch.rs` | 0 | — | 2B | frozen | — |
| `termin-app` core + `termin-ui` draw/terminal_view | 0 | — | 2A | frozen (bugfix via owner) | 4A |
| `termin-app/src/controllers/*`, `termin-ui/src/widgets/*` (per list) | — | — | — | 3A–3E as listed | 4C |
| `xtask` | 0 | — | — | — | 4A, 4B |

## Agent brief template

Use this when spawning an agent for a track:

```
You are implementing track <ID> — <name> — of Termin, a native macOS terminal in Rust.
Read, in order: AGENTS.md, docs/README.md, docs/architecture.md, <track spec docs>, docs/specs/performance.md §6.
Your track section in docs/implementation-phases.md is the source of truth for scope, owned files and acceptance criteria.
Work on branch track/<ID>-<slug> in your own git worktree. Edit only the files your track owns.
Code against the frozen contracts; if one blocks you, stop and describe the needed contract change instead of editing it.
Finish with: clippy clean, tests green, acceptance criteria demonstrated, `cargo xtask size` delta, and a PR description listing anything you need from other tracks.
```

## Milestones

| Milestone | After | Owner demo |
|---|---|---|
| M0 Contracts | Phase 0 | `cargo doc` contract walkthrough |
| M1 It's a terminal | Phase 2 | vim/htop/Claude Code in a single pane, budgets |
| M2 Feature complete | Phase 3 | workspaces, splits, tabs, blocks, editor, completions, palette, attention, CLI, restore, hot reload |
| M3 v1.0 | Phase 4 | notarized app, all budgets and compat matrix green |
