//! Main application state and `AppHandler` implementation (`termin-app::app`).
//!
//! See `docs/architecture.md §5` and `docs/specs/platform-macos.md`.

use rustc_hash::FxHashMap;
use std::sync::Arc;
use termin_config::{parse_theme, Config, BUILTIN_TERMIN_DARK};
use termin_core::{PaneId, Rect, Size, TabId, WindowId};
use termin_platform::{ActionId, AppHandler, PlatformCx, PlatformEvent, RenderTarget, WindowOpts};
use termin_pty::{Reactor, ReactorEvent, ReactorHandle, Session};
use termin_render::MetalRenderer;
use termin_text::{CoreTextBackend, FontBackend, FontSpec, GlyphCache};
use termin_ui::{PaneRenderCache, RowInstances, TermFonts, TermPalette, Tokens};
use termin_vt::Terminal;

use crate::block_mode::{BlockMode, KeyOutcome};
use crate::frame::render_window_frame;
use crate::input::{handle_ime_commit, handle_key, handle_scroll, handle_window_mouse};
use crate::keymap::Keymap;
use crate::model::{AppModel, ModelEffect};
use crate::session_ctl::{close_session, drain_session_effects, spawn_session};

/// Main application state owned on the main thread.
pub struct App {
    pub model: AppModel,
    pub config: Arc<Config>,
    pub keymap: Keymap,
    pub reactor: Option<ReactorHandle<Terminal>>,
    pub renderer: Option<MetalRenderer>,
    pub glyphs: GlyphCache<CoreTextBackend>,
    pub fonts: TermFonts,
    pub palette: TermPalette,
    pub tokens: Tokens,
    pub pane_caches: FxHashMap<PaneId, PaneRenderCache>,
    /// Last PTY winsize/reflow time per pane (live-resize throttle).
    pub resize_times: FxHashMap<PaneId, std::time::Instant>,
    pub scroll_remainders: FxHashMap<PaneId, f32>,
    /// Session handles keyed by model tab id, retained after model removal until exit.
    pub session_handles: FxHashMap<TabId, Arc<Session<Terminal>>>,
    /// Current owning window for each live or closing model tab.
    session_windows: FxHashMap<TabId, WindowId>,
    /// Closed sessions retained while their HUP/escalation is observed by the reactor.
    retired_sessions: FxHashMap<TabId, Arc<Session<Terminal>>>,
    pub cursor_blink_visible: bool,
    pub font_size: f32,
    /// Warp-style block mode state (editor, history, scroll) per tab.
    pub block_mode: BlockMode,
    block_scratch: RowInstances,
    /// Last drag regions sent to the platform per window, plus a scratch buffer.
    drag_regions: FxHashMap<WindowId, Vec<Rect>>,
    drag_scratch: Vec<Rect>,
}

/// Focused tab in block mode, resolved for one input event.
struct BlockTarget {
    tab: TabId,
    cwd: String,
    modes: termin_vt::Modes,
    session: Arc<Session<Terminal>>,
}

impl Default for App {
    fn default() -> Self {
        Self::new(Arc::new(Config::default()))
    }
}

impl App {
    /// Create a new application instance with loaded config.
    pub fn new(config: Arc<Config>) -> Self {
        let keymap = Keymap::from_config(&config);
        let mut backend = CoreTextBackend::new();
        let font_size = config.font.size;

        let font_spec = FontSpec {
            family: if config.font.family.is_empty() {
                None
            } else {
                Some(config.font.family.clone())
            },
            size_pt: font_size,
            weight: termin_text::FontWeight::Regular,
            italic: false,
        };
        let font_id = backend.resolve(&font_spec);
        let metrics = backend.metrics(font_id);
        let fonts = TermFonts::new(font_id, &metrics, 2.0);

        let glyphs = GlyphCache::new(backend);
        let theme = parse_theme(BUILTIN_TERMIN_DARK).unwrap_or_default();
        let palette = TermPalette::from_theme(&theme.terminal, config.font.bold_is_bright);
        let tokens = Tokens::from_theme(&theme);
        let block_mode = BlockMode::new(config.blocks.enabled);

        Self {
            model: AppModel::new(),
            config,
            keymap,
            reactor: None,
            renderer: None,
            glyphs,
            fonts,
            palette,
            tokens,
            pane_caches: FxHashMap::default(),
            resize_times: FxHashMap::default(),
            scroll_remainders: FxHashMap::default(),
            session_handles: FxHashMap::default(),
            session_windows: FxHashMap::default(),
            retired_sessions: FxHashMap::default(),
            cursor_blink_visible: true,
            font_size,
            block_mode,
            block_scratch: RowInstances::default(),
            drag_regions: FxHashMap::default(),
            drag_scratch: Vec::new(),
        }
    }

