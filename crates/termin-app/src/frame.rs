//! Frame rendering into the window's Metal render target (`termin-app::frame`).
//!
//! See `docs/architecture.md §4.1` and `docs/specs/text-and-rendering.md §3`.

use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;
use termin_core::{PaneId, Rect, Size, WindowId};
use termin_platform::RenderTarget;
use termin_render::{DrawList, Layer, MetalRenderer, Renderer};
use termin_text::{CoreTextBackend, GlyphCache};
use termin_ui::{
    draw_sidebar, draw_tabstrip, solid_quad, PaneRenderCache, PaneWidget, RowCtx, RowInstances,
    Scrollbar, SidebarWorkspace, TabState, TermFonts, TermPalette, TerminalView, TerminalViewOpts,
    Tokens, ViewKey, TAB_STRIP_HEIGHT,
};

use termin_ui::block_view::render::{home_relative, ROW_INSET};
use termin_ui::block_view::BlockView;
use termin_ui::draw::draw_text_mono;
use termin_ui::widgets::completion_menu::CompletionMenuWidget;
use termin_ui::widgets::editor::EditorWidget;
use termin_ui::widgets::tabstrip::StripLayout;

use crate::block_mode::{BlockMode, BlockTab};
use crate::model::{AppModel, RenameEdit, RenameTarget};
use termin_ui::widgets::text_field::draw_text_field;

/// Minimum interval between PTY winsize/reflow updates per pane while the grid size is changing.
const RESIZE_THROTTLE: Duration = Duration::from_millis(48);

struct RenderTargetAdapter<'a>(&'a mut dyn RenderTarget);

impl<'a> termin_render::RenderTarget for RenderTargetAdapter<'a> {
    fn size(&self) -> Size {
        self.0.size()
    }
    fn scale_factor(&self) -> f32 {
        self.0.scale_factor()
    }
}

