# Architecture

## 1. System overview

```
┌──────────────────────────── Main thread (AppKit run loop) ─────────────────────────────┐
│                                                                                         │
│  termin-platform (macOS)  ──PlatformEvent──▶  termin-app (App: model + controllers)      │
│    NSApplication / NSWindow                       │                                     │
│    TerminView (NSView + NSTextInputClient)        │ builds                              │
│    CAMetalLayer, CVDisplayLink ──on_frame──▶      ▼                                     │
│                                             termin-ui  (layout, widgets, hit-test)      │
│                                                   │ DrawList                            │
│                                                   ▼                                     │
│                                             termin-render (Metal)  ◀── termin-text       │
│                                                                    (CoreText, atlas)    │
│        ▲ wake (CFRunLoopSource)                                                         │
└────────┼────────────────────────────────────────────────────────────────────────────────┘
         │                         Arc<Session> { Mutex<Terminal>, effects queue, writer }
┌────────┴──────── IO thread (kqueue reactor, 1 for all tabs) ────────┐
│ termin-pty: read PTY fds ─▶ lock Terminal ─▶ termin-vt::advance()     │
│             write queues, child exit (EVFILT_PROC), config file watch │
│             (EVFILT_VNODE), IPC socket accept/read                    │
└──────────────────────────────────────────────────────────────────────┘
┌──── Worker thread (lazy, parks when idle, exits after 30 s idle) ────┐
│ git status, port scan (libproc), completion generators, history load  │
└──────────────────────────────────────────────────────────────────────┘
```

Exactly **two permanent threads** (main + IO) plus one **lazy worker**. No async runtime, no thread-per-tab. Every additional thread costs ~512 KiB of stack reservation plus scheduler overhead; we don't pay that per tab.

## 2. Crate layout (Cargo workspace)

```
termin/
├── Cargo.toml                # [workspace], shared [profile.*], [workspace.dependencies]
├── crates/
│   ├── termin-core/          # ids, geometry, color, input event types, small utils. no deps but bitflags
│   ├── termin-vt/            # Terminal state machine: grid, scrollback, modes, OSC, blocks. pure, no IO
│   ├── termin-pty/           # PTY open/spawn, kqueue reactor, Session, writer queues
│   ├── termin-shell/         # embedded integration scripts + env/argv builder for zsh/bash
│   ├── termin-text/          # FontBackend trait + CoreText impl, glyph cache, atlas packer, box-drawing
│   ├── termin-render/        # Renderer trait + Metal impl, DrawList, pipelines, shaders
│   ├── termin-platform/      # Platform trait + macOS impl (AppKit/objc2)
│   ├── termin-editor/        # input editor buffer, shell tokenizer, history, completion engine, specs
│   ├── termin-config/        # TOML schema, defaults, theme files, validation, diffs
│   ├── termin-ui/            # theme tokens, layout, widgets, hit-testing, DrawList building
│   ├── termin-ipc/           # JSON-lines protocol types, server state machine, client
│   ├── termin-persist/       # session snapshot model + load/save
│   └── termin-app/           # App: model (windows/workspaces/tree/tabs), actions, controllers, wiring
├── bin/termin/               # main.rs: GUI launch or CLI client (same binary)
├── xtask/                    # bundle, size report, bench runner, metallib build helper (not shipped)
├── resources/                # Info.plist template, icon .icns, shaders (.metal), completion specs (.toml)
└── docs/
```

Dependency direction (no cycles; arrows mean "depends on"):

```
termin-core ◀── everything
termin-vt      ◀── termin-pty, termin-ui, termin-app
termin-text    ◀── termin-render, termin-ui
termin-render  ◀── termin-ui, termin-app
termin-platform◀── termin-app
termin-editor  ◀── termin-ui, termin-app
termin-config  ◀── termin-ui, termin-app
termin-shell   ◀── termin-pty  (only for argv/env building)
termin-ipc, termin-persist ◀── termin-app
termin-app     ◀── bin/termin
```

Rules:
- `termin-vt`, `termin-editor`, `termin-config`, `termin-ipc` (protocol), `termin-persist`, `termin-core` must compile and test on **any** Unix without macOS frameworks. This keeps them fuzzable/testable in CI and ready for Linux.
- All `objc2*` usage lives in `termin-platform`, `termin-text/src/coretext`, `termin-render/src/metal`. Nothing else may `use objc2`.
- Platform-specific modules are gated `#[cfg(target_os = "macos")]` with a trait in the crate root.

## 3. Cross-crate contracts

These signatures are created in **Phase 0** (see `implementation-phases.md`) with stub impls so every track can build against them. Changing a contract after Phase 0 requires updating this doc and notifying all tracks.

