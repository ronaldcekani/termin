//! Single-line inline text field (rename editor): editing logic plus a proportional-text renderer.

use crate::{
    draw::{draw_text_ui, measure_text_ui, rounded_quad, solid_quad, UiFont},
    terminal_view::row::TermFonts,
    theme::Tokens,
};
use termin_core::{Key, KeyEvent, Mods, NamedKey, Rect};
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};

/// Result of feeding a key to a [`TextField`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FieldOutcome {
    /// Key is not an editing key (the field still swallows it while focused).
    Ignored,
    /// Text, caret or selection changed.
    Edited,
    /// Enter pressed.
    Commit,
    /// Escape pressed.
    Cancel,
}

/// Single-line text buffer with caret and optional selection (char indices).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextField {
    text: String,
    caret: usize,
    anchor: Option<usize>,
}

impl TextField {
    /// New field holding `initial`, fully selected so typing replaces it.
    pub fn new(initial: &str) -> Self {
        let text: String = initial.chars().filter(|c| !c.is_control()).collect();
        let n = text.chars().count();
        Self {
            text,
            caret: n,
            anchor: if n > 0 { Some(0) } else { None },
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    /// Selected char range `(start, end)` if non-empty.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        if a == self.caret {
            None
        } else {
            Some((a.min(self.caret), a.max(self.caret)))
        }
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn byte_at(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map_or(self.text.len(), |(b, _)| b)
    }

    fn delete_selection(&mut self) -> bool {
        let Some((s, e)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        let (bs, be) = (self.byte_at(s), self.byte_at(e));
        self.text.replace_range(bs..be, "");
        self.caret = s;
        self.anchor = None;
        true
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.len();
    }

    /// Insert text at the caret (replacing any selection); control characters are dropped.
    pub fn insert(&mut self, s: &str) -> bool {
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return false;
        }
        self.delete_selection();
        let at = self.byte_at(self.caret);
        self.text.insert_str(at, &clean);
        self.caret += clean.chars().count();
        true
    }

    pub fn backspace(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.caret == 0 {
            return false;
        }
        let (s, e) = (self.byte_at(self.caret - 1), self.byte_at(self.caret));
        self.text.replace_range(s..e, "");
        self.caret -= 1;
        true
    }

    pub fn delete_forward(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.caret >= self.len() {
            return false;
        }
        let (s, e) = (self.byte_at(self.caret), self.byte_at(self.caret + 1));
        self.text.replace_range(s..e, "");
        true
    }

    fn move_to(&mut self, pos: usize, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = pos.min(self.len());
    }

    pub fn move_left(&mut self, extend: bool) {
        if !extend {
            if let Some((s, _)) = self.selection() {
                self.move_to(s, false);
                return;
            }
        }
        self.move_to(self.caret.saturating_sub(1), extend);
    }

    pub fn move_right(&mut self, extend: bool) {
        if !extend {
            if let Some((_, e)) = self.selection() {
                self.move_to(e, false);
                return;
            }
        }
        self.move_to(self.caret + 1, extend);
    }

    pub fn home(&mut self, extend: bool) {
        self.move_to(0, extend);
    }

    pub fn end(&mut self, extend: bool) {
        self.move_to(self.len(), extend);
    }

    /// Apply a key press.
    pub fn handle_key(&mut self, ev: &KeyEvent) -> FieldOutcome {
        use FieldOutcome::*;
        let shift = ev.mods.contains(Mods::SHIFT);
        let cmd = ev.mods.contains(Mods::CMD);
        match ev.key {
            Key::Named(NamedKey::Enter) => return Commit,
            Key::Named(NamedKey::Escape) => return Cancel,
            Key::Named(NamedKey::Backspace) => return edited(self.backspace()),
            Key::Named(NamedKey::Delete) => return edited(self.delete_forward()),
            Key::Named(NamedKey::ArrowLeft) => {
                if cmd {
                    self.home(shift)
                } else {
                    self.move_left(shift)
                }
                return Edited;
            }
            Key::Named(NamedKey::ArrowRight) => {
                if cmd {
                    self.end(shift)
                } else {
                    self.move_right(shift)
                }
                return Edited;
            }
            Key::Named(NamedKey::Home) => {
                self.home(shift);
                return Edited;
            }
            Key::Named(NamedKey::End) => {
                self.end(shift);
                return Edited;
            }
            Key::Char('a') | Key::Char('A') if cmd => {
                self.select_all();
                return Edited;
            }
            _ => {}
        }
        if cmd || ev.mods.contains(Mods::CTRL) {
            return Ignored;
        }
        // Space is deliberately not synthesized: the platform reports modifier
        // changes as `Named(Space)` key events, and typed text arrives via IME commit.
        match &ev.text {
            Some(t) => edited(self.insert(t.as_ref())),
            None => Ignored,
        }
    }

    /// First visible char index so the caret stays within `cols` columns.
    pub fn view_start(&self, cols: usize) -> usize {
        if cols == 0 {
            return 0;
        }
        (self.caret + 1).saturating_sub(cols)
    }
}

fn edited(changed: bool) -> FieldOutcome {
    if changed {
        FieldOutcome::Edited
    } else {
        FieldOutcome::Ignored
    }
}

/// Draw `field` inside `rect` (background, selection, text, caret).
#[allow(clippy::too_many_arguments)]
pub fn draw_text_field<B: FontBackend>(
    rect: Rect,
    field: &TextField,
    caret_visible: bool,
    tokens: &Tokens,
    fonts: &TermFonts,
    glyphs: &mut GlyphCache<B>,
    layer: &mut Layer,
) {
    layer.quads.push(rounded_quad(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        tokens.bg_app,
        4.0,
        1.0,
        tokens.accent_tide,
    ));
    let ui = &fonts.ui;
    let pad = 6.0;
    let avail = (rect.w - 2.0 * pad).max(1.0);
    let ty = rect.y + ((rect.h - ui.geo.cell_h) * 0.5).max(0.0);
    draw_field_text(
        field,
        caret_visible,
        rect.x + pad,
        ty,
        avail,
        tokens,
        ui,
        glyphs,
        layer,
    );
}

/// Draw `field`'s selection, text and caret with its line box top-left at `(tx, ty)`, scrolled
/// horizontally so the caret stays within `avail` points. Caret and selection x positions come
/// from measured advances of the proportional UI font.
#[allow(clippy::too_many_arguments)]
pub fn draw_field_text<B: FontBackend>(
    field: &TextField,
    caret_visible: bool,
    tx: f32,
    ty: f32,
    avail: f32,
    tokens: &Tokens,
    ui: &UiFont,
    glyphs: &mut GlyphCache<B>,
    layer: &mut Layer,
) {
    let cell_h = ui.geo.cell_h;
    let text = field.text();
    // Byte offset of char index `i` (clamped to the end).
    let byte_at = |i: usize| text.char_indices().nth(i).map_or(text.len(), |(b, _)| b);
    // Scroll horizontally (drop leading chars) until the caret fits with room for an
    // ellipsis; caret and selection x positions come from measured advances.
    let caret = field.caret();
    let reserve = 8.0;
    let mut start = 0usize;
    let mut caret_w = measure_text_ui(glyphs, ui, &text[..byte_at(caret)]);
    if caret_w > avail - reserve {
        for c in text.chars().take(caret) {
            if caret_w <= avail - reserve {
                break;
            }
            let g = glyphs.backend.glyph(ui.font, c);
            caret_w -= glyphs.backend.glyph_advance(ui.font, g);
            start += 1;
        }
    }
    let vis = &text[byte_at(start)..];
    if let Some((s, e)) = field.selection() {
        let (s, e) = (s.max(start), e.max(start));
        let x0 = measure_text_ui(glyphs, ui, &text[byte_at(start)..byte_at(s)]);
        let x1 = measure_text_ui(glyphs, ui, &text[byte_at(start)..byte_at(e)]);
        let x1 = x1.min(avail);
        if x1 > x0 {
            layer
                .quads
                .push(solid_quad(tx + x0, ty, x1 - x0, cell_h, tokens.bg_selected));
        }
    }
    draw_text_ui(glyphs, ui, vis, tx, ty, avail, tokens.text_primary, layer);
    if caret_visible {
        let cx = (tx + caret_w).round();
        layer
            .quads
            .push(solid_quad(cx, ty, 1.5, cell_h, tokens.text_primary));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn typing_replaces_initial_selection() {
        let mut f = TextField::new("abc");
        assert_eq!(f.selection(), Some((0, 3)));
        assert_eq!(
            f.handle_key(&key(Key::Char('x'), Mods::empty(), Some("x"))),
            FieldOutcome::Edited
        );
        assert_eq!(f.text(), "x");
        assert_eq!(f.caret(), 1);
    }

    #[test]
    fn edit_navigation_and_unicode() {
        let mut f = TextField::new("");
        f.insert("héy");
        f.move_left(false);
        assert!(f.backspace());
        assert_eq!(f.text(), "hy");
        f.home(false);
        assert!(!f.backspace());
        assert!(f.delete_forward());
        assert_eq!(f.text(), "y");
        f.end(false);
        assert_eq!(f.caret(), 1);
    }

    #[test]
    fn select_all_and_replace_and_outcomes() {
        let mut f = TextField::new("");
        f.insert("hello");
        let cmd_a = key(Key::Char('a'), Mods::CMD, None);
        assert_eq!(f.handle_key(&cmd_a), FieldOutcome::Edited);
        assert_eq!(f.selection(), Some((0, 5)));
        f.insert("z");
        assert_eq!(f.text(), "z");
        assert_eq!(
            f.handle_key(&key(Key::Named(NamedKey::Enter), Mods::empty(), None)),
            FieldOutcome::Commit
        );
        assert_eq!(
            f.handle_key(&key(Key::Named(NamedKey::Escape), Mods::empty(), None)),
            FieldOutcome::Cancel
        );
        assert_eq!(
            f.handle_key(&key(Key::Char('c'), Mods::CMD, Some("c"))),
            FieldOutcome::Ignored
        );
    }

    #[test]
    fn control_chars_dropped_and_view_scrolls() {
        let mut f = TextField::new("");
        assert!(!f.insert("\n\x1b"));
        f.insert("abcdefgh");
        assert_eq!(f.view_start(4), 5);
        f.home(false);
        assert_eq!(f.view_start(4), 0);
    }

    #[test]
    fn shift_arrows_extend_selection() {
        let mut f = TextField::new("");
        f.insert("abc");
        f.move_left(true);
        f.move_left(true);
        assert_eq!(f.selection(), Some((1, 3)));
        f.move_left(false);
        assert_eq!(f.selection(), None);
        assert_eq!(f.caret(), 1);
    }
}
