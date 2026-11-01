//! The `Action` enum: single mutation entry point for keys, menus, palette
//! and IPC (`docs/architecture.md §5`, `docs/specs/app-model-and-ui.md §7`).
//!
//! Textual form (config, palette, IPC): `snake_case` name, optionally with
//! parenthesised arguments: `select_tab(3)`, `focus_pane(left)`,
//! `resize_split(left,5)`, `move_workspace(2,0)`, `send_text(ls\r)`.
//! [`Action`]'s `Display` and [`Action::parse`] round-trip.

use std::fmt;
use termin_core::{PaneId, WindowId};

use crate::model::Direction;

/// User or programmatic action.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Action {
    // --- windows & workspaces ---
    /// Open a new OS window.
    NewWindow,
    /// Close the current window.
    CloseWindow,
    /// New workspace after the active one.
    NewWorkspace,
    /// Close the active workspace.
    CloseWorkspace,
    /// Rename the active workspace; `None` opens the inline editor,
    /// `Some("")` resets to auto naming.
    RenameWorkspace(Option<String>),
    /// Next workspace (wraps).
    NextWorkspace,
    /// Previous workspace (wraps).
    PrevWorkspace,
    /// Select workspace `n` (1-based).
    SelectWorkspace(usize),
    /// Move the active workspace to another window at `index`.
    MoveWorkspace {
        /// Destination window.
        to_window: WindowId,
        /// Position in the destination sidebar (clamped).
        index: usize,
    },
    /// Show/hide the sidebar.
    ToggleSidebar,
    // --- tabs ---
    /// New tab in the focused pane.
    NewTab,
    /// Close the focused tab.
    CloseTab,
    /// Rename the focused tab; `None` opens the editor, `Some("")` clears.
    RenameTab(Option<String>),
    /// Next tab in the focused pane (wraps).
    NextTab,
    /// Previous tab in the focused pane (wraps).
    PrevTab,
    /// Select tab `n` (1-based) in the focused pane.
    SelectTab(usize),
    /// Move the focused tab into another pane at `index`.
    MoveTab {
        /// Destination pane.
        to_pane: PaneId,
        /// Position in the destination strip (clamped).
        index: usize,
    },
    // --- panes ---
    /// Split the focused pane side by side.
    SplitRight,
    /// Split the focused pane stacked.
    SplitDown,
    /// Close the focused pane (and its tabs).
    ClosePane,
    /// Geometric focus navigation.
    FocusPane(Direction),
    /// Focus next pane in reading order (wraps).
    FocusNextPane,
    /// Focus previous pane in reading order (wraps).
    FocusPrevPane,
    /// Move the nearest divider in `dir` by `percent` points of ratio.
    ResizeSplit {
        /// Direction the divider moves.
        dir: Direction,
        /// Amount in percent (default keyboard step 5).
        percent: u8,
    },
    /// Equalize all splits.
    EqualizeSplits,
    /// Toggle zoom of the focused pane.
    ZoomPane,
    /// Swap the focused pane with its neighbour in `dir`.
    SwapPane(Direction),
    // --- edit / find ---
    /// Copy selection.
    Copy,
    /// Paste clipboard.
    Paste,
    /// Select all.
    SelectAll,
    /// Clear buffer / hide finished blocks.
    ClearBuffer,
    /// Open find bar.
    Find,
    /// Next match.
    FindNext,
    /// Previous match.
    FindPrev,
    // --- scrolling & blocks ---
    /// Scroll one page up.
    ScrollPageUp,
    /// Scroll one page down.
    ScrollPageDown,
    /// Scroll to top of scrollback.
    ScrollToTop,
    /// Scroll to bottom.
    ScrollToBottom,
    /// Scroll to previous block/prompt.
    ScrollToPrevBlock,
    /// Scroll to next block/prompt.
    ScrollToNextBlock,
    /// Select previous block.
    SelectPrevBlock,
    /// Select next block.
    SelectNextBlock,
    /// Copy selected block (command + output).
    CopyBlock,
    /// Copy selected block's output.
    CopyBlockOutput,
    /// Copy selected block's command.
    CopyBlockCommand,
    /// Re-run selected block's command.
    RerunBlock,
    /// Toggle block ↔ classic mode.
    ToggleBlockMode,
    // --- overlays ---
    /// Action palette (⌘⇧P).
    CommandPalette,
    /// Go-to palette (⌘P).
    GotoPalette,
    /// History search (⌃R).
    HistorySearch,
    // --- app ---
    /// Increase font size.
    FontIncrease,
    /// Decrease font size.
    FontDecrease,
    /// Reset font size.
    FontReset,
    /// Re-read `config.toml`.
    ReloadConfig,
    /// Open `config.toml` in the default editor.
    OpenConfig,
    /// Jump to the most recent unread attention.
    JumpToAttention,
    /// Clear attention of the focused tab.
    ClearAttention,
    /// Write text to the focused tab's PTY.
    SendText(String),
    /// Install the `termin` CLI symlink into PATH (`/usr/local/bin/termin` or `~/.local/bin/termin`).
    InstallCli,
    /// Quit the application.
    Quit,
}