    /// Process any pending model effects (e.g. spawn PTY sessions).
    pub fn process_model_effects(&mut self, cx: &mut PlatformCx) {
        self.process_model_effects_with(|window| cx.close_window(window));
    }

    fn process_model_effects_with(&mut self, mut close_window: impl FnMut(WindowId)) {
        self.refresh_session_windows();
        let effects = self.model.drain_effects();
        for eff in effects {
            match eff {
                ModelEffect::SpawnTab { tab, pane, cwd: _ } => {
                    if let Some(reactor) = self.reactor.as_ref() {
                        if let Some(tab_entity) = self.model.tabs.get_mut(tab) {
                            let ws_id = self
                                .model
                                .panes
                                .get(pane)
                                .map(|p| p.workspace)
                                .unwrap_or(termin_core::WorkspaceId(0));

                            match spawn_session(
                                tab_entity,
                                pane,
                                ws_id,
                                reactor,
                                &self.config,
                                80,
                                24,
                            ) {
                                Ok(session) => {
                                    if let Some(window) = window_for_tab(&self.model, tab) {
                                        self.session_windows.insert(tab, window);
                                    }
                                    self.session_handles.insert(tab, session);
                                }
                                Err(error) => {
                                    log::error!("Failed to spawn terminal session: {error}")
                                }
                            }
                        }
                    }
                }
                ModelEffect::CloseTab { tab } => self.close_tab_session(tab),
                ModelEffect::CloseWindow { window } => {
                    self.close_window_sessions(window);
                    close_window(window);
                }
            }
        }
    }

    fn refresh_session_windows(&mut self) {
        for (tab_id, tab) in self.model.tabs.iter() {
            if tab.session.is_some() {
                if let Some(window) = window_for_tab(&self.model, tab_id) {
                    self.session_windows.insert(tab_id, window);
                }
            }
        }
    }

    fn close_tab_session(&mut self, tab_id: TabId) {
        self.session_windows.remove(&tab_id);
        if let Some(session) = self.session_handles.remove(&tab_id) {
            self.terminate_session(session);
        }
    }

    fn close_window_sessions(&mut self, window: WindowId) {
        let tabs: Vec<_> = self
            .session_windows
            .iter()
            .filter_map(|(&tab, &owner)| (owner == window).then_some(tab))
            .collect();
        for tab in tabs {
            self.close_tab_session(tab);
        }
    }

    fn terminate_session(&mut self, session: Arc<Session<Terminal>>) {
        if close_session(&session) {
            self.retired_sessions.insert(session.id, session);
        }
    }

    /// Update font size (zoom in / zoom out / reset).
    /// The focused tab, when it is rendering in block mode.
    fn focused_block_target(&self) -> Option<BlockTarget> {
        let tab_id = self.model.focused_tab()?;
        let tab = self.model.tabs.get(tab_id)?;
        let session = tab.session.clone()?;
        let modes = {
            let term = session.term.lock().ok()?;
            if !self.block_mode.is_active(tab, &term) {
                return None;
            }
            term.modes()
        };
        let cwd = tab
            .cwd
            .clone()
            .or_else(|| std::env::var("HOME").ok())
            .unwrap_or_else(|| "/".into());
        Some(BlockTarget {
            tab: tab_id,
            cwd,
            modes,
            session,
        })
    }