/// Render a complete frame for `win_id` into `target`.
#[allow(clippy::too_many_arguments)]
pub fn render_window_frame(
    model: &mut AppModel,
    win_id: WindowId,
    target: &mut dyn RenderTarget,
    renderer: &mut MetalRenderer,
    glyphs: &mut GlyphCache<CoreTextBackend>,
    fonts: &TermFonts,
    palette: &TermPalette,
    tokens: &Tokens,
    pane_caches: &mut FxHashMap<PaneId, PaneRenderCache>,
    cursor_blink_visible: bool,
    block_mode: &mut BlockMode,
    block_scratch: &mut RowInstances,
    resize_times: &mut FxHashMap<PaneId, Instant>,
) -> bool {
    let mut resize_pending = false;
    let size = target.size();
    let scale = target.scale_factor();

    // 1. Update window dimensions in model if changed
    if let Some(win) = model.windows.get_mut(win_id) {
        win.size = size;
    }

    // 2. Compute workspace layout
    let min_pane_size = Size::new(fonts.geo.cell_w * 12.0, fonts.geo.cell_h * 3.0);
    let layout = match model.layout(win_id, min_pane_size) {
        Some(l) => l.clone(),
        None => return false,
    };

    let mut draw_list = DrawList::new();
    let mut root_layer = Layer::new(Some(Rect::new(0.0, 0.0, size.w, size.h)));

    // Window background fill
    root_layer
        .quads
        .push(solid_quad(0.0, 0.0, size.w, size.h, tokens.bg_app));
    draw_list.push_layer(root_layer);

    // Sidebar is scoped to this OS window, including its own active workspace.
    if let Some(window) = model.windows.get(win_id) {
        if window.sidebar_visible {
            let sidebar_rect = window.sidebar_rect();
            let workspaces: Vec<_> = window
                .workspaces
                .iter()
                .filter_map(|&ws_id| {
                    let ws = model.workspaces.get(ws_id)?;
                    let unread = ws
                        .tree
                        .leaves()
                        .iter()
                        .filter_map(|pane_id| model.panes.get(*pane_id))
                        .flat_map(|pane| pane.tabs.iter())
                        .filter_map(|tab_id| model.tabs.get(*tab_id))
                        .filter(|tab| tab.attention.unread)
                        .count();
                    Some(SidebarWorkspace {
                        id: ws_id,
                        name: ws.name.as_deref().unwrap_or(""),
                        status: ws.meta.status.as_deref(),
                        git_branch: ws.meta.git_branch.as_deref(),
                        git_dirty: ws.meta.git_dirty,
                        paths: &ws.meta.paths,
                        ports: &ws.meta.ports,
                        unread,
                        active: ws_id == window.active_workspace,
                    })
                })
                .collect();
            let mut sidebar_layer = Layer::new(Some(sidebar_rect));
            let hits = draw_sidebar(
                sidebar_rect,
                &workspaces,
                tokens,
                fonts,
                glyphs,
                &mut sidebar_layer,
            );
            draw_list.push_layer(sidebar_layer);
            if let Some(RenameEdit {
                target: RenameTarget::Workspace(ws),
                field,
                ..
            }) = model.rename.as_ref()
            {
                if let Some((_, card)) = hits.iter().find(|(id, _)| id == ws) {
                    let rect = Rect::new(
                        card.x + 6.0,
                        card.y + 4.0,
                        card.w - 12.0,
                        fonts.geo.cell_h + 10.0,
                    );
                    let mut layer = Layer::new(Some(*card));
                    draw_text_field(
                        rect,
                        field,
                        cursor_blink_visible,
                        tokens,
                        fonts,
                        glyphs,
                        &mut layer,
                    );
                    draw_list.push_layer(layer);
                }
            }
        }
    }

    let focused_pane_id = model
        .windows
        .get(win_id)
        .and_then(|window| model.workspaces.get(window.active_workspace))
        .map(|workspace| workspace.focused_pane);

    // 3. Render each visible pane
    for (pane_id, pane_rect) in layout.panes {
        let is_focused = Some(pane_id) == focused_pane_id;
        let mut pane_layer = Layer::new(Some(pane_rect));

        // Render each pane's actual tabs and controls. The strip gets its own layer:
        // TerminalView::render clips the pane layer to the content rect below it.
        let strip_rect = Rect::new(pane_rect.x, pane_rect.y, pane_rect.w, TAB_STRIP_HEIGHT);
        let mut strip_layer = Layer::new(Some(strip_rect));
        if let Some(pane) = model.panes.get(pane_id) {
            let tabs: Vec<_> = pane
                .tabs
                .iter()
                .filter_map(|&tab_id| {
                    let tab = model.tabs.get(tab_id)?;
                    Some((
                        tab_id,
                        // Prefer the cwd basename over the generic "shell" fallback.
                        match (tab.title.display(), tab.cwd.as_deref()) {
                            ("shell", Some(cwd)) => {
                                let base = cwd.trim_end_matches('/').rsplit('/').next();
                                match base {
                                    Some(b) if !b.is_empty() => b,
                                    _ => "~",
                                }
                            }
                            (t, _) => t,
                        },
                        TabState {
                            active: tab_id == pane.active_tab,
                            focused_pane: is_focused,
                            attention: tab.attention.unread,
                            running: tab.session.as_ref().is_some_and(|session| {
                                session.state() == termin_pty::SessionState::Running
                            }),
                            ..TabState::default()
                        },
                    ))
                })
                .collect();
            draw_tabstrip(
                strip_rect,
                &tabs,
                pane.active_tab,
                tokens,
                fonts,
                glyphs,
                &mut strip_layer,
            );
        }

        let content_rect = PaneWidget::content_rect(pane_rect);
        if !is_focused {
            pane_layer.quads.push(solid_quad(
                content_rect.x,
                content_rect.y,
                content_rect.w,
                content_rect.h,
                termin_core::Rgba::from_rgba(0x0C, 0x0C, 0x0D, 0x30),
            ));
        }

        // Terminal view inside content rect
        if let Some(pane) = model.panes.get(pane_id) {
            if let Some(tab) = model.tabs.get(pane.active_tab) {
                if let Some(ref session) = tab.session {
                    let cell_w = fonts.geo.cell_w;
                    let cell_h = fonts.geo.cell_h;
                    // Reserve the block-mode row insets in both modes so switching
                    // between classic and block rendering never resizes the PTY.
                    let cols = ((content_rect.w - 2.0 * ROW_INSET) / cell_w)
                        .floor()
                        .max(1.0) as u16;
                    let rows = (content_rect.h / cell_h).floor().max(1.0) as u16;

                    // Resize through Session only after releasing its terminal lock:
                    // Session::resize locks the same terminal to update the consumer.
                    // During live resize the PTY winsize + reflow is throttled; the
                    // view keeps rendering the terminal's current grid meanwhile.
                    let (cur_cols, cur_rows) = session
                        .term
                        .lock()
                        .map(|term| (term.cols(), term.rows()))
                        .unwrap_or((cols, rows));
                    let (cols, rows) = if cur_cols != cols || cur_rows != rows {
                        let now = Instant::now();
                        let due = resize_times
                            .get(&pane_id)
                            .map_or(true, |t| now.duration_since(*t) >= RESIZE_THROTTLE);
                        if due {
                            resize_times.insert(pane_id, now);
                            let px_w = (content_rect.w * scale).round() as u16;
                            let px_h = (content_rect.h * scale).round() as u16;
                            session.resize(cols, rows, (px_w, px_h));
                            (cols, rows)
                        } else {
                            resize_pending = true;
                            (cur_cols, cur_rows)
                        }
                    } else {
                        (cols, rows)
                    };

                    let block_tab = match session.term.lock() {
                        Ok(term) if block_mode.is_active(tab, &term) => {
                            block_mode.tabs.get_mut(&tab.id)
                        }
                        _ => None,
                    };
                    if let Some(block_tab) = block_tab {
                        if let Ok(term) = session.term.lock() {
                            render_block_pane(
                                &term,
                                block_tab,
                                BlockPaneCtx {
                                    rect: content_rect,
                                    focused: is_focused,
                                    cursor_blink_visible,
                                    cwd: tab.cwd.as_deref(),
                                },
                                glyphs,
                                fonts,
                                palette,
                                tokens,
                                block_scratch,
                                &mut draw_list,
                            );
                        }
                    } else if let Ok(mut term) = session.term.lock() {
                        let cache = pane_caches.entry(pane_id).or_default();
                        let view_key = ViewKey {
                            cols,
                            rows,
                            epoch: scale.to_bits(),
                            atlas_epoch: glyphs.epoch,
                        };

                        let mut ctx = RowCtx {
                            glyphs,
                            fonts,
                            palette,
                        };

                        let scroll_pos = term.display_scroll_pos();
                        cache.sync(&mut term, scroll_pos, view_key, &mut ctx);

                        let view_opts = TerminalViewOpts {
                            rect: content_rect,
                            focused: is_focused,
                            cursor_blink_visible,
                            selection: term.selection().map(|s| (s.anchor, s.head)),
                        };

                        TerminalView::render(
                            &term,
                            cache,
                            fonts,
                            palette,
                            &view_opts,
                            ctx.glyphs,
                            &mut pane_layer,
                        );

                        // Scrollbar if scrollback present
                        let total = term.scrollback_len() + term.rows() as usize;
                        let visible = term.rows() as usize;
                        if total > visible {
                            let sb_rect = Rect::new(
                                content_rect.right() - 6.0,
                                content_rect.y,
                                4.0,
                                content_rect.h,
                            );
                            let offset =
                                term.scrollback_len().saturating_sub(term.display_scroll());
                            let sb = Scrollbar::new(sb_rect, total, visible, offset);
                            sb.render(&mut pane_layer, tokens.border_subtle);
                        }
                    }
                }
            }
        }

        draw_list.push_layer(pane_layer);
        draw_list.push_layer(strip_layer);
        if let Some(RenameEdit {
            target: RenameTarget::Tab(tab),
            field,
            ..
        }) = model.rename.as_ref()
        {
            if let Some(rect) = crate::input::tab_rect_of(model, win_id, pane_id, *tab) {
                let rect = Rect::new(rect.x + 2.0, rect.y + 2.0, rect.w - 4.0, rect.h - 4.0);
                let mut layer = Layer::new(Some(strip_rect));
                draw_text_field(
                    rect,
                    field,
                    cursor_blink_visible,
                    tokens,
                    fonts,
                    glyphs,
                    &mut layer,
                );
                draw_list.push_layer(layer);
            }
        }
    }

    // 4. Render dividers
    let mut divider_layer = Layer::new(Some(Rect::new(0.0, 0.0, size.w, size.h)));
    for d in layout.dividers {
        divider_layer.quads.push(solid_quad(
            d.line.x,
            d.line.y,
            d.line.w,
            d.line.h,
            tokens.border_subtle,
        ));
    }
    draw_list.push_layer(divider_layer);

    // 5. Upload new glyph bitmaps to GPU atlas
    let mut uploads = Vec::new();
    glyphs.drain_uploads(&mut uploads);
    if !uploads.is_empty() {
        renderer.upload_glyphs_with_data(&uploads, glyphs.staging_bytes());
        glyphs.clear_staging();
    }

    // 6. Draw into Metal render target
    let layer_ptr = target.metal_layer();
    if !layer_ptr.is_null() {
        unsafe {
            // SAFETY: layer_ptr is guaranteed valid by RenderTarget contract during frame callback
            let _ = renderer.render_to_raw_layer(layer_ptr, size, scale, &draw_list, tokens.bg_app);
        }
    } else {
        let mut adapter = RenderTargetAdapter(target);
        renderer.render(&mut adapter, &draw_list, tokens.bg_app);
    }
    resize_pending
}

