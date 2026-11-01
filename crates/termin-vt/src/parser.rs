//! VTE parser integration and command dispatching.
//!
//! See `docs/specs/terminal-core.md §1`, `§4`, and `§5`.

use termin_core::Rgba;
use vte::{Params, Perform};

use crate::block::BlockList;
use crate::cell::{AbsLine, Attrs, Color, Style};
use crate::damage::Damage;
use crate::grid::Grid;
use crate::modes::{CursorShape, KittyKeyboard, Modes};
use crate::osc::handle_osc;
use crate::row::RowPool;
use crate::scrollback::Scrollback;
use crate::style::{GraphemeTable, HyperlinkTable, StyleTable};
use crate::{Effect, EffectSink, TerminalConfig};

#[inline]
fn active_grid_mut<'g>(is_alt: bool, grid: &'g mut Grid, alt_grid: &'g mut Grid) -> &'g mut Grid {
    if is_alt {
        alt_grid
    } else {
        grid
    }
}

pub struct ParserHandler<'a, S: EffectSink> {
    pub grid: &'a mut Grid,
    pub alt_grid: &'a mut Grid,
    pub is_alt_screen: bool,
    pub scrollback: &'a mut Scrollback,
    pub blocks: &'a mut BlockList,
    pub styles: &'a mut StyleTable,
    pub graphemes: &'a mut GraphemeTable,
    pub hyperlinks: &'a mut HyperlinkTable,
    pub row_pool: &'a mut RowPool,
    pub damage: &'a mut Damage,
    pub modes: &'a mut Modes,
    pub kitty: &'a mut KittyKeyboard,
    pub modify_other_keys: &'a mut u8,
    pub cursor_shape: &'a mut CursorShape,
    pub cursor_blink: &'a mut bool,
    pub current_style: Style,
    pub current_style_id: u16,
    pub sink: &'a mut S,
    pub cfg: &'a TerminalConfig,
    pub palette: &'a [Rgba; 256],
    pub default_fg: Rgba,
    pub default_bg: Rgba,
    // DCS buffer
    pub dcs_active: bool,
    pub dcs_bytes: Vec<u8>,
    pub in_ground: bool,
}

impl<'a, S: EffectSink> ParserHandler<'a, S> {
    #[inline]
    pub fn can_fast_ascii(&self) -> bool {
        if !self.in_ground || self.dcs_active || self.modes.contains(Modes::INSERT) {
            return false;
        }
        let g = if self.is_alt_screen {
            &*self.alt_grid
        } else {
            &*self.grid
        };
        !g.pending_wrap
            && g.gl == 0
            && g.single_shift.is_none()
            && g.g0 == crate::grid::Charset::Ascii
            && g.cursor_col + 1 < g.cols
    }

    #[inline]
    pub fn print_ascii_slice(&mut self, bytes: &[u8]) {
        let style_id = self.current_style_id;
        let is_alt = self.is_alt_screen;
        let cur_row = {
            let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
            let r = g.cursor_row;
            g.print_ascii_run(bytes, style_id);
            r
        };
        self.damage.mark_row(cur_row);
    }

    fn current_line(&self) -> AbsLine {
        let base = self.scrollback.hot_base_line + self.scrollback.hot_len() as u64;
        let row = if self.is_alt_screen {
            self.alt_grid.cursor_row
        } else {
            self.grid.cursor_row
        };
        AbsLine(base + row as u64)
    }

    fn update_style(&mut self) {
        self.current_style_id = self.styles.insert(self.current_style);
    }
}

impl<'a, S: EffectSink> Perform for ParserHandler<'a, S> {
    fn print(&mut self, c: char) {
        self.in_ground = true;
        let style_id = self.current_style_id;
        let modes = *self.modes;
        let is_alt = self.is_alt_screen;

        let cur_row = {
            let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
            g.cursor_row
        };
        self.damage.mark_row(cur_row);

        let evicted = {
            let grid = active_grid_mut(is_alt, self.grid, self.alt_grid);
            grid.print_char(c, style_id, modes, self.row_pool, self.graphemes)
        };

        if !is_alt {
            if let Some(row) = evicted {
                self.scrollback
                    .push_into_pool(row, self.styles, self.graphemes, self.row_pool);
                self.damage.add_scroll(-1);
            }
        } else if let Some(row) = evicted {
            self.row_pool.recycle(row);
        }
    }