/// Which subsystem owns the implementation of an action (routing in
/// [`crate::dispatch`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Area {
    /// Model: windows (and quit).
    Windows,
    /// Model: workspaces/sidebar.
    Workspaces,
    /// Model: tabs.
    Tabs,
    /// Model: panes/splits.
    Panes,
    /// Terminal view (copy/paste/scroll/font/clear): track 2A + 3B.
    Terminal,
    /// Blocks & block-mode (3B).
    Blocks,
    /// Input editor / history search (3B).
    Editor,
    /// Palette overlays (3C).
    Palette,
    /// Find bar (3C).
    Find,
    /// Attention (3D).
    Attention,
    /// Config reload/open (3E).
    Config,
    /// Direct PTY input.
    Input,
}

/// Failure to parse an action string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionParseError {
    /// Unknown action name.
    Unknown(String),
    /// Wrong/missing/invalid arguments for a known action.
    BadArgs(&'static str),
}

impl fmt::Display for ActionParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(n) => write!(f, "unknown action '{n}'"),
            Self::BadArgs(n) => write!(f, "invalid arguments for action '{n}'"),
        }
    }
}

impl std::error::Error for ActionParseError {}

/// Palette/menu entry describing an action.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionInfo {
    /// Representative action (default arguments for parameterised ones).
    pub action: Action,
    /// Human title for the palette.
    pub title: &'static str,
}