/// Per-pane inputs for [`render_block_pane`].
struct BlockPaneCtx<'a> {
    rect: Rect,
    focused: bool,
    cursor_blink_visible: bool,
    cwd: Option<&'a str>,
}

/// Height of the editor area for `lines` input lines (context chip row included).
fn editor_height(lines: usize, cell_h: f32) -> f32 {
    let chip_h = cell_h + 10.0;
    chip_h + lines.clamp(1, 12) as f32 * cell_h + 16.0
}

/// Draw a pane in Warp-style block mode: finished/running command blocks above,
/// the input editor (with a cwd chip row) below while the shell is at a prompt.
#[allow(clippy::too_many_arguments)]
fn render_block_pane(
    term: &termin_vt::Terminal,
    block_tab: &mut BlockTab,
    pane: BlockPaneCtx<'_>,
    glyphs: &mut GlyphCache<CoreTextBackend>,
    fonts: &TermFonts,
    palette: &TermPalette,
    tokens: &Tokens,
    scratch: &mut RowInstances,
    draw_list: &mut DrawList,
) {
    let cell_w = fonts.geo.cell_w;
    let cell_h = fonts.geo.cell_h;
    let rect = pane.rect;
    let editor_visible = block_tab.editor_visible();
    let editor_h = if editor_visible {
        let lines = block_tab.ctl.editor.text.split('\n').count();
        editor_height(lines, cell_h).min(rect.h)
    } else {
        0.0
    };
    let blocks_rect = Rect::new(rect.x, rect.y, rect.w, (rect.h - editor_h).max(0.0));

    // Blocks: chrome + output rows over a pixel-scrolled visual line map.
    let cursor = term.cursor();
    let bottom = termin_vt::AbsLine(cursor.pos.line.0 + 1);
    block_tab.map.rebuild(
        term.blocks(),
        cell_h,
        cell_h + 12.0,
        cell_h + 8.0,
        term.first_hot_line(),
        bottom,
    );
    let max_scroll = (block_tab.map.height - blocks_rect.h).max(0.0);
    block_tab.scroll_from_bottom = block_tab.scroll_from_bottom.min(max_scroll);
    let scroll_y = max_scroll - block_tab.scroll_from_bottom;

    let mut block_layer = Layer::new(Some(blocks_rect));
    BlockView::render_chrome(
        term.blocks(),
        &block_tab.map,
        blocks_rect,
        scroll_y,
        None,
        None,
        fonts,
        glyphs,
        tokens,
        &mut block_layer,
    );
    let running = !editor_visible;
    let cursor_line = (running && cursor.visible).then_some(cursor.pos.line);
    let mut ctx = RowCtx {
        glyphs,
        fonts,
        palette,
    };
    let cursor_row = BlockView::render_rows(
        term,
        &block_tab.map,
        blocks_rect,
        scroll_y,
        cursor_line,
        &mut ctx,
        scratch,
        &mut block_layer,
    );
    if let Some((x, y)) = cursor_row {
        if pane.focused && pane.cursor_blink_visible {
            block_layer.quads.push(solid_quad(
                x + cursor.pos.col as f32 * cell_w,
                y,
                cell_w,
                cell_h,
                palette.cursor,
            ));
        }
    }
    draw_list.push_layer(block_layer);

    if !editor_visible {
        return;
    }

    // Editor: context chip row (cwd) above the input.
    let editor_rect = Rect::new(rect.x, blocks_rect.bottom(), rect.w, editor_h);
    let chip_h = cell_h + 10.0;
    let mut editor_layer = Layer::new(Some(editor_rect));
    editor_layer.quads.push(solid_quad(
        editor_rect.x,
        editor_rect.y,
        editor_rect.w,
        editor_rect.h,
        tokens.bg_input,
    ));
    editor_layer.quads.push(solid_quad(
        editor_rect.x,
        editor_rect.y,
        editor_rect.w,
        1.0,
        tokens.border_subtle,
    ));
    let (tilde, rest) = home_relative(pane.cwd.unwrap_or("~"));
    let mut chip_x = editor_rect.x + ROW_INSET + 2.0;
    let chip_y = editor_rect.y + (chip_h - cell_h) * 0.5;
    if tilde {
        draw_text_mono(
            glyphs,
            fonts.font,
            &fonts.geo,
            "~",
            chip_x,
            chip_y,
            cell_w,
            tokens.accent_tide,
            &mut editor_layer,
        );
        chip_x += cell_w;
    }
    draw_text_mono(
        glyphs,
        fonts.font,
        &fonts.geo,
        rest,
        chip_x,
        chip_y,
        editor_rect.right() - chip_x - ROW_INSET,
        tokens.accent_tide,
        &mut editor_layer,
    );
    draw_list.push_layer(editor_layer);

    let input_rect = Rect::new(
        editor_rect.x,
        editor_rect.y + chip_h,
        editor_rect.w,
        (editor_rect.h - chip_h).max(0.0),
    );
    let mut input_layer = Layer::new(Some(input_rect));
    EditorWidget::render(
        &block_tab.ctl.editor,
        input_rect,
        pane.focused,
        false,
        fonts,
        glyphs,
        tokens,
        &mut input_layer,
    );
    draw_list.push_layer(input_layer);

    if let Some(menu) = block_tab.ctl.editor.menu.as_ref() {
        let mut menu_layer = Layer::new(Some(rect));
        CompletionMenuWidget::render(
            menu,
            input_rect,
            blocks_rect,
            fonts,
            glyphs,
            tokens,
            &mut menu_layer,
        );
        draw_list.push_layer(menu_layer);
    }
}