    fn execute(&mut self, byte: u8) {
        let style_id = self.current_style_id;
        let is_alt = self.is_alt_screen;
        let modes = *self.modes;

        match byte {
            // BEL: Bell
            0x07 => {
                self.sink.effect(Effect::Bell);
            }

            // BS: Backspace
            0x08 => {
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.move_cursor_rel(0, -1, modes);
                self.damage.cursor_moved = true;
            }

            // HT: Horizontal Tab
            0x09 => {
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.move_to_next_tab();
                self.damage.cursor_moved = true;
            }

            // LF, VT, FF: Line feed
            0x0a..=0x0c => {
                let lnm = modes.contains(Modes::LINE_FEED_NEW_LINE);
                let (should_scroll, bottom_margin) = {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    if lnm {
                        g.cursor_col = 0;
                    }
                    g.pending_wrap = false;
                    (g.cursor_row >= g.bottom_margin, g.bottom_margin)
                };

                if should_scroll {
                    let evicted = {
                        let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                        g.scroll_up_one(style_id, self.row_pool)
                    };
                    if !is_alt {
                        if let Some(row) = evicted {
                            self.scrollback.push_into_pool(
                                row,
                                self.styles,
                                self.graphemes,
                                self.row_pool,
                            );
                            self.damage.add_scroll(-1);
                        }
                    } else if let Some(row) = evicted {
                        self.row_pool.recycle(row);
                    }
                } else {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    g.cursor_row = (g.cursor_row + 1).min(bottom_margin);
                }

                self.damage.cursor_moved = true;
            }

            // CR: Carriage return
            0x0d => {
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.cursor_col = 0;
                g.pending_wrap = false;
                self.damage.cursor_moved = true;
            }

            // SO: Shift Out (switch to G1 charset)
            0x0e => {
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.gl = 1;
            }

            // SI: Shift In (switch to G0 charset)
            0x0f => {
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.gl = 0;
            }

            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        self.in_ground = true;
        let modes = *self.modes;
        let style = self.current_style_id;
        let is_alt = self.is_alt_screen;

        match (intermediates, byte) {
            // ESC 7: DECSC (Save cursor)
            ([], b'7') => {
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.save_cursor(style, modes);
            }

            // ESC 8: DECRC (Restore cursor)
            ([], b'8') => {
                let (restored_style, new_modes) = {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    g.restore_cursor(modes)
                };
                self.current_style_id = restored_style;
                if let Some(s) = self.styles.get(restored_style) {
                    self.current_style = *s;
                }
                *self.modes = new_modes;
                self.damage.cursor_moved = true;
            }

            // ESC D: IND (Index)
            ([], b'D') => {
                let (should_scroll, bottom_margin) = {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    g.pending_wrap = false;
                    (g.cursor_row >= g.bottom_margin, g.bottom_margin)
                };

                if should_scroll {
                    let evicted = {
                        let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                        g.scroll_up(1, style, self.row_pool)
                    };
                    if !is_alt {
                        for row in evicted {
                            self.scrollback.push(row, self.styles, self.graphemes);
                            self.damage.add_scroll(-1);
                        }
                    }
                } else {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    g.cursor_row = (g.cursor_row + 1).min(bottom_margin);
                }
                self.damage.cursor_moved = true;
            }

            // ESC E: NEL (Next Line)
            ([], b'E') => {
                let (should_scroll, bottom_margin) = {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    g.cursor_col = 0;
                    g.pending_wrap = false;
                    (g.cursor_row >= g.bottom_margin, g.bottom_margin)
                };

                if should_scroll {
                    let evicted = {
                        let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                        g.scroll_up(1, style, self.row_pool)
                    };
                    if !is_alt {
                        for row in evicted {
                            self.scrollback.push(row, self.styles, self.graphemes);
                            self.damage.add_scroll(-1);
                        }
                    }
                } else {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    g.cursor_row = (g.cursor_row + 1).min(bottom_margin);
                }
                self.damage.cursor_moved = true;
            }

            // ESC H: HTS (Horizontal Tab Set)
            ([], b'H') => {
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.set_tab_stop();
            }

            // ESC M: RI (Reverse Index)
            ([], b'M') => {
                let (should_scroll, top_margin) = {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    g.pending_wrap = false;
                    (g.cursor_row <= g.top_margin, g.top_margin)
                };

                if should_scroll {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    g.scroll_down(1, style, self.row_pool);
                } else {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    g.cursor_row = g.cursor_row.saturating_sub(1).max(top_margin);
                }
                self.damage.cursor_moved = true;
            }

            // ESC =: Keypad Application Mode (DECKPAM)
            ([], b'=') => {
                self.modes.insert(Modes::DECKPAM);
                self.sink.effect(Effect::ModeChanged(*self.modes));
            }

            // ESC >: Keypad Numeric Mode (DECKPNM)
            ([], b'>') => {
                self.modes.remove(Modes::DECKPAM);
                self.sink.effect(Effect::ModeChanged(*self.modes));
            }

            // ESC c: RIS (Reset to Initial State)
            ([], b'c') => {
                self.grid.reset_size(self.cfg.cols, self.cfg.rows);
                self.alt_grid.reset_size(self.cfg.cols, self.cfg.rows);
                self.is_alt_screen = false;
                *self.modes = Modes::AUTOWRAP | Modes::CURSOR_VISIBLE;
                self.current_style = Style::default();
                self.current_style_id = 0;
                self.damage.mark_all();
            }

            // ESC N: Single Shift 2 (SS2)
            ([], b'N') => {
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.single_shift = Some(2);
            }

            // ESC O: Single Shift 3 (SS3)
            ([], b'O') => {
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.single_shift = Some(3);
            }

            // Charset designations: ESC ( C, ESC ) C, ESC * C, ESC + C
            ([b'('], c) => {
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.g0 = match c {
                    b'0' => crate::grid::Charset::DecSpecialGraphics,
                    b'A' => crate::grid::Charset::Uk,
                    _ => crate::grid::Charset::Ascii,
                };
            }
            ([b')'], c) => {
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.g1 = match c {
                    b'0' => crate::grid::Charset::DecSpecialGraphics,
                    b'A' => crate::grid::Charset::Uk,
                    _ => crate::grid::Charset::Ascii,
                };
            }
            ([b'*'], c) => {
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.g2 = match c {
                    b'0' => crate::grid::Charset::DecSpecialGraphics,
                    b'A' => crate::grid::Charset::Uk,
                    _ => crate::grid::Charset::Ascii,
                };
            }
            ([b'+'], c) => {
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.g3 = match c {
                    b'0' => crate::grid::Charset::DecSpecialGraphics,
                    b'A' => crate::grid::Charset::Uk,
                    _ => crate::grid::Charset::Ascii,
                };
            }

            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        self.in_ground = true;
        if intermediates.is_empty() && action == 'm' {
            self.handle_sgr(params);
            return;
        }
        let style = self.current_style_id;
        let modes = *self.modes;
        let is_alt = self.is_alt_screen;

        // Flatten first subparameter of each param into a stack array (zero heap alloc)
        let mut p_arr = [0u16; 16];
        let mut p_len = 0usize;
        for slice in params.iter() {
            if p_len < p_arr.len() {
                p_arr[p_len] = slice.first().copied().unwrap_or(0);
                p_len += 1;
            }
        }
        let p_vec = &p_arr[..p_len];
        let get_p = |idx: usize, def: u16| -> u16 {
            p_vec.get(idx).copied().filter(|&x| x > 0).unwrap_or(def)
        };

        match (intermediates, action) {
            // CUU: Cursor Up
            ([], 'A') => {
                let n = get_p(0, 1);
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.move_cursor_rel(-(n as i32), 0, modes);
                self.damage.cursor_moved = true;
            }

            // CUD: Cursor Down
            ([], 'B') => {
                let n = get_p(0, 1);
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.move_cursor_rel(n as i32, 0, modes);
                self.damage.cursor_moved = true;
            }

            // CUF: Cursor Forward
            ([], 'C') => {
                let n = get_p(0, 1);
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.move_cursor_rel(0, n as i32, modes);
                self.damage.cursor_moved = true;
            }

            // CUB: Cursor Backward
            ([], 'D') => {
                let n = get_p(0, 1);
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.move_cursor_rel(0, -(n as i32), modes);
                self.damage.cursor_moved = true;
            }

            // CNL: Cursor Next Line
            ([], 'E') => {
                let n = get_p(0, 1);
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.move_cursor_rel(n as i32, 0, modes);
                g.cursor_col = 0;
                self.damage.cursor_moved = true;
            }

            // CPL: Cursor Previous Line
            ([], 'F') => {
                let n = get_p(0, 1);
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.move_cursor_rel(-(n as i32), 0, modes);
                g.cursor_col = 0;
                self.damage.cursor_moved = true;
            }

            // CHA / HPA: Cursor Horizontal Absolute
            ([], 'G') | ([], '`') => {
                let col = get_p(0, 1).saturating_sub(1);
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.set_cursor_col(col, modes);
                self.damage.cursor_moved = true;
            }

            // CUP / HVP: Cursor Position (row, col)
            ([], 'H') | ([], 'f') => {
                let row = get_p(0, 1).saturating_sub(1);
                let col = get_p(1, 1).saturating_sub(1);
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.set_cursor_pos(row, col, modes);
                self.damage.cursor_moved = true;
            }

            // ED: Erase in Display
            ([], 'J') => {
                let mode = p_vec.first().copied().unwrap_or(0) as u8;
                if mode == 3 {
                    // ED 3: Clear scrollback
                    self.scrollback.clear();
                    let cur_line = self.current_line();
                    self.blocks.clear_up_to(cur_line);
                    self.damage.mark_all();
                } else {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    g.erase_display(mode, style);
                    if mode == 2 {
                        let cur_line = self.current_line();
                        self.blocks.clear_up_to(cur_line);
                    }
                    self.damage.mark_all();
                }
            }

            // EL: Erase in Line
            ([], 'K') => {
                let mode = p_vec.first().copied().unwrap_or(0) as u8;
                let cur_row = {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    let row = g.cursor_row;
                    g.erase_line(mode, style);
                    row
                };
                self.damage.mark_row(cur_row);
            }

            // IL: Insert Line
            ([], 'L') => {
                let n = get_p(0, 1);
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                if g.cursor_row >= g.top_margin && g.cursor_row <= g.bottom_margin {
                    let orig_top = g.top_margin;
                    g.top_margin = g.cursor_row;
                    g.scroll_down(n, style, self.row_pool);
                    let g2 = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    g2.top_margin = orig_top;
                }
                self.damage.mark_all();
            }

            // DL: Delete Line
            ([], 'M') => {
                let n = get_p(0, 1);
                let (in_margins, top, orig_top) = {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    (
                        g.cursor_row >= g.top_margin && g.cursor_row <= g.bottom_margin,
                        g.cursor_row,
                        g.top_margin,
                    )
                };

                if in_margins {
                    {
                        let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                        g.top_margin = top;
                    }
                    let evicted = {
                        let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                        g.scroll_up(n, style, self.row_pool)
                    };
                    {
                        let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                        g.top_margin = orig_top;
                    }
                    for row in evicted {
                        self.row_pool.recycle(row);
                    }
                }
                self.damage.mark_all();
            }

            // DCH: Delete Character
            ([], 'P') => {
                let n = get_p(0, 1);
                let cur_row = {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    g.delete_chars(n, style);
                    g.cursor_row
                };
                self.damage.mark_row(cur_row);
            }

            // ICH: Insert Character
            ([], '@') => {
                let n = get_p(0, 1);
                let cur_row = {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    g.insert_chars(n, style);
                    g.cursor_row
                };
                self.damage.mark_row(cur_row);
            }

            // ECH: Erase Character
            ([], 'X') => {
                let n = get_p(0, 1);
                let cur_row = {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    g.erase_chars(n, style);
                    g.cursor_row
                };
                self.damage.mark_row(cur_row);
            }

            // SU: Scroll Up
            ([], 'S') => {
                let n = get_p(0, 1);
                let evicted = {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    g.scroll_up(n, style, self.row_pool)
                };
                if !is_alt {
                    for row in evicted {
                        self.scrollback.push(row, self.styles, self.graphemes);
                    }
                }
                self.damage.mark_all();
            }

            // SD: Scroll Down
            ([], 'T') => {
                let n = get_p(0, 1);
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.scroll_down(n, style, self.row_pool);
                self.damage.mark_all();
            }

            // CHT: Cursor Forward Tabulation
            ([], 'I') => {
                let n = get_p(0, 1);
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                for _ in 0..n {
                    g.move_to_next_tab();
                }
                self.damage.cursor_moved = true;
            }

            // CBT: Cursor Backward Tabulation
            ([], 'Z') => {
                let n = get_p(0, 1);
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                for _ in 0..n {
                    g.move_to_prev_tab();
                }
                self.damage.cursor_moved = true;
            }

            // TBC: Tabulation Clear
            ([], 'g') => {
                let mode = p_vec.first().copied().unwrap_or(0);
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                if mode == 0 {
                    g.clear_tab_stop();
                } else if mode == 3 {
                    g.clear_all_tab_stops();
                }
            }

            // VPA: Vertical Position Absolute
            ([], 'd') => {
                let row = get_p(0, 1).saturating_sub(1);
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.set_cursor_row(row, modes);
                self.damage.cursor_moved = true;
            }

            // VPR: Vertical Position Relative
            ([], 'e') => {
                let n = get_p(0, 1);
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.move_cursor_rel(n as i32, 0, modes);
                self.damage.cursor_moved = true;
            }

            // HPR: Horizontal Position Relative
            ([], 'a') => {
                let n = get_p(0, 1);
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.move_cursor_rel(0, n as i32, modes);
                self.damage.cursor_moved = true;
            }

            // DECSTBM: Set Top and Bottom Margins
            ([], 'r') => {
                let top = get_p(0, 1).saturating_sub(1);
                let bottom = if p_vec.len() > 1 && p_vec[1] > 0 {
                    p_vec[1] - 1
                } else {
                    active_grid_mut(is_alt, self.grid, self.alt_grid)
                        .num_rows
                        .saturating_sub(1)
                };
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.set_scroll_region(top, bottom, modes);
                self.damage.cursor_moved = true;
            }

            // SGR: Set Graphics Rendition
            ([], 'm') => {
                self.handle_sgr(params);
            }

            // modifyOtherKeys: CSI > 4 ; n m
            ([b'>'], 'm') => {
                if p_vec.first().copied() == Some(4) && p_vec.len() > 1 {
                    *self.modify_other_keys = p_vec[1] as u8;
                }
            }

            // SM / DECSET: Set Mode
            ([], 'h') => {
                for &p in p_vec {
                    match p {
                        4 => self.modes.insert(Modes::INSERT),
                        20 => self.modes.insert(Modes::LINE_FEED_NEW_LINE),
                        _ => {}
                    }
                }
                self.sink.effect(Effect::ModeChanged(*self.modes));
            }

            ([b'?'], 'h') => {
                for &p in p_vec {
                    match p {
                        1 => self.modes.insert(Modes::DECCKM),
                        5 => self.modes.insert(Modes::REVERSE_VIDEO),
                        6 => {
                            self.modes.insert(Modes::ORIGIN);
                            let m = *self.modes;
                            let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                            g.set_cursor_pos(0, 0, m);
                        }
                        7 => self.modes.insert(Modes::AUTOWRAP),
                        9 => self.modes.insert(Modes::MOUSE_X10),
                        25 => self.modes.insert(Modes::CURSOR_VISIBLE),
                        47 | 1047 => {
                            if !self.is_alt_screen {
                                self.is_alt_screen = true;
                                self.modes.insert(Modes::ALT_SCREEN);
                                self.damage.mark_all();
                            }
                        }
                        1049 => {
                            if !self.is_alt_screen {
                                let m = *self.modes;
                                self.grid.save_cursor(style, m);
                                self.is_alt_screen = true;
                                self.modes.insert(Modes::ALT_SCREEN);
                                self.alt_grid.erase_display(2, style);
                                self.alt_grid.cursor_row = 0;
                                self.alt_grid.cursor_col = 0;
                                self.damage.mark_all();
                            }
                        }
                        1000 => self.modes.insert(Modes::MOUSE_NORMAL),
                        1002 => self.modes.insert(Modes::MOUSE_BUTTON),
                        1003 => self.modes.insert(Modes::MOUSE_ANY),
                        1004 => self.modes.insert(Modes::FOCUS_EVENTS),
                        1006 => self.modes.insert(Modes::MOUSE_SGR),
                        1007 => self.modes.insert(Modes::ALT_SCROLL),
                        1016 => self.modes.insert(Modes::MOUSE_SGR_PIXELS),
                        2004 => self.modes.insert(Modes::BRACKETED_PASTE),
                        2026 => self.modes.insert(Modes::SYNC_OUTPUT),
                        _ => {}
                    }
                }
                self.sink.effect(Effect::ModeChanged(*self.modes));
            }

            // RM / DECRST: Reset Mode
            ([], 'l') => {
                for &p in p_vec {
                    match p {
                        4 => self.modes.remove(Modes::INSERT),
                        20 => self.modes.remove(Modes::LINE_FEED_NEW_LINE),
                        _ => {}
                    }
                }
                self.sink.effect(Effect::ModeChanged(*self.modes));
            }

            ([b'?'], 'l') => {
                for &p in p_vec {
                    match p {
                        1 => self.modes.remove(Modes::DECCKM),
                        5 => self.modes.remove(Modes::REVERSE_VIDEO),
                        6 => {
                            self.modes.remove(Modes::ORIGIN);
                            let m = *self.modes;
                            let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                            g.set_cursor_pos(0, 0, m);
                        }
                        7 => self.modes.remove(Modes::AUTOWRAP),
                        9 => self.modes.remove(Modes::MOUSE_X10),
                        25 => self.modes.remove(Modes::CURSOR_VISIBLE),
                        47 | 1047 => {
                            if self.is_alt_screen {
                                self.is_alt_screen = false;
                                self.modes.remove(Modes::ALT_SCREEN);
                                self.damage.mark_all();
                            }
                        }
                        1049 => {
                            if self.is_alt_screen {
                                self.is_alt_screen = false;
                                self.modes.remove(Modes::ALT_SCREEN);
                                let (restored_style, new_modes) =
                                    self.grid.restore_cursor(*self.modes);
                                self.current_style_id = restored_style;
                                *self.modes = new_modes;
                                self.damage.mark_all();
                            }
                        }
                        1000 => self.modes.remove(Modes::MOUSE_NORMAL),
                        1002 => self.modes.remove(Modes::MOUSE_BUTTON),
                        1003 => self.modes.remove(Modes::MOUSE_ANY),
                        1004 => self.modes.remove(Modes::FOCUS_EVENTS),
                        1006 => self.modes.remove(Modes::MOUSE_SGR),
                        1007 => self.modes.remove(Modes::ALT_SCROLL),
                        1016 => self.modes.remove(Modes::MOUSE_SGR_PIXELS),
                        2004 => self.modes.remove(Modes::BRACKETED_PASTE),
                        2026 => {
                            self.modes.remove(Modes::SYNC_OUTPUT);
                            self.damage.mark_all();
                        }
                        _ => {}
                    }
                }
                self.sink.effect(Effect::ModeChanged(*self.modes));
            }

            // Kitty keyboard protocol
            ([b'>'], 'u') => {
                let flags = p_vec.first().copied().unwrap_or(0) as u8;
                let mode = p_vec.get(1).copied().unwrap_or(1) as u8;
                self.kitty.push(flags);
                self.kitty.set(flags, mode);
            }
            ([b'<'], 'u') => {
                let count = p_vec.first().copied().unwrap_or(1) as usize;
                self.kitty.pop(count);
            }
            ([b'='], 'u') => {
                let flags = p_vec.first().copied().unwrap_or(0) as u8;
                let mode = p_vec.get(1).copied().unwrap_or(1) as u8;
                self.kitty.set(flags, mode);
            }
            ([b'?'], 'u') => {
                let reply = format!("\x1b[?{}u", self.kitty.current_flags());
                self.sink.write_back(reply.as_bytes());
            }

            // DECSCUSR: Cursor shape
            ([b' '], 'q') => {
                let shape_code = p_vec.first().copied().unwrap_or(1);
                let (shape, blink) = match shape_code {
                    0 | 1 => (CursorShape::Block, true),
                    2 => (CursorShape::Block, false),
                    3 => (CursorShape::Underline, true),
                    4 => (CursorShape::Underline, false),
                    5 => (CursorShape::Bar, true),
                    6 => (CursorShape::Bar, false),
                    _ => (CursorShape::Block, true),
                };
                *self.cursor_shape = shape;
                *self.cursor_blink = blink;
                self.sink.effect(Effect::CursorShape(shape));
            }

            // DA1: Device Attributes
            ([], 'c') => {
                // VT220 with advanced video option & color
                self.sink.write_back(b"\x1b[?62;22c");
            }

            // DA2: Secondary Device Attributes
            ([b'>'], 'c') => {
                // Termin: firmware type 1, version 0.1.0 -> \x1b[>1;10;0c
                self.sink.write_back(b"\x1b[>1;10;0c");
            }

            // XTVERSION: CSI > q
            ([b'>'], 'q') => {
                self.sink.write_back(b"\x1bP>|termin(0.1.0)\x1b\\");
            }

            // DSR / CPR: Device Status Report
            ([], 'n') => {
                let p = p_vec.first().copied().unwrap_or(0);
                if p == 5 {
                    // Status report: OK
                    self.sink.write_back(b"\x1b[0n");
                } else if p == 6 {
                    // Cursor Position Report (respecting Origin Mode)
                    let (row, col) = {
                        let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                        if g.is_origin(modes) {
                            (
                                g.cursor_row.saturating_sub(g.top_margin) + 1,
                                g.cursor_col.saturating_sub(g.left_margin) + 1,
                            )
                        } else {
                            (g.cursor_row + 1, g.cursor_col + 1)
                        }
                    };
                    let reply = format!("\x1b[{};{}R", row, col);
                    self.sink.write_back(reply.as_bytes());
                }
            }

            // ANSI.SYS / xterm Save Cursor: CSI s
            ([], 's') => {
                let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                g.save_cursor(style, modes);
            }

            // ANSI.SYS / xterm Restore Cursor: CSI u
            ([], 'u') => {
                let (restored_style, new_modes) = {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    g.restore_cursor(modes)
                };
                self.current_style_id = restored_style;
                if let Some(s) = self.styles.get(restored_style) {
                    self.current_style = *s;
                }
                *self.modes = new_modes;
                self.damage.cursor_moved = true;
            }

            // DECRQM: Request Mode (CSI ? Pd $ p or CSI Pa $ p)
            ([b'?', b'$'], 'p') => {
                let p = p_vec.first().copied().unwrap_or(0);
                let status = match p {
                    1 => mode_status(self.modes.contains(Modes::DECCKM)),
                    5 => mode_status(self.modes.contains(Modes::REVERSE_VIDEO)),
                    6 => mode_status(self.modes.contains(Modes::ORIGIN)),
                    7 => mode_status(self.modes.contains(Modes::AUTOWRAP)),
                    9 => mode_status(self.modes.contains(Modes::MOUSE_X10)),
                    25 => mode_status(self.modes.contains(Modes::CURSOR_VISIBLE)),
                    47 | 1047 | 1049 => mode_status(self.is_alt_screen),
                    1000 => mode_status(self.modes.contains(Modes::MOUSE_NORMAL)),
                    1002 => mode_status(self.modes.contains(Modes::MOUSE_BUTTON)),
                    1003 => mode_status(self.modes.contains(Modes::MOUSE_ANY)),
                    1004 => mode_status(self.modes.contains(Modes::FOCUS_EVENTS)),
                    1006 => mode_status(self.modes.contains(Modes::MOUSE_SGR)),
                    1007 => mode_status(self.modes.contains(Modes::ALT_SCROLL)),
                    1016 => mode_status(self.modes.contains(Modes::MOUSE_SGR_PIXELS)),
                    2004 => mode_status(self.modes.contains(Modes::BRACKETED_PASTE)),
                    2026 => mode_status(self.modes.contains(Modes::SYNC_OUTPUT)),
                    _ => 0, // Unrecognized
                };
                let reply = format!("\x1b[?{};{}$y", p, status);
                self.sink.write_back(reply.as_bytes());
            }

            ([b'$'], 'p') => {
                let p = p_vec.first().copied().unwrap_or(0);
                let status = match p {
                    4 => mode_status(self.modes.contains(Modes::INSERT)),
                    20 => mode_status(self.modes.contains(Modes::LINE_FEED_NEW_LINE)),
                    _ => 0, // Unrecognized
                };
                let reply = format!("\x1b[{};{}$y", p, status);
                self.sink.write_back(reply.as_bytes());
            }

            // XTWINOPS: Window operations
            ([], 't') => {
                let p = p_vec.first().copied().unwrap_or(0);
                let (cols, rows) = {
                    let g = active_grid_mut(is_alt, self.grid, self.alt_grid);
                    (g.cols, g.num_rows)
                };
                let (cell_w, cell_h) = self.cfg.cell_px;

                match p {
                    14 => {
                        // Report window size in pixels
                        let reply = format!("\x1b[4;{};{}t", rows * cell_h, cols * cell_w);
                        self.sink.write_back(reply.as_bytes());
                    }
                    16 => {
                        // Report cell size in pixels
                        let reply = format!("\x1b[6;{};{}t", cell_h, cell_w);
                        self.sink.write_back(reply.as_bytes());
                    }
                    18 => {
                        // Report screen size in chars
                        let reply = format!("\x1b[8;{};{}t", rows, cols);
                        self.sink.write_back(reply.as_bytes());
                    }
                    _ => {}
                }
            }

            _ => {}
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        self.in_ground = bell_terminated;
        let mut link = self.current_style.link;
        let line = self.current_line();

        handle_osc(
            params,
            bell_terminated,
            self.sink,
            self.blocks,
            self.hyperlinks,
            &mut link,
            line,
            self.cfg.osc52,
            self.palette,
            self.default_fg,
            self.default_bg,
        );

        if link != self.current_style.link {
            self.current_style.link = link;
            self.update_style();
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        // DCS started
        self.dcs_active = action == 'q' || action == '+';
        self.dcs_bytes.clear();
    }

    fn put(&mut self, byte: u8) {
        if self.dcs_active && self.dcs_bytes.len() < 4096 {
            self.dcs_bytes.push(byte);
        }
    }

    fn unhook(&mut self) {
        self.in_ground = true;
        if self.dcs_active {
            // XTGETTCAP responses: DCS + q <ptyp> ST
            if let Ok(s) = std::str::from_utf8(&self.dcs_bytes) {
                if s.starts_with("+q") {
                    // Answer XTGETTCAP
                    let mut reply = String::from("\x1bP1+r");
                    reply.push_str("TN=7465726d696e;Co#256;RGB");
                    reply.push_str("\x1b\\");
                    self.sink.write_back(reply.as_bytes());
                }
            }
            self.dcs_active = false;
            self.dcs_bytes.clear();
        }
    }
}

impl<'a, S: EffectSink> ParserHandler<'a, S> {
    fn handle_sgr(&mut self, params: &Params) {
        if params.is_empty() {
            self.current_style = Style::default();
            self.update_style();
            return;
        }

        let mut iter = params.iter();
        while let Some(subparams) = iter.next() {
            if subparams.is_empty() {
                self.current_style = Style::default();
                continue;
            }

            let code = subparams[0];
            match code {
                0 => self.current_style = Style::default(),
                1 => self.current_style.attrs.insert(Attrs::BOLD),
                2 => self.current_style.attrs.insert(Attrs::DIM),
                3 => self.current_style.attrs.insert(Attrs::ITALIC),
                4 => {
                    if subparams.len() > 1 {
                        match subparams[1] {
                            0 => self.current_style.attrs.remove(
                                Attrs::UNDERLINE
                                    | Attrs::DOUBLE_UNDERLINE
                                    | Attrs::CURLY_UNDERLINE
                                    | Attrs::DOTTED_UNDERLINE
                                    | Attrs::DASHED_UNDERLINE,
                            ),
                            1 => self.current_style.attrs.insert(Attrs::UNDERLINE),
                            2 => self.current_style.attrs.insert(Attrs::DOUBLE_UNDERLINE),
                            3 => self.current_style.attrs.insert(Attrs::CURLY_UNDERLINE),
                            4 => self.current_style.attrs.insert(Attrs::DOTTED_UNDERLINE),
                            5 => self.current_style.attrs.insert(Attrs::DASHED_UNDERLINE),
                            _ => self.current_style.attrs.insert(Attrs::UNDERLINE),
                        }
                    } else {
                        self.current_style.attrs.insert(Attrs::UNDERLINE);
                    }
                }
                5 | 6 => self.current_style.attrs.insert(Attrs::BLINK),
                7 => self.current_style.attrs.insert(Attrs::INVERSE),
                8 => self.current_style.attrs.insert(Attrs::HIDDEN),
                9 => self.current_style.attrs.insert(Attrs::STRIKE),
                21 => self.current_style.attrs.insert(Attrs::DOUBLE_UNDERLINE),
                22 => self.current_style.attrs.remove(Attrs::BOLD | Attrs::DIM),
                23 => self.current_style.attrs.remove(Attrs::ITALIC),
                24 => self.current_style.attrs.remove(
                    Attrs::UNDERLINE
                        | Attrs::DOUBLE_UNDERLINE
                        | Attrs::CURLY_UNDERLINE
                        | Attrs::DOTTED_UNDERLINE
                        | Attrs::DASHED_UNDERLINE,
                ),
                25 => self.current_style.attrs.remove(Attrs::BLINK),
                27 => self.current_style.attrs.remove(Attrs::INVERSE),
                28 => self.current_style.attrs.remove(Attrs::HIDDEN),
                29 => self.current_style.attrs.remove(Attrs::STRIKE),
                53 => self.current_style.attrs.insert(Attrs::OVERLINE),
                55 => self.current_style.attrs.remove(Attrs::OVERLINE),

                // Standard FG colors 30..37
                30..=37 => self.current_style.fg = Color::Indexed((code - 30) as u8),
                // Extended FG color 38
                38 => {
                    if let Some(c) = parse_color(subparams, &mut iter) {
                        self.current_style.fg = c;
                    }
                }
                // Default FG
                39 => self.current_style.fg = Color::Default,

                // Standard BG colors 40..47
                40..=47 => self.current_style.bg = Color::Indexed((code - 40) as u8),
                // Extended BG color 48
                48 => {
                    if let Some(c) = parse_color(subparams, &mut iter) {
                        self.current_style.bg = c;
                    }
                }
                // Default BG
                49 => self.current_style.bg = Color::Default,

                // Underline color 58
                58 => {
                    if let Some(c) = parse_color(subparams, &mut iter) {
                        self.current_style.ul_color = c;
                    }
                }
                // Default Underline color
                59 => self.current_style.ul_color = Color::Default,

                // Bright FG colors 90..97
                90..=97 => self.current_style.fg = Color::Indexed((code - 90 + 8) as u8),
                // Bright BG colors 100..107
                100..=107 => self.current_style.bg = Color::Indexed((code - 100 + 8) as u8),

                _ => {}
            }
        }

        self.update_style();
    }
}

fn parse_color<'a>(subparams: &[u16], iter: &mut impl Iterator<Item = &'a [u16]>) -> Option<Color> {
    // Case 1: Subparameters within the same slice: [38, 5, n] or [38, 2, r, g, b] or [38, 2, _, r, g, b]
    if subparams.len() > 2 {
        if subparams[1] == 5 {
            return Some(Color::Indexed(subparams[2] as u8));
        } else if subparams[1] == 2 {
            if subparams.len() >= 6 {
                return Some(Color::Rgb(
                    subparams[3] as u8,
                    subparams[4] as u8,
                    subparams[5] as u8,
                ));
            } else if subparams.len() >= 5 {
                return Some(Color::Rgb(
                    subparams[2] as u8,
                    subparams[3] as u8,
                    subparams[4] as u8,
                ));
            }
        }
    }

    // Case 2: Standard semicolon-separated parameters: 38 ; 5 ; n or 38 ; 2 ; r ; g ; b
    let mode_slice = iter.next()?;
    let mode = *mode_slice.first()?;

    if mode == 5 {
        let color_slice = iter.next()?;
        let idx = *color_slice.first()? as u8;
        Some(Color::Indexed(idx))
    } else if mode == 2 {
        let r = *iter.next()?.first()? as u8;
        let g = *iter.next()?.first()? as u8;
        let b = *iter.next()?.first()? as u8;
        Some(Color::Rgb(r, g, b))
    } else {
        None
    }
}

#[inline]
fn mode_status(set: bool) -> u8 {
    if set {
        1
    } else {
        2
    }
}