    /// Panes whose active tab renders in block mode (terminal mouse input is off there).
    fn block_panes(&self) -> Vec<PaneId> {
        self.model
            .panes
            .iter()
            .filter_map(|(pane_id, pane)| {
                let tab = self.model.tabs.get(pane.active_tab)?;
                let session = tab.session.as_ref()?;
                let term = session.term.lock().ok()?;
                self.block_mode.is_active(tab, &term).then_some(pane_id)
            })
            .collect()
    }

    /// Offer a key to the block editor. `None` lets the classic path handle it.
    fn block_key(&mut self, ev: &termin_core::KeyEvent, cx: &mut PlatformCx) -> Option<bool> {
        let target = self.focused_block_target()?;
        let editor_owns_input = self
            .block_mode
            .tabs
            .get(&target.tab)
            .is_some_and(|t| t.ctl.route() == crate::controllers::editor::InputRoute::Editor);
        if ev.pressed && editor_owns_input {
            match self.keymap.lookup(ev, crate::keymap::WhenContext::Editor) {
                Some(crate::action::Action::Paste) => {
                    let text = cx.clipboard_get()?;
                    return Some(self.block_mode.paste(target.tab, &target.cwd, &text));
                }
                Some(crate::action::Action::Copy) => {
                    let state = self.block_mode.tabs.get(&target.tab)?;
                    let selected = state.ctl.editor.selected_text()?.to_string();
                    cx.clipboard_set(&selected);
                    return Some(false);
                }
                _ => {}
            }
        }
        match self
            .block_mode
            .handle_key(target.tab, &target.cwd, ev, target.modes)
        {
            KeyOutcome::Pass => None,
            KeyOutcome::Handled => Some(true),
            KeyOutcome::Write(bytes) => {
                target.session.write(&bytes);
                Some(true)
            }
        }
    }

    /// Recompute titlebar drag regions and send them to the platform when they change.
    fn update_drag_regions(&mut self, cx: &mut PlatformCx, win: WindowId) {
        let min = Size::new(self.fonts.geo.cell_w * 12.0, self.fonts.geo.cell_h * 3.0);
        crate::frame::drag_regions_into(&mut self.model, win, min, &mut self.drag_scratch);
        if self.drag_regions.get(&win) != Some(&self.drag_scratch) {
            let regions = self.drag_scratch.clone();
            self.drag_regions.insert(win, regions.clone());
            cx.set_drag_regions(win, regions);
        }
    }

    pub fn update_font_size(&mut self, size: f32) {
        self.font_size = size.clamp(6.0, 72.0);
        let spec = FontSpec {
            family: if self.config.font.family.is_empty() {
                None
            } else {
                Some(self.config.font.family.clone())
            },
            size_pt: self.font_size,
            weight: termin_text::FontWeight::Regular,
            italic: false,
        };
        let font_id = self.glyphs.backend.resolve(&spec);
        let metrics = self.glyphs.backend.metrics(font_id);
        self.fonts = TermFonts::new(font_id, &metrics, self.fonts.geo.scale);
        self.invalidate_render_caches();
    }

    fn invalidate_render_caches(&mut self) {
        self.glyphs.clear();
        for cache in self.pane_caches.values_mut() {
            cache.invalidate();
        }
    }

    /// Apply a new backing scale factor: rebuild font metrics at that scale and drop every
    /// cache whose contents depend on it (glyph atlas, row caches). The grid size and PTY
    /// winsize are recomputed on the next frame from the new cell size.
    fn apply_scale(&mut self, scale: f32) {
        if !scale.is_finite() || scale < 1.0 || (scale - self.fonts.geo.scale).abs() < 1e-3 {
            return;
        }
        self.glyphs.backend.set_scale(scale);
        let spec = FontSpec {
            family: if self.config.font.family.is_empty() {
                None
            } else {
                Some(self.config.font.family.clone())
            },
            size_pt: self.font_size,
            weight: termin_text::FontWeight::Regular,
            italic: false,
        };
        let font_id = self.glyphs.backend.resolve(&spec);
        let metrics = self.glyphs.backend.metrics(font_id);
        self.fonts = TermFonts::new(font_id, &metrics, scale);
        self.invalidate_render_caches();
        self.resize_times.clear();
    }
}