/// Height of the transparent titlebar band where empty chrome drags the window.
const TITLEBAR_DRAG_HEIGHT: f32 = 38.0;

/// Collect the window-drag regions for `win_id` into `out` (cleared first).
///
/// Only empty chrome in the titlebar band drags: the sidebar header left of its
/// "+" button and the gap in each top-row tab strip between the tabs and the
/// action buttons. Everything else (buttons, tabs, terminal) receives clicks.
/// `out` is never left empty, because the platform treats an empty list as
/// "whole titlebar band drags".
pub fn drag_regions_into(
    model: &mut AppModel,
    win_id: WindowId,
    min_pane_size: Size,
    out: &mut Vec<Rect>,
) {
    out.clear();
    if let Some(window) = model.windows.get(win_id) {
        if window.sidebar_visible {
            let sidebar = window.sidebar_rect();
            // Sidebar "+" occupies [right - 30, right - 6]; stop short of it.
            let w = (sidebar.w - 34.0).max(0.0);
            out.push(Rect::new(sidebar.x, sidebar.y, w, TITLEBAR_DRAG_HEIGHT));
        }
    }
    // Refresh the cached layout, then read it immutably alongside the pane arena.
    let _ = model.layout(win_id, min_pane_size);
    let layout = model
        .windows
        .get(win_id)
        .and_then(|w| model.workspaces.get(w.active_workspace))
        .map(|ws| &ws.layout);
    if let Some(layout) = layout {
        for &(pane_id, pane_rect) in &layout.panes {
            if pane_rect.y >= TITLEBAR_DRAG_HEIGHT {
                continue;
            }
            if let Some(gap) = strip_gap(model, pane_id, pane_rect) {
                out.push(gap);
            }
        }
    }
    if out.is_empty() {
        // Off-window sentinel: keeps the platform from falling back to the default band.
        out.push(Rect::new(-1.0, -1.0, 0.0, 0.0));
    }
}