### 3.1 termin-core

```rust
// Ids: u32 newtypes, allocated from a global monotonically increasing counter. Never reused within a process.
pub struct WindowId(pub u32);
pub struct WorkspaceId(pub u32);
pub struct PaneId(pub u32);
pub struct TabId(pub u32);      // == session id
pub struct BlockId(pub u32);    // unique per tab

// Geometry in logical points (f32). Physical = logical * scale_factor.
pub struct Point { pub x: f32, pub y: f32 }
pub struct Size  { pub w: f32, pub h: f32 }
pub struct Rect  { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }

// Packed RGBA8, straight alpha. 4 bytes.
pub struct Rgba(pub u32);

bitflags! { pub struct Mods: u8 { SHIFT, CTRL, ALT, CMD, CAPS, FN } }

pub enum Key { Char(char), Named(NamedKey) }           // NamedKey: Enter, Tab, Backspace, Escape, arrows, F1..F20, Home, End, PageUp, PageDown, Insert, Delete, ...
pub struct KeyEvent {
    pub key: Key,                 // logical key, layout-aware
    pub physical: u16,            // platform keycode (for kitty protocol / keybinding by position)
    pub mods: Mods,
    pub text: Option<SmallStr>,   // text produced (after dead keys), None for non-text keys
    pub repeat: bool,
    pub pressed: bool,            // false = release (only used by kitty keyboard protocol)
}
pub enum MouseButton { Left, Right, Middle, Other(u8) }
pub struct MouseEvent { pub kind: MouseKind, pub pos: Point, pub mods: Mods, pub click_count: u8 }
pub enum MouseKind { Down(MouseButton), Up(MouseButton), Move, Drag(MouseButton) }
pub struct ScrollEvent { pub pos: Point, pub dx: f32, pub dy: f32, pub precise: bool, pub phase: ScrollPhase, pub mods: Mods }

pub struct SmallStr; // inline up to 22 bytes, else boxed. Own impl (no smol_str dep).
```

### 3.2 termin-vt

```rust
pub struct TerminalConfig { pub scrollback_lines: u32, pub cols: u16, pub rows: u16, pub cell_px: (u16, u16), pub osc52: Osc52Policy, ... }

pub struct Terminal { /* see specs/terminal-core.md */ }

impl Terminal {
    pub fn new(cfg: &TerminalConfig) -> Self;
    /// Parse bytes from the PTY. Side effects are pushed to `sink`.
    pub fn advance(&mut self, bytes: &[u8], sink: &mut impl EffectSink);
    pub fn resize(&mut self, cols: u16, rows: u16, cell_px: (u16, u16));
    pub fn generation(&self) -> u64;             // bumps on any visible change
    pub fn damage(&self) -> &Damage;             // dirty visible rows + cursor
    pub fn reset_damage(&mut self);
    pub fn view(&self, scroll: ScrollPos) -> GridView<'_>;   // iterate visible rows
    pub fn modes(&self) -> Modes;                // alt screen, mouse mode, bracketed paste, kitty kbd flags, ...
    pub fn cursor(&self) -> CursorState;
    pub fn blocks(&self) -> &BlockList;
    pub fn styles(&self) -> &StyleTable;
    pub fn text_range(&self, range: LineRange) -> String;
    pub fn search(&self, query: &SearchQuery, from: AbsPos) -> Option<Match>;
    pub fn encode_key(&self, ev: &KeyEvent) -> SmallVec<[u8; 16]>;   // respects DECCKM, kitty flags, etc.
    pub fn encode_mouse(&self, ev: &CellMouseEvent) -> Option<SmallVec<[u8; 16]>>;
}

pub trait EffectSink {
    fn write_back(&mut self, bytes: &[u8]);      // DA/DSR/OSC query replies → PTY
    fn effect(&mut self, e: Effect);
}
pub enum Effect {
    Title(Box<str>), Bell, Notify { title: Box<str>, body: Box<str> },
    Cwd(Box<str>), ClipboardSet(Box<[u8]>), ClipboardRequest,
    Block(BlockEvent), CursorShape(CursorShape), Hyperlink, ModeChanged(ModeFlags),
    ColorQuery(..), // answered through write_back by Terminal itself when palette known
}
```

### 3.3 termin-pty

