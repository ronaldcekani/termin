//! Frame rendering into the window's Metal render target (`termin-app::frame`).
//!
//! See `docs/architecture.md §4.1` and `docs/specs/text-and-rendering.md §3`.

use rustc_hash::FxHashMap;
use termin_core::{PaneId, Rect, Size, WindowId};
use termin_platform::RenderTarget;
use termin_render::{DrawList, Layer, MetalRenderer, Renderer};
use termin_text::{CoreTextBackend, GlyphCache};
use termin_ui::{
    draw_sidebar, draw_tabstrip, solid_quad, PaneRenderCache, PaneWidget, RowCtx, Scrollbar,
    SidebarWorkspace, TabState, TermFonts, TermPalette, TerminalView, TerminalViewOpts, Tokens,
    ViewKey, TAB_STRIP_HEIGHT,
};

use termin_ui::widgets::tabstrip::StripLayout;

use crate::model::AppModel;

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
) {
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
        None => return,
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
            draw_sidebar(
                sidebar_rect,
                &workspaces,
                tokens,
                fonts,
                glyphs,
                &mut sidebar_layer,
            );
            draw_list.push_layer(sidebar_layer);
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
                        tab.title.display(),
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
                    let cols = (content_rect.w / cell_w).floor().max(1.0) as u16;
                    let rows = (content_rect.h / cell_h).floor().max(1.0) as u16;

                    // Resize through Session only after releasing its terminal lock:
                    // Session::resize locks the same terminal to update the consumer.
                    let needs_resize = session
                        .term
                        .lock()
                        .map(|term| term.cols() != cols || term.rows() != rows)
                        .unwrap_or(false);
                    if needs_resize {
                        session.resize(cols, rows, (content_rect.w as u16, content_rect.h as u16));
                    }

                    if let Ok(mut term) = session.term.lock() {
                        let cache = pane_caches.entry(pane_id).or_default();
                        let view_key = ViewKey {
                            cols,
                            rows,
                            epoch: 0,
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
            let Some(pane) = model.panes.get(pane_id) else {
                continue;
            };
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
            if gap > 0.0 {
                out.push(Rect::new(tabs_end, strip.y, gap, strip.h));
            }
        }
    }
    if out.is_empty() {
        // Off-window sentinel: keeps the platform from falling back to the default band.
        out.push(Rect::new(-1.0, -1.0, 0.0, 0.0));
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