/// Empty tab-strip gap (between the last tab and the action buttons) of `pane_id`.
fn strip_gap(model: &AppModel, pane_id: PaneId, pane_rect: Rect) -> Option<Rect> {
    let pane = model.panes.get(pane_id)?;
    let strip = Rect::new(pane_rect.x, pane_rect.y, pane_rect.w, TAB_STRIP_HEIGHT);
    let active = pane
        .tabs
        .iter()
        .position(|&t| t == pane.active_tab)
        .unwrap_or(0);
    let strip_layout = StripLayout::new(strip, pane.tabs.len(), active);
    let tabs_end =
        strip_layout.start_x + strip_layout.visible as f32 * (strip_layout.tab_width + 2.0);
    let gap = strip_layout.actions_x - tabs_end;
    (gap > 0.0).then(|| Rect::new(tabs_end, strip.y, gap, strip.h))
}

/// Tab-strip gaps inside the titlebar band: double-clicking them opens a new tab
/// (delivered to the app) instead of zooming the window. Uses the cached layout.
pub fn dblclick_regions_into(model: &AppModel, win_id: WindowId, out: &mut Vec<Rect>) {
    out.clear();
    let Some(window) = model.windows.get(win_id) else {
        return;
    };
    let Some(ws) = model.workspaces.get(window.active_workspace) else {
        return;
    };
    for &(pane_id, pane_rect) in &ws.layout.panes {
        if pane_rect.y >= TITLEBAR_DRAG_HEIGHT {
            continue;
        }
        if let Some(gap) = strip_gap(model, pane_id, pane_rect) {
            out.push(gap);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains_any(regions: &[Rect], x: f32, y: f32) -> bool {
        regions
            .iter()
            .any(|r| x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h)
    }

    #[test]
    fn dblclick_regions_are_tab_strip_gaps_inside_drag_regions() {
        let mut model = AppModel::new();
        let win = model.new_window(None, Size::new(960.0, 640.0), None);
        let min = Size::new(96.0, 48.0);
        let mut drag = Vec::new();
        drag_regions_into(&mut model, win, min, &mut drag);
        let mut dbl = Vec::new();
        dblclick_regions_into(&model, win, &mut dbl);
        assert!(!dbl.is_empty());
        for r in &dbl {
            assert!(contains_any(&drag, r.x + r.w * 0.5, r.y + r.h * 0.5));
        }
        // Sidebar header keeps zoom behaviour.
        let sidebar = model.windows.get(win).unwrap().sidebar_rect();
        assert!(!contains_any(&dbl, sidebar.x + 20.0, 10.0));
    }

    #[test]
    fn drag_regions_leave_buttons_clickable() {
        let mut model = AppModel::new();
        let win = model.new_window(None, Size::new(960.0, 640.0), None);
        let min = Size::new(96.0, 48.0);
        let mut regions = Vec::new();
        drag_regions_into(&mut model, win, min, &mut regions);

        let window = model.windows.get(win).unwrap();
        let sidebar = window.sidebar_rect();
        // Sidebar "+" button center must not drag.
        assert!(!contains_any(&regions, sidebar.right() - 18.0, 21.0));
        // Sidebar header title area drags.
        assert!(contains_any(&regions, sidebar.x + 20.0, 10.0));

        let layout = model.layout(win, min).unwrap().clone();
        let (_, pane) = layout.panes[0];
        // Tab-strip new-tab / split buttons and the first tab must not drag.
        let strip = Rect::new(pane.x, pane.y, pane.w, TAB_STRIP_HEIGHT);
        let actions_x = StripLayout::new(strip, 1, 0).actions_x;
        for i in 0..3 {
            assert!(!contains_any(
                &regions,
                actions_x + 26.0 * i as f32 + 12.0,
                15.0
            ));
        }
        assert!(!contains_any(&regions, pane.x + 20.0, 15.0));
        // Terminal content never drags.
        assert!(!contains_any(
            &regions,
            pane.x + 100.0,
            TAB_STRIP_HEIGHT + 5.0
        ));
    }

    #[test]
    fn drag_regions_never_empty() {
        let mut model = AppModel::new();
        let win = model.new_window(None, Size::new(10.0, 10.0), None);
        if let Some(w) = model.windows.get_mut(win) {
            w.sidebar_visible = false;
        }
        let mut regions = Vec::new();
        drag_regions_into(&mut model, win, Size::new(96.0, 48.0), &mut regions);
        assert!(!regions.is_empty());
    }
}