```rust
/// What a session feeds PTY output into. Defined in termin-core (re-exported by termin-pty).
/// `termin_vt::Terminal` implements it; tests use a recording stub, so termin-pty and termin-vt
/// never depend on each other.
pub trait PtyConsumer: Send + 'static {
    type Effect: Send;
    fn consume(&mut self, bytes: &[u8], write_back: &mut Vec<u8>, effects: &mut Vec<Self::Effect>);
    fn resize(&mut self, cols: u16, rows: u16, px: (u16, u16));
    fn generation(&self) -> u64;
}

pub struct Reactor<C: PtyConsumer>; // owns IO thread
impl<C: PtyConsumer> Reactor<C> {
    pub fn start(wake: WakeHandle) -> io::Result<ReactorHandle<C>>;
}
pub struct ReactorHandle<C: PtyConsumer>; // Clone + Send
impl<C: PtyConsumer> ReactorHandle<C> {
    pub fn spawn(&self, spec: SpawnSpec, consumer: C) -> io::Result<Arc<Session<C>>>;
    pub fn watch_path(&self, path: &Path, token: WatchToken);        // config hot reload
    pub fn listen_unix(&self, path: &Path) -> io::Result<()>;        // IPC
    pub fn shutdown(&self);
}
pub struct Session<C: PtyConsumer> {
    pub id: TabId,
    pub term: Mutex<C>,                      // the app uses Session<termin_vt::Terminal>
    pub fn write(&self, bytes: &[u8]);       // enqueue; reactor flushes
    pub fn resize(&self, cols: u16, rows: u16, px: (u16, u16)); // TIOCSWINSZ + consumer.resize
    pub fn drain_effects(&self, out: &mut Vec<C::Effect>);
    pub fn pid(&self) -> Pid; pub fn foreground_pid(&self) -> Option<Pid>;   // tcgetpgrp
    pub fn kill(&self, sig: Signal);
    pub fn state(&self) -> SessionState;     // Running | Exited(code)
}
pub enum ReactorEvent { SessionOutput(TabId), SessionExited(TabId, i32), PathChanged(WatchToken), Ipc(IpcConnId, Vec<u8>) }
```

### 3.4 termin-platform

```rust
pub trait Platform: Sized {
    fn run<H: AppHandler>(handler: H) -> !;
}
pub trait AppHandler: 'static {
    fn init(&mut self, cx: &mut PlatformCx);
    fn event(&mut self, cx: &mut PlatformCx, win: WindowId, ev: PlatformEvent);
    fn frame(&mut self, cx: &mut PlatformCx, win: WindowId, target: &mut dyn RenderTarget);
    fn wake(&mut self, cx: &mut PlatformCx);           // IO thread signalled
    fn menu_action(&mut self, cx: &mut PlatformCx, action: ActionId);
    fn should_terminate(&mut self, cx: &mut PlatformCx) -> bool;
}
pub enum PlatformEvent {
    Key(KeyEvent), ImeCompose { text: SmallStr, cursor: Option<Range<usize>> }, ImeCommit(String),
    Mouse(MouseEvent), Scroll(ScrollEvent), Resized { size: Size, scale: f32 },
    Focus(bool), Occluded(bool), CloseRequested, FileDrop(Vec<PathBuf>), Appearance(Appearance),
}
pub struct PlatformCx { /* main-thread only */ }
impl PlatformCx {
    pub fn create_window(&mut self, opts: &WindowOpts) -> WindowId;
    pub fn close_window(&mut self, w: WindowId);
    pub fn request_frame(&mut self, w: WindowId);            // starts display link until idle
    pub fn set_cursor(&mut self, w: WindowId, c: CursorIcon);
    pub fn set_ime_rect(&mut self, w: WindowId, r: Rect);
    pub fn clipboard_get(&self) -> Option<String>; pub fn clipboard_set(&mut self, s: &str);
    pub fn notify(&mut self, n: &SystemNotification); pub fn set_badge(&mut self, n: u32);
    pub fn open_url(&self, url: &str); pub fn reveal_in_finder(&self, p: &Path);
    pub fn set_menu(&mut self, menu: &MenuSpec);
    pub fn wake_handle(&self) -> WakeHandle;                 // Send + Clone; calls AppHandler::wake on main
    pub fn appearance(&self) -> Appearance;
}
```

### 3.5 termin-text / termin-render