impl AppHandler for App {
    fn init(&mut self, cx: &mut PlatformCx) {
        // 1. Start IO reactor
        let wake = cx.wake_handle();
        match Reactor::<Terminal>::start(wake) {
            Ok(handle) => {
                self.reactor = Some(handle);
            }
            Err(e) => {
                log::error!("Failed to start IO reactor: {:?}", e);
            }
        }

        // 2. Initialize Metal renderer
        match MetalRenderer::new() {
            Ok(renderer) => {
                self.renderer = Some(renderer);
            }
            Err(e) => {
                log::error!("Failed to initialize Metal renderer: {:?}", e);
            }
        }

        // 3. Create initial window
        let default_size = Size::new(960.0, 640.0);
        let opts = WindowOpts {
            title: Some("Termin".to_string()),
            frame: Some(termin_core::Rect::new(
                100.0,
                100.0,
                default_size.w,
                default_size.h,
            )),
            ..WindowOpts::default()
        };
        let win = cx.create_window(&opts);
        self.model.new_window(Some(win), default_size, None);

        // 4. Spawn sessions for new tabs
        self.process_model_effects(cx);
        cx.request_frame(win);
    }

    fn event(&mut self, cx: &mut PlatformCx, win: WindowId, ev: PlatformEvent) {
        match ev {
            PlatformEvent::Key(key_ev) => {
                if key_ev.pressed {
                    if let Some(action) = self
                        .keymap
                        .lookup(&key_ev, crate::keymap::WhenContext::Terminal)
                    {
                        match action {
                            crate::action::Action::FontIncrease => {
                                self.update_font_size(self.font_size + 1.0);
                                cx.request_frame(win);
                                return;
                            }
                            crate::action::Action::FontDecrease => {
                                self.update_font_size(self.font_size - 1.0);
                                cx.request_frame(win);
                                return;
                            }
                            crate::action::Action::FontReset => {
                                self.update_font_size(self.config.font.size);
                                cx.request_frame(win);
                                return;
                            }
                            crate::action::Action::NewWindow => {
                                let default_size = Size::new(960.0, 640.0);
                                let opts = WindowOpts {
                                    title: Some("Termin".to_string()),
                                    frame: Some(termin_core::Rect::new(
                                        120.0,
                                        120.0,
                                        default_size.w,
                                        default_size.h,
                                    )),
                                    ..WindowOpts::default()
                                };
                                let new_win = cx.create_window(&opts);
                                self.model.new_window(Some(new_win), default_size, None);
                                self.process_model_effects(cx);
                                cx.request_frame(new_win);
                                return;
                            }
                            _ => {}
                        }
                    }
                }
                if let Some(redraw) = self.block_key(&key_ev, cx) {
                    if redraw {
                        cx.request_frame(win);
                    }
                    return;
                }
                if handle_key(&mut self.model, &self.keymap, &key_ev, cx) {
                    self.process_model_effects(cx);
                    cx.request_frame(win);
                }
            }
            PlatformEvent::ImeCommit(text) => {
                if let Some(target) = self.focused_block_target() {
                    if self.block_mode.insert_text(target.tab, &target.cwd, &text) {
                        cx.request_frame(win);
                        return;
                    }
                }
                if handle_ime_commit(&mut self.model, &text) {
                    cx.request_frame(win);
                }
            }
            PlatformEvent::Scroll(scroll_ev) if self.focused_block_target().is_some() => {
                let Some(target) = self.focused_block_target() else {
                    return;
                };
                let dy = if scroll_ev.precise {
                    scroll_ev.dy
                } else {
                    scroll_ev.dy * self.fonts.geo.cell_h
                };
                if self.block_mode.scroll(target.tab, dy) {
                    cx.request_frame(win);
                }
            }
            PlatformEvent::Mouse(mouse_ev) => {
                let block_panes = self.block_panes();
                if handle_window_mouse(&mut self.model, win, &mouse_ev, &self.fonts, &|pane| {
                    block_panes.contains(&pane)
                }) {
                    self.process_model_effects(cx);
                    cx.request_frame(win);
                }
            }
            PlatformEvent::Scroll(scroll_ev) => {
                if handle_scroll(
                    &mut self.model,
                    win,
                    &scroll_ev,
                    &self.fonts,
                    &mut self.scroll_remainders,
                ) {
                    cx.request_frame(win);
                }
            }
            PlatformEvent::Resized { size, scale } => {
                if let Some(win_state) = self.model.windows.get_mut(win) {
                    win_state.size = size;
                }
                self.apply_scale(scale);
                cx.request_frame(win);
            }
            PlatformEvent::Focus(focused) => {
                if focused {
                    self.model.focused_window = Some(win);
                }
                cx.request_frame(win);
            }
            PlatformEvent::CloseRequested => {
                self.model.close_window(win);
                self.process_model_effects(cx);
            }
            _ => {}
        }
    }