impl Action {
    /// Canonical `snake_case` name without arguments.
    pub fn name(&self) -> &'static str {
        use Action::*;
        match self {
            NewWindow => "new_window",
            CloseWindow => "close_window",
            NewWorkspace => "new_workspace",
            CloseWorkspace => "close_workspace",
            RenameWorkspace(_) => "rename_workspace",
            NextWorkspace => "next_workspace",
            PrevWorkspace => "prev_workspace",
            SelectWorkspace(_) => "select_workspace",
            MoveWorkspace { .. } => "move_workspace",
            ToggleSidebar => "toggle_sidebar",
            NewTab => "new_tab",
            CloseTab => "close_tab",
            RenameTab(_) => "rename_tab",
            NextTab => "next_tab",
            PrevTab => "prev_tab",
            SelectTab(_) => "select_tab",
            MoveTab { .. } => "move_tab",
            SplitRight => "split_right",
            SplitDown => "split_down",
            ClosePane => "close_pane",
            FocusPane(_) => "focus_pane",
            FocusNextPane => "focus_next_pane",
            FocusPrevPane => "focus_prev_pane",
            ResizeSplit { .. } => "resize_split",
            EqualizeSplits => "equalize_splits",
            ZoomPane => "zoom_pane",
            SwapPane(_) => "swap_pane",
            Copy => "copy",
            Paste => "paste",
            SelectAll => "select_all",
            ClearBuffer => "clear_buffer",
            Find => "find",
            FindNext => "find_next",
            FindPrev => "find_prev",
            ScrollPageUp => "scroll_page_up",
            ScrollPageDown => "scroll_page_down",
            ScrollToTop => "scroll_to_top",
            ScrollToBottom => "scroll_to_bottom",
            ScrollToPrevBlock => "scroll_to_prev_block",
            ScrollToNextBlock => "scroll_to_next_block",
            SelectPrevBlock => "select_prev_block",
            SelectNextBlock => "select_next_block",
            CopyBlock => "copy_block",
            CopyBlockOutput => "copy_block_output",
            CopyBlockCommand => "copy_block_command",
            RerunBlock => "rerun_block",
            ToggleBlockMode => "toggle_block_mode",
            CommandPalette => "command_palette",
            GotoPalette => "goto_palette",
            HistorySearch => "history_search",
            FontIncrease => "font_increase",
            FontDecrease => "font_decrease",
            FontReset => "font_reset",
            ReloadConfig => "reload_config",
            OpenConfig => "open_config",
            JumpToAttention => "jump_to_attention",
            ClearAttention => "clear_attention",
            SendText(_) => "send_text",
            InstallCli => "install_cli",
            Quit => "quit",
        }
    }

    /// Subsystem that implements this action.
    pub fn area(&self) -> Area {
        use Action::*;
        match self {
            NewWindow | CloseWindow | Quit => Area::Windows,
            NewWorkspace
            | CloseWorkspace
            | RenameWorkspace(_)
            | NextWorkspace
            | PrevWorkspace
            | SelectWorkspace(_)
            | MoveWorkspace { .. }
            | ToggleSidebar => Area::Workspaces,
            NewTab
            | CloseTab
            | RenameTab(_)
            | NextTab
            | PrevTab
            | SelectTab(_)
            | MoveTab { .. } => Area::Tabs,
            SplitRight
            | SplitDown
            | ClosePane
            | FocusPane(_)
            | FocusNextPane
            | FocusPrevPane
            | ResizeSplit { .. }
            | EqualizeSplits
            | ZoomPane
            | SwapPane(_) => Area::Panes,
            Copy | Paste | SelectAll | ClearBuffer | ScrollPageUp | ScrollPageDown
            | ScrollToTop | ScrollToBottom | FontIncrease | FontDecrease | FontReset => {
                Area::Terminal
            }
            ScrollToPrevBlock | ScrollToNextBlock | SelectPrevBlock | SelectNextBlock
            | CopyBlock | CopyBlockOutput | CopyBlockCommand | RerunBlock | ToggleBlockMode => {
                Area::Blocks
            }
            HistorySearch => Area::Editor,
            CommandPalette | GotoPalette => Area::Palette,
            Find | FindNext | FindPrev => Area::Find,
            JumpToAttention | ClearAttention => Area::Attention,
            ReloadConfig | OpenConfig | InstallCli => Area::Config,
            SendText(_) => Area::Input,
        }
    }

    /// Parse `name` or `name(args)`.
    pub fn parse(s: &str) -> Result<Action, ActionParseError> {
        let s = s.trim();
        let (name, args) = match s.find('(') {
            Some(i) if s.ends_with(')') => (&s[..i], Some(&s[i + 1..s.len() - 1])),
            _ => (s, None),
        };
        let name = name.trim();
        let bad = |n: &'static str| ActionParseError::BadArgs(n);
        let parts = |a: Option<&str>| -> Vec<String> {
            a.map(|a| a.split(',').map(|p| p.trim().to_string()).collect())
                .unwrap_or_default()
        };
        let no_args = |a: Option<&Action>, n: &'static str| -> Result<(), ActionParseError> {
            let _ = a;
            match args {
                None => Ok(()),
                Some(x) if x.trim().is_empty() => Ok(()),
                Some(_) => Err(bad(n)),
            }
        };
        use Action::*;
        let simple = |a: Action| -> Result<Action, ActionParseError> {
            no_args(None, a.name())?;
            Ok(a)
        };
        let dir_arg = |n: &'static str| -> Result<Direction, ActionParseError> {
            let p = parts(args);
            match p.as_slice() {
                [d] => Direction::parse(d).ok_or(bad(n)),
                _ => Err(bad(n)),
            }
        };
        match name {
            "new_window" => simple(NewWindow),
            "close_window" => simple(CloseWindow),
            "new_workspace" => simple(NewWorkspace),
            "close_workspace" => simple(CloseWorkspace),
            "rename_workspace" => Ok(RenameWorkspace(args.map(str::to_string))),
            "next_workspace" => simple(NextWorkspace),
            "prev_workspace" => simple(PrevWorkspace),
            "select_workspace" => match parts(args).as_slice() {
                [n] => n
                    .parse()
                    .map(SelectWorkspace)
                    .map_err(|_| bad("select_workspace")),
                _ => Err(bad("select_workspace")),
            },
            "move_workspace" => match parts(args).as_slice() {
                [w, i] => match (w.parse(), i.parse()) {
                    (Ok(w), Ok(index)) => Ok(MoveWorkspace {
                        to_window: WindowId(w),
                        index,
                    }),
                    _ => Err(bad("move_workspace")),
                },
                _ => Err(bad("move_workspace")),
            },
            "toggle_sidebar" => simple(ToggleSidebar),
            "new_tab" => simple(NewTab),
            "close_tab" => simple(CloseTab),
            "rename_tab" => Ok(RenameTab(args.map(str::to_string))),
            "next_tab" => simple(NextTab),
            "prev_tab" => simple(PrevTab),
            "select_tab" => match parts(args).as_slice() {
                [n] => n.parse().map(SelectTab).map_err(|_| bad("select_tab")),
                _ => Err(bad("select_tab")),
            },
            "move_tab" => match parts(args).as_slice() {
                [p, i] => match (p.parse(), i.parse()) {
                    (Ok(p), Ok(index)) => Ok(MoveTab {
                        to_pane: PaneId(p),
                        index,
                    }),
                    _ => Err(bad("move_tab")),
                },
                _ => Err(bad("move_tab")),
            },
            "split_right" => simple(SplitRight),
            "split_down" => simple(SplitDown),
            "close_pane" => simple(ClosePane),
            "focus_pane" => dir_arg("focus_pane").map(FocusPane),
            "focus_next_pane" => simple(FocusNextPane),
            "focus_prev_pane" => simple(FocusPrevPane),
            "resize_split" => match parts(args).as_slice() {
                [d] => Direction::parse(d)
                    .map(|dir| ResizeSplit { dir, percent: 5 })
                    .ok_or(bad("resize_split")),
                [d, n] => match (Direction::parse(d), n.parse::<u8>()) {
                    (Some(dir), Ok(percent)) => Ok(ResizeSplit { dir, percent }),
                    _ => Err(bad("resize_split")),
                },
                _ => Err(bad("resize_split")),
            },
            "equalize_splits" => simple(EqualizeSplits),
            "zoom_pane" => simple(ZoomPane),
            "swap_pane" => dir_arg("swap_pane").map(SwapPane),
            "copy" => simple(Copy),
            "paste" => simple(Paste),
            "select_all" => simple(SelectAll),
            "clear_buffer" => simple(ClearBuffer),
            "find" => simple(Find),
            "find_next" => simple(FindNext),
            "find_prev" => simple(FindPrev),
            "scroll_page_up" => simple(ScrollPageUp),
            "scroll_page_down" => simple(ScrollPageDown),
            "scroll_to_top" => simple(ScrollToTop),
            "scroll_to_bottom" => simple(ScrollToBottom),
            "scroll_to_prev_block" => simple(ScrollToPrevBlock),
            "scroll_to_next_block" => simple(ScrollToNextBlock),
            "select_prev_block" => simple(SelectPrevBlock),
            "select_next_block" => simple(SelectNextBlock),
            "copy_block" => simple(CopyBlock),
            "copy_block_output" => simple(CopyBlockOutput),
            "copy_block_command" => simple(CopyBlockCommand),
            "rerun_block" => simple(RerunBlock),
            "toggle_block_mode" => simple(ToggleBlockMode),
            "command_palette" => simple(CommandPalette),
            "goto_palette" => simple(GotoPalette),
            "history_search" => simple(HistorySearch),
            "font_increase" => simple(FontIncrease),
            "font_decrease" => simple(FontDecrease),
            "font_reset" => simple(FontReset),
            "reload_config" => simple(ReloadConfig),
            "open_config" => simple(OpenConfig),
            "jump_to_attention" => simple(JumpToAttention),
            "clear_attention" => simple(ClearAttention),
            "send_text" => Ok(SendText(args.unwrap_or("").to_string())),
            "install_cli" => simple(InstallCli),
            "quit" => simple(Quit),
            other => Err(ActionParseError::Unknown(other.to_string())),
        }
    }

    /// Every action exposed to the palette, with human titles. Parameterised
    /// actions appear with representative arguments (`select_workspace(1..9)`
    /// excluded; those are reachable via keymap/IPC).
    pub fn catalog() -> Vec<ActionInfo> {
        use Action::*;
        use Direction::*;
        let mut v: Vec<(Action, &'static str)> = vec![
            (NewWindow, "New Window"),
            (CloseWindow, "Close Window"),
            (NewWorkspace, "New Workspace"),
            (CloseWorkspace, "Close Workspace"),
            (RenameWorkspace(None), "Rename Workspace…"),
            (NextWorkspace, "Next Workspace"),
            (PrevWorkspace, "Previous Workspace"),
            (ToggleSidebar, "Toggle Sidebar"),
            (NewTab, "New Tab"),
            (CloseTab, "Close Tab"),
            (RenameTab(None), "Rename Tab…"),
            (NextTab, "Next Tab"),
            (PrevTab, "Previous Tab"),
            (SplitRight, "Split Right"),
            (SplitDown, "Split Down"),
            (ClosePane, "Close Pane"),
            (FocusNextPane, "Focus Next Pane"),
            (FocusPrevPane, "Focus Previous Pane"),
            (EqualizeSplits, "Equalize Splits"),
            (ZoomPane, "Zoom Pane"),
            (Copy, "Copy"),
            (Paste, "Paste"),
            (SelectAll, "Select All"),
            (ClearBuffer, "Clear Buffer"),
            (Find, "Find"),
            (FindNext, "Find Next"),
            (FindPrev, "Find Previous"),
            (ScrollPageUp, "Scroll Page Up"),
            (ScrollPageDown, "Scroll Page Down"),
            (ScrollToTop, "Scroll to Top"),
            (ScrollToBottom, "Scroll to Bottom"),
            (ScrollToPrevBlock, "Scroll to Previous Block"),
            (ScrollToNextBlock, "Scroll to Next Block"),
            (SelectPrevBlock, "Select Previous Block"),
            (SelectNextBlock, "Select Next Block"),
            (CopyBlock, "Copy Block"),
            (CopyBlockOutput, "Copy Block Output"),
            (CopyBlockCommand, "Copy Block Command"),
            (RerunBlock, "Re-run Block"),
            (ToggleBlockMode, "Toggle Block Mode"),
            (CommandPalette, "Command Palette"),
            (GotoPalette, "Go to…"),
            (HistorySearch, "Search History"),
            (FontIncrease, "Increase Font Size"),
            (FontDecrease, "Decrease Font Size"),
            (FontReset, "Reset Font Size"),
            (ReloadConfig, "Reload Config"),
            (OpenConfig, "Open Config File"),
            (InstallCli, "Install CLI in PATH"),
            (JumpToAttention, "Jump to Latest Attention"),
            (ClearAttention, "Clear Attention"),
            (Quit, "Quit Termin"),
        ];
        for (d, t) in [
            (Left, "Focus Pane Left"),
            (Right, "Focus Pane Right"),
            (Up, "Focus Pane Up"),
            (Down, "Focus Pane Down"),
        ] {
            v.push((FocusPane(d), t));
        }
        for (d, t) in [
            (Left, "Resize Split Left"),
            (Right, "Resize Split Right"),
            (Up, "Resize Split Up"),
            (Down, "Resize Split Down"),
        ] {
            v.push((ResizeSplit { dir: d, percent: 5 }, t));
        }
        for (d, t) in [
            (Left, "Swap Pane Left"),
            (Right, "Swap Pane Right"),
            (Up, "Swap Pane Up"),
            (Down, "Swap Pane Down"),
        ] {
            v.push((SwapPane(d), t));
        }
        v.into_iter()
            .map(|(action, title)| ActionInfo { action, title })
            .collect()
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Action::*;
        let n = self.name();
        match self {
            RenameWorkspace(Some(s)) | RenameTab(Some(s)) | SendText(s) => write!(f, "{n}({s})"),
            SelectWorkspace(i) | SelectTab(i) => write!(f, "{n}({i})"),
            MoveWorkspace { to_window, index } => write!(f, "{n}({},{index})", to_window.0),
            MoveTab { to_pane, index } => write!(f, "{n}({},{index})", to_pane.0),
            FocusPane(d) | SwapPane(d) => write!(f, "{n}({})", d.name()),
            ResizeSplit { dir, percent } => write!(f, "{n}({},{percent})", dir.name()),
            _ => f.write_str(n),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_round_trips_through_text() {
        let cat = Action::catalog();
        assert!(cat.len() > 50);
        for info in cat {
            let s = info.action.to_string();
            assert_eq!(Action::parse(&s), Ok(info.action.clone()), "{s}");
            assert!(!info.title.is_empty());
        }
    }

    #[test]
    fn parameterised_round_trip() {
        for a in [
            Action::SelectTab(3),
            Action::SelectWorkspace(9),
            Action::MoveWorkspace {
                to_window: WindowId(2),
                index: 0,
            },
            Action::MoveTab {
                to_pane: PaneId(5),
                index: 1,
            },
            Action::ResizeSplit {
                dir: Direction::Left,
                percent: 10,
            },
            Action::SendText("clear\r".into()),
            Action::SendText("a,b (c)".into()),
            Action::RenameTab(Some("x".into())),
            Action::RenameWorkspace(None),
        ] {
            assert_eq!(Action::parse(&a.to_string()), Ok(a));
        }
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        assert!(matches!(
            Action::parse("nope"),
            Err(ActionParseError::Unknown(_))
        ));
        assert!(Action::parse("select_tab(x)").is_err());
        assert!(Action::parse("focus_pane(sideways)").is_err());
        assert!(Action::parse("split_right(1)").is_err());
        assert!(Action::parse("").is_err());
        assert!(Action::parse("(").is_err());
        assert!(Action::parse("select_tab(").is_err());
        assert_eq!(
            Action::parse("resize_split(up)"),
            Ok(Action::ResizeSplit {
                dir: Direction::Up,
                percent: 5
            })
        );
    }

    #[test]
    fn every_name_has_an_area() {
        for info in Action::catalog() {
            let _ = info.action.area();
        }
    }
}