```rust
// termin-text
pub trait FontBackend {
    fn resolve(&mut self, spec: &FontSpec) -> FontId;                  // family, size, weight, style
    fn metrics(&self, f: FontId) -> FontMetrics;                       // cell w/h, ascent, descent, underline pos/thickness
    fn glyph(&mut self, f: FontId, c: char) -> GlyphRef;               // with fallback chain
    fn shape(&mut self, f: FontId, text: &str, ligatures: bool, out: &mut Vec<ShapedGlyph>);
    fn rasterize(&mut self, g: GlyphRef, scale: f32, out: &mut RasterBuf) -> RasterInfo; // Mask(A8) | Color(BGRA8)
}
pub struct GlyphCache<B: FontBackend> { /* key → AtlasSlot, produces upload list */ }

// termin-render
pub struct DrawList { pub layers: Vec<Layer> }
pub struct Layer { pub clip: Option<Rect>, pub quads: Vec<QuadInst>, pub glyphs: Vec<GlyphInst>, pub color_glyphs: Vec<GlyphInst>, pub icons: Vec<GlyphInst> }
#[repr(C)] pub struct QuadInst { pub rect: [f32; 4], pub color: u32, pub border_color: u32, pub radius: f32, pub border: f32, pub shadow: f32, pub _pad: f32 }   // 40 B
#[repr(C)] pub struct GlyphInst { pub pos: [f32; 2], pub size: [u16; 2], pub uv: [u16; 4], pub color: u32 }             // 28 B
pub trait Renderer {
    fn upload_glyphs(&mut self, uploads: &[AtlasUpload]);
    fn render(&mut self, target: &mut dyn RenderTarget, list: &DrawList, clear: Rgba);
    fn release_idle_resources(&mut self);
}
```

The reactor stores sessions of one concrete type; the app instantiates it once as `Reactor::<termin_vt::Terminal>::start`.

`WakeHandle` is defined in `termin-core` (a `Send + Clone` wrapper around `Arc<dyn Fn() + Send + Sync>`), so `termin-pty` does not depend on `termin-platform`.

### 3.6 termin-editor, termin-config, termin-ipc, termin-persist

Defined in their spec docs. Each exposes plain data types + pure functions; no threads, no globals.

## 4. Data flow

### 4.1 Output path (PTY → pixels)

1. Reactor `kevent` returns readable PTY fd.
2. Read into a reusable 64 KiB buffer (one per reactor, not per session).
3. Lock `session.term`, call `advance(bytes, &mut sink)`. Budget: release lock after **≤ 1 MiB parsed or ≤ 4 ms**, whichever first, then continue on next loop iteration (prevents starving the renderer during `cat huge.log`).
4. `sink.write_back` bytes are appended to the session write queue; `effect`s go to the session effect queue.
5. If generation changed or effects present and the session isn't already marked pending: set `pending` atomic, call `wake_handle.wake()` (coalesced: one wake per main-loop turn, not per read).
6. Main thread `AppHandler::wake`: for each pending session, drain effects (title, notify, blocks, cwd…) into the app model; mark the owning pane dirty; `request_frame(window)`.
7. On frame: for each **visible, dirty** pane, lock its `Terminal` briefly and copy damaged rows into that pane's `PaneRenderCache` (resolved glyph ids + colors per row). Unlock. Build the DrawList from caches + UI chrome. Render.

Lock hold time on main is bounded by `rows × cols` copies of dirty rows only.

### 4.2 Input path (keys → PTY)

1. `TerminView::keyDown` → `interpretKeyEvents` (IME) → `PlatformEvent::Key`/`ImeCommit`.
2. App resolves keybinding table (user config → defaults). Bound → dispatch `Action`. Unbound:
   - Tab in **block mode at prompt** → input editor handles it.
   - Otherwise → `Terminal::encode_key` → `Session::write`.
3. Writes go into the session write queue; reactor is woken via an `EVFILT_USER` event and flushes non-blocking.

### 4.3 Idle

No timers run when idle except: cursor blink (only for the focused pane, only if blinking enabled, paused when window unfocused/occluded), and worker polling of sidebar metadata (≥ 5 s, only while sidebar visible and window not occluded). Display link is stopped when no frame was requested for 2 consecutive vsyncs.

## 5. App state ownership

All UI and model state is owned by `App` on the main thread. No `Rc<RefCell>` graphs: entities live in **slot maps** (`Vec<Option<T>>` + free list, own impl in `termin-core::slab`) keyed by the id newtypes. Cross-references are ids, never pointers.

```rust
struct App {
    windows: Slab<WindowState>,        // order of workspaces, active workspace, sidebar width
    workspaces: Slab<Workspace>,       // name, split tree, focused pane, attention, metadata
    panes: Slab<Pane>,                 // tabs: SmallVec<[TabId; 4]>, active tab, render cache
    tabs: Slab<Tab>,                   // Arc<Session>, title, editor state, block-view state, attention
    config: Arc<Config>, theme: Theme, keymap: Keymap,
    reactor: ReactorHandle, worker: WorkerHandle,
    text: GlyphCache<CoreTextBackend>, renderer: MetalRenderer,
    ipc: IpcServerState, persist: PersistState,
}
```

