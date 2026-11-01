//! UI layout, theme tokens, widgets, and DrawList builder (`termin-ui`).
//!
//! See `docs/design/design-system.md` and `docs/design/components.md`.

pub mod block_view;
pub mod draw;
pub mod terminal_view;
pub mod theme;
pub mod widgets;

pub use draw::{draw_text_mono, place_glyph, rounded_quad, solid_quad, CellGeometry};
pub use terminal_view::{
    build_row, char_of_cell, hash_cells, PaneRenderCache, RowCtx, RowInstances, TermFonts,
    TermPalette, TerminalView, TerminalViewOpts, ViewKey,
};
pub use theme::{MotionTokens, Tokens};
pub use widgets::{
    draw_sidebar, draw_tab, draw_tabstrip, draw_workspace_card, PaneWidget, Scrollbar,
    ScrollbarTick, ScrollbarTickKind, SidebarWorkspace, StripHits, TabHits, TabState,
    WorkspaceCardState, TAB_STRIP_HEIGHT,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokens_default() {
        let tokens = Tokens::default();
        assert_eq!(tokens.bg_app.r(), 0x0C);
    }
}