    fn frame(&mut self, cx: &mut PlatformCx, win: WindowId, target: &mut dyn RenderTarget) {
        // The render target is the source of truth for the backing scale (covers the initial
        // frame and moving between displays with different scale factors).
        self.apply_scale(target.scale_factor());
        self.update_drag_regions(cx, win);
        let mut resize_pending = false;
        if let Some(ref mut renderer) = self.renderer {
            resize_pending = render_window_frame(
                &mut self.model,
                win,
                target,
                renderer,
                &mut self.glyphs,
                &self.fonts,
                &self.palette,
                &self.tokens,
                &mut self.pane_caches,
                self.cursor_blink_visible,
                &mut self.block_mode,
                &mut self.block_scratch,
                &mut self.resize_times,
            );
        }
        if resize_pending {
            cx.request_frame(win);
        }
    }

    fn wake(&mut self, cx: &mut PlatformCx) {
        let mut dirty_windows = Vec::new();

        // Reactor events are the source of redraw invalidation. PTY output can
        // change visible cells without producing a VT side effect.
        if let Some(reactor) = &self.reactor {
            let mut events = Vec::new();
            reactor.drain_events(&mut events);
            for event in events {
                match event {
                    ReactorEvent::SessionOutput(tab) => {
                        if let Some(window) = window_for_session(&self.model, tab) {
                            if !dirty_windows.contains(&window) {
                                dirty_windows.push(window);
                            }
                        }
                    }
                    ReactorEvent::SessionExited(tab, _) => {
                        self.retired_sessions.remove(&tab);
                        if let Some(reactor) = &self.reactor {
                            if let Err(error) = reactor.unregister_session(tab) {
                                log::warn!(
                                    "Could not unregister exited session {}: {error}",
                                    tab.0
                                );
                            }
                        }
                        if let Some(window) = window_for_session(&self.model, tab) {
                            if !dirty_windows.contains(&window) {
                                dirty_windows.push(window);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Drain effects from all active sessions
        let tab_sessions: Vec<_> = self
            .model
            .tabs
            .iter()
            .filter_map(|(id, tab)| tab.session.clone().map(|s| (id, s)))
            .collect();

        for (id, session) in tab_sessions {
            let block_mode = &mut self.block_mode;
            let changed = self.model.tabs.get_mut(id).is_some_and(|tab| {
                drain_session_effects(tab, &session, |ev| block_mode.on_block_event(id, ev))
            });
            if changed {
                if let Some(window) = window_for_tab(&self.model, id) {
                    if !dirty_windows.contains(&window) {
                        dirty_windows.push(window);
                    }
                }
            }
        }

        self.process_model_effects(cx);
        let tabs = &self.model.tabs;
        self.block_mode.retain(|id| tabs.get(id).is_some());

        for win in dirty_windows {
            cx.request_frame(win);
        }
    }

    fn menu_action(&mut self, cx: &mut PlatformCx, action: ActionId) {
        let _ = (cx, action);
    }

    fn should_terminate(&mut self, cx: &mut PlatformCx) -> bool {
        let wins: Vec<_> = self.model.windows.ids().collect();
        for w in wins {
            self.model.close_window(w);
        }
        self.process_model_effects(cx);
        true
    }
}

fn window_for_tab(model: &AppModel, tab_id: termin_core::TabId) -> Option<WindowId> {
    let tab = model.tabs.get(tab_id)?;
    let pane = model.panes.get(tab.pane)?;
    let workspace = model.workspaces.get(pane.workspace)?;
    model
        .windows
        .contains(workspace.window)
        .then_some(workspace.window)
}

fn window_for_session(model: &AppModel, session_id: termin_core::TabId) -> Option<WindowId> {
    model
        .tabs
        .iter()
        .find_map(|(tab_id, tab)| {
            (tab.session.as_ref().map(|session| session.id) == Some(session_id)).then_some(tab_id)
        })
        .and_then(|tab_id| window_for_tab(model, tab_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use termin_core::{Size, WakeHandle};
    use termin_pty::SessionState;
    use termin_pty::SpawnSpec;
    use termin_vt::{Osc52Policy, TerminalConfig};

    #[test]
    fn model_tab_resolves_to_its_owning_window() {
        let mut model = AppModel::new();
        let first = model.new_window(None, Size::new(800.0, 600.0), None);
        let first_tab = model.focused_tab().expect("initial tab");
        let second = model.new_window(None, Size::new(800.0, 600.0), None);
        let second_tab = model.focused_tab().expect("second tab");

        assert_ne!(first, second);
        assert_eq!(window_for_tab(&model, first_tab), Some(first));
        assert_eq!(window_for_tab(&model, second_tab), Some(second));
        assert_eq!(window_for_tab(&model, termin_core::TabId(u32::MAX)), None);
    }

    #[test]
    fn closing_tab_sends_hup_and_retains_handle_until_exit() {
        let mut app = App::default();
        let window = app.model.new_window(None, Size::new(800.0, 600.0), None);
        let tab_id = app.model.focused_tab().expect("initial tab");
        // This test installs its own short-lived session instead of spawning the
        // configured interactive shell from the model's initial SpawnTab effect.
        app.model.drain_effects();

        let reactor = Reactor::<Terminal>::start(WakeHandle::new(|| {})).expect("reactor");
        let terminal = Terminal::new(&TerminalConfig {
            cols: 80,
            rows: 24,
            scrollback_lines: 100,
            cell_px: (8, 16),
            osc52: Osc52Policy::Write,
        });
        let session = reactor
            .spawn(
                SpawnSpec {
                    prog: PathBuf::from("/bin/sleep"),
                    args: vec!["60".into()],
                    env: Vec::new(),
                    cwd: PathBuf::from("/tmp"),
                    cols: 80,
                    rows: 24,
                    cell_px: (8, 16),
                },
                terminal,
            )
            .expect("sleep session");
        app.model.tabs.get_mut(tab_id).expect("tab").session = Some(Arc::clone(&session));
        app.session_handles.insert(tab_id, Arc::clone(&session));
        app.session_windows.insert(tab_id, window);
        app.reactor = Some(reactor.clone());
        let mut closed_windows = Vec::new();

        assert!(app.model.close_tab(tab_id));
        app.process_model_effects_with(|window| closed_windows.push(window));
        assert_eq!(closed_windows, vec![window]);
        assert!(!app.session_handles.contains_key(&tab_id));
        assert!(app.retired_sessions.contains_key(&session.id));

        let deadline = Instant::now() + Duration::from_secs(2);
        while session.state() == SessionState::Running && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(matches!(session.state(), SessionState::Exited(_)));
        let mut cx = PlatformCx::new(WakeHandle::new(|| {}));
        AppHandler::wake(&mut app, &mut cx);
        assert!(!app.retired_sessions.contains_key(&session.id));
        reactor.shutdown();
    }
}