Actions (`termin-app::action::Action` enum) are the single mutation entry point for user input, menu items, palette commands, keybindings, and IPC requests. This guarantees every feature is scriptable and bindable.

## 6. Error handling

- Library crates return `Result<T, crate::Error>` with small enum errors (no `anyhow`, no `thiserror`; hand-written `Display`).
- The app never panics on bad input: VT parser treats malformed sequences as no-ops; config errors show a banner and keep the last good config.
- `panic = "abort"` in release. A panic hook writes a crash note to `~/Library/Logs/Termin/crash-<ts>.txt` before abort.
- Child process failure to spawn → tab shows an inline error block with the OS error string and a "Retry" action.

## 7. Dependency policy

Every runtime dependency must be on this allowlist. Adding one requires a size measurement (`cargo xtask size` before/after) recorded in the PR description.

| Crate | Why | Notes |
|---|---|---|
| `objc2`, `objc2-foundation`, `objc2-app-kit`, `objc2-quartz-core`, `objc2-metal`, `objc2-core-foundation`, `objc2-core-text`, `objc2-core-graphics`, `objc2-user-notifications`, `block2` | macOS APIs | Enable only needed features (they are feature-gated per class) |
| `vte` | VT parser state machine | Parser only; we implement `Perform` |
| `rustix` | PTY, kqueue, termios, signals, sockets | features: `pty`, `event`, `termios`, `process`, `net`, `fs` |
| `bitflags` | flag sets | |
| `smallvec` | inline small vectors | `union`, `const_generics` |
| `rustc-hash` | FxHashMap for hot maps | |
| `unicode-width`, `unicode-segmentation` | cell widths, graphemes | |
| `memchr` | fast scanning (search, parsing ASCII runs) | |
| `serde` (derive), `toml` (`parse` only), `serde_json` | config, IPC, session files | `toml` with `default-features = false, features = ["parse", "serde"]` |
| `nucleo-matcher` | fuzzy matching for palette/history/completions | matcher only, not `nucleo` |
| `log` | logging facade | `release_max_level_info`; tiny own file logger |

Dev-only (not shipped): `criterion`, `proptest`, `insta` (optional), `cargo-fuzz` targets.

Explicitly **banned**: `tokio`/`async-std`/any async runtime, `winit`, `wgpu`, `tracing`, `anyhow`, `regex` (use `memchr` + own simple matching; regex search may be reconsidered later behind a feature), `image`, `reqwest`, `chrono` (use own tiny time formatting), `notify` (use kqueue directly), `crossbeam` (std mpsc + atomics suffice), `parking_lot` (std Mutex is futex/os_unfair_lock-based and small).

## 8. Build profile

```toml
[profile.release]
opt-level = 3            # hot paths need speed; size controlled via deps + LTO
lto = "fat"
codegen-units = 1
panic = "abort"
strip = "symbols"
debug = false
incremental = false

[profile.release.package."*"]
opt-level = 3

[profile.release-size]    # experiment profile; CI compares both
inherits = "release"
opt-level = "s"

[profile.profiling]
inherits = "release"
debug = "line-tables-only"
strip = "none"
```

- Targets: `aarch64-apple-darwin` and `x86_64-apple-darwin` built separately; universal binary via `lipo` only for the distributed `.app` (opt-in; arm64-only build is the size reference).
- `MACOSX_DEPLOYMENT_TARGET=13.0`.
- Shaders precompiled to `.metallib` at build time by `termin-render/build.rs` (`xcrun metal` + `xcrun metallib`) and embedded with `include_bytes!`. If the Metal toolchain is unavailable, build.rs embeds the MSL source and the renderer compiles it at startup via `newLibraryWithSource` (logged as a warning).
- Completion specs (`resources/specs/*.toml`) are compiled by `termin-editor/build.rs` into static Rust tables — zero parse cost at runtime, no TOML in the hot path.

## 9. Platform portability seams

| Seam | macOS v1 | Future Linux | Future Windows |
|---|---|---|---|
| `Platform` | AppKit | Wayland (+X11) | Win32 |
| `Renderer` | Metal | Vulkan or GL ES 3 | D3D11 |
| `FontBackend` | CoreText | FreeType + HarfBuzz + fontconfig | DirectWrite |
| PTY | `rustix::pty` + kqueue | same + epoll | ConPTY + IOCP |
| Metadata (cwd, ports, fg process) | `libproc` | `/proc` | Toolhelp + NtQuery |
| Notifications | `UNUserNotificationCenter` | D-Bus `org.freedesktop.Notifications` | Toast |

The reactor uses a tiny internal `Poller` trait (kqueue now, epoll later) so `termin-pty` stays portable.
