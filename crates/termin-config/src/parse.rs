//! Configuration parser and loader with diagnostic collection.
//!
//! See `docs/specs/config.md §2, §5`.

use crate::chord::validate_keybinding_entry;
use crate::color::parse_color;
use crate::diagnostic::Diagnostic;
use crate::schema::*;
use std::collections::HashMap;
use std::path::Path;

const KNOWN_SECTIONS: &[&str] = &[
    "font",
    "theme",
    "window",
    "sidebar",
    "terminal",
    "shell",
    "blocks",
    "editor",
    "split",
    "pane",
    "notifications",
    "keybindings",
    "colors",
];

/// Parse configuration from a TOML string with permissive default action validator.
///
/// Returns the parsed (or default) configuration and a list of diagnostics.
/// Parsing never fails or panics; defaults are used for any missing or invalid keys.
pub fn parse(src: &str) -> (Config, Vec<Diagnostic>) {
    parse_with_action_validator(src, |_| true)
}

/// Load configuration from a file path with default action validator.
///
/// Missing file returns defaults with no diagnostics.
pub fn load(path: &Path) -> (Config, Vec<Diagnostic>) {
    load_with_action_validator(path, |_| true)
}

/// Load configuration from a file path with injected action name validator.
pub fn load_with_action_validator<F>(
    path: &Path,
    is_valid_action: F,
) -> (Config, Vec<Diagnostic>)
where
    F: Fn(&str) -> bool,
{
    if !path.exists() {
        return (Config::default(), Vec::new());
    }
    match std::fs::read_to_string(path) {
        Ok(content) => parse_with_action_validator(&content, is_valid_action),
        Err(e) => {
            let diag = Diagnostic::error(format!(
                "failed to read config file {}: {}",
                path.display(),
                e
            ));
            (Config::default(), vec![diag])
        }
    }
}

/// Parse configuration from a TOML string with an injected action name validation function.
///
/// See `docs/specs/config.md §2, §5`.
pub fn parse_with_action_validator<F>(
    src: &str,
    is_valid_action: F,
) -> (Config, Vec<Diagnostic>)
where
    F: Fn(&str) -> bool,
{
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return (Config::default(), Vec::new());
    }

    let toml_val: toml::Value = match toml::from_str(src) {
        Ok(v) => v,
        Err(e) => {
            let mut diag = Diagnostic::error(format!("TOML syntax error: {}", e));
            if let Some(span) = e.span() {
                let up_to_start = &src[..span.start.min(src.len())];
                let line = up_to_start.lines().count().max(1);
                let col = up_to_start.lines().last().map(|l| l.len() + 1).unwrap_or(1);
                diag = diag.with_location(line, col);
            }
            return (Config::default(), vec![diag]);
        }
    };

    let table = match toml_val.as_table() {
        Some(t) => t,
        None => {
            return (
                Config::default(),
                vec![Diagnostic::error("configuration root must be a table")],
            );
        }
    };

    let mut cfg = Config::default();
    let mut diags = Vec::new();

    // Check for unknown top-level sections or keys
    for k in table.keys() {
        if !KNOWN_SECTIONS.contains(&k.as_str()) {
            diags.push(
                Diagnostic::warning(format!("unknown configuration section or key '{}'", k))
                    .with_key(k),
            );
        }
    }

    // 1. [font]
    if let Some(val) = table.get("font") {
        parse_font_section(val, &mut cfg.font, &mut diags);
    }

    // 2. [theme]
    if let Some(val) = table.get("theme") {
        parse_theme_section(val, &mut cfg.theme, &mut diags);
    }

    // 3. [window]
    if let Some(val) = table.get("window") {
        parse_window_section(val, &mut cfg.window, &mut diags);
    }

    // 4. [sidebar]
    if let Some(val) = table.get("sidebar") {
        parse_sidebar_section(val, &mut cfg.sidebar, &mut diags);
    }

    // 5. [terminal]
    if let Some(val) = table.get("terminal") {
        parse_terminal_section(val, &mut cfg.terminal, &mut diags);
    }

    // 6. [shell]
    if let Some(val) = table.get("shell") {
        parse_shell_section(val, &mut cfg.shell, &mut diags);
    }

    // 7. [blocks]
    if let Some(val) = table.get("blocks") {
        parse_blocks_section(val, &mut cfg.blocks, &mut diags);
    }

    // 8. [editor]
    if let Some(val) = table.get("editor") {
        parse_editor_section(val, &mut cfg.editor, &mut diags);
    }

    // 9. [split]
    if let Some(val) = table.get("split") {
        parse_split_section(val, &mut cfg.split, &mut diags);
    }

    // 10. [pane]
    if let Some(val) = table.get("pane") {
        parse_pane_section(val, &mut cfg.pane, &mut diags);
    }

    // 11. [notifications]
    if let Some(val) = table.get("notifications") {
        parse_notifications_section(val, &mut cfg.notifications, &mut diags);
    }

    // 12. [keybindings]
    if let Some(val) = table.get("keybindings") {
        parse_keybindings_section(val, &mut cfg.keybindings, &is_valid_action, &mut diags);
    }

    // 13. [colors]
    if let Some(val) = table.get("colors") {
        parse_colors_section(val, &mut cfg.colors, &mut diags);
    }

    (cfg, diags)
}

fn as_f32(val: &toml::Value) -> Option<f32> {
    match val {
        toml::Value::Float(f) => Some(*f as f32),
        toml::Value::Integer(i) => Some(*i as f32),
        _ => None,
    }
}

fn parse_font_section(val: &toml::Value, font: &mut FontConfig, diags: &mut Vec<Diagnostic>) {
    let tbl = match val.as_table() {
        Some(t) => t,
        None => {
            diags.push(
                Diagnostic::error("section 'font' must be a table").with_key("font"),
            );
            return;
        }
    };

    const KNOWN: &[&str] = &[
        "family",
        "size",
        "line_height",
        "ligatures",
        "thicken",
        "bold_is_bright",
    ];
    for (k, v) in tbl {
        match k.as_str() {
            "family" => match v.as_str() {
                Some(s) => font.family = s.to_string(),
                None => diags.push(
                    Diagnostic::error("wrong type for 'font.family', expected string")
                        .with_key("font.family"),
                ),
            },
            "size" => match as_f32(v) {
                Some(f) if f > 0.0 => font.size = f,
                _ => diags.push(
                    Diagnostic::error("wrong type for 'font.size', expected positive number")
                        .with_key("font.size"),
                ),
            },
            "line_height" => match as_f32(v) {
                Some(f) if f > 0.0 => font.line_height = f,
                _ => diags.push(
                    Diagnostic::error(
                        "wrong type for 'font.line_height', expected positive number",
                    )
                    .with_key("font.line_height"),
                ),
            },
            "ligatures" => match v.as_bool() {
                Some(b) => font.ligatures = b,
                None => diags.push(
                    Diagnostic::error("wrong type for 'font.ligatures', expected boolean")
                        .with_key("font.ligatures"),
                ),
            },
            "thicken" => match v.as_bool() {
                Some(b) => font.thicken = b,
                None => diags.push(
                    Diagnostic::error("wrong type for 'font.thicken', expected boolean")
                        .with_key("font.thicken"),
                ),
            },
            "bold_is_bright" => match v.as_bool() {
                Some(b) => font.bold_is_bright = b,
                None => diags.push(
                    Diagnostic::error(
                        "wrong type for 'font.bold_is_bright', expected boolean",
                    )
                    .with_key("font.bold_is_bright"),
                ),
            },
            other => {
                if !KNOWN.contains(&other) {
                    diags.push(
                        Diagnostic::warning(format!("unknown key 'font.{}'", other))
                            .with_key(format!("font.{}", other)),
                    );
                }
            }
        }
    }
}

fn parse_theme_section(val: &toml::Value, theme: &mut ThemeConfig, diags: &mut Vec<Diagnostic>) {
    let tbl = match val.as_table() {
        Some(t) => t,
        None => {
            diags.push(
                Diagnostic::error("section 'theme' must be a table").with_key("theme"),
            );
            return;
        }
    };

    const KNOWN: &[&str] = &["name", "light", "dark"];
    for (k, v) in tbl {
        match k.as_str() {
            "name" => match v.as_str() {
                Some(s) => theme.name = s.to_string(),
                None => diags.push(
                    Diagnostic::error("wrong type for 'theme.name', expected string")
                        .with_key("theme.name"),
                ),
            },
            "light" => match v.as_str() {
                Some(s) => theme.light = s.to_string(),
                None => diags.push(
                    Diagnostic::error("wrong type for 'theme.light', expected string")
                        .with_key("theme.light"),
                ),
            },
            "dark" => match v.as_str() {
                Some(s) => theme.dark = s.to_string(),
                None => diags.push(
                    Diagnostic::error("wrong type for 'theme.dark', expected string")
                        .with_key("theme.dark"),
                ),
            },
            other => {
                if !KNOWN.contains(&other) {
                    diags.push(
                        Diagnostic::warning(format!("unknown key 'theme.{}'", other))
                            .with_key(format!("theme.{}", other)),
                    );
                }
            }
        }
    }
}

fn parse_window_section(val: &toml::Value, win: &mut WindowConfig, diags: &mut Vec<Diagnostic>) {
    let tbl = match val.as_table() {
        Some(t) => t,
        None => {
            diags.push(
                Diagnostic::error("section 'window' must be a table").with_key("window"),
            );
            return;
        }
    };

    const KNOWN: &[&str] = &[
        "padding",
        "opacity",
        "blur",
        "restore_session",
        "quit_on_last_window_close",
        "colorspace",
    ];
    for (k, v) in tbl {
        match k.as_str() {
            "padding" => match v.as_array() {
                Some(arr) if arr.len() == 2 => {
                    let p0 = as_f32(&arr[0]);
                    let p1 = as_f32(&arr[1]);
                    match (p0, p1) {
                        (Some(v0), Some(v1)) => win.padding = [v0, v1],
                        _ => diags.push(
                            Diagnostic::error(
                                "wrong element type for 'window.padding', expected numbers",
                            )
                            .with_key("window.padding"),
                        ),
                    }
                }
                _ => diags.push(
                    Diagnostic::error(
                        "wrong type for 'window.padding', expected [vertical, horizontal] numbers",
                    )
                    .with_key("window.padding"),
                ),
            },
            "opacity" => match as_f32(v) {
                Some(f) => win.opacity = f.clamp(0.0, 1.0),
                None => diags.push(
                    Diagnostic::error("wrong type for 'window.opacity', expected number")
                        .with_key("window.opacity"),
                ),
            },
            "blur" => match v.as_bool() {
                Some(b) => win.blur = b,
                None => diags.push(
                    Diagnostic::error("wrong type for 'window.blur', expected boolean")
                        .with_key("window.blur"),
                ),
            },
            "restore_session" => match v.as_bool() {
                Some(b) => win.restore_session = b,
                None => diags.push(
                    Diagnostic::error(
                        "wrong type for 'window.restore_session', expected boolean",
                    )
                    .with_key("window.restore_session"),
                ),
            },
            "quit_on_last_window_close" => match v.as_bool() {
                Some(b) => win.quit_on_last_window_close = b,
                None => diags.push(
                    Diagnostic::error(
                        "wrong type for 'window.quit_on_last_window_close', expected boolean",
                    )
                    .with_key("window.quit_on_last_window_close"),
                ),
            },
            "colorspace" => match v.as_str() {
                Some(s) => win.colorspace = s.to_string(),
                None => diags.push(
                    Diagnostic::error("wrong type for 'window.colorspace', expected string")
                        .with_key("window.colorspace"),
                ),
            },
            other => {
                if !KNOWN.contains(&other) {
                    diags.push(
                        Diagnostic::warning(format!("unknown key 'window.{}'", other))
                            .with_key(format!("window.{}", other)),
                    );
                }
            }
        }
    }
}

fn parse_sidebar_section(
    val: &toml::Value,
    side: &mut SidebarConfig,
    diags: &mut Vec<Diagnostic>,
) {
    let tbl = match val.as_table() {
        Some(t) => t,
        None => {
            diags.push(
                Diagnostic::error("section 'sidebar' must be a table").with_key("sidebar"),
            );
            return;
        }
    };

    const KNOWN: &[&str] = &[
        "visible",
        "width",
        "show_git",
        "show_ports",
        "show_paths",
    ];
    for (k, v) in tbl {
        match k.as_str() {
            "visible" => match v.as_bool() {
                Some(b) => side.visible = b,
                None => diags.push(
                    Diagnostic::error("wrong type for 'sidebar.visible', expected boolean")
                        .with_key("sidebar.visible"),
                ),
            },
            "width" => match as_f32(v) {
                Some(w) => side.width = w.clamp(180.0, 400.0),
                None => diags.push(
                    Diagnostic::error("wrong type for 'sidebar.width', expected number")
                        .with_key("sidebar.width"),
                ),
            },
            "show_git" => match v.as_bool() {
                Some(b) => side.show_git = b,
                None => diags.push(
                    Diagnostic::error("wrong type for 'sidebar.show_git', expected boolean")
                        .with_key("sidebar.show_git"),
                ),
            },
            "show_ports" => match v.as_bool() {
                Some(b) => side.show_ports = b,
                None => diags.push(
                    Diagnostic::error("wrong type for 'sidebar.show_ports', expected boolean")
                        .with_key("sidebar.show_ports"),
                ),
            },
            "show_paths" => match v.as_bool() {
                Some(b) => side.show_paths = b,
                None => diags.push(
                    Diagnostic::error("wrong type for 'sidebar.show_paths', expected boolean")
                        .with_key("sidebar.show_paths"),
                ),
            },
            other => {
                if !KNOWN.contains(&other) {
                    diags.push(
                        Diagnostic::warning(format!("unknown key 'sidebar.{}'", other))
                            .with_key(format!("sidebar.{}", other)),
                    );
                }
            }
        }
    }
}

fn parse_terminal_section(
    val: &toml::Value,
    term: &mut TerminalConfig,
    diags: &mut Vec<Diagnostic>,
) {
    let tbl = match val.as_table() {
        Some(t) => t,
        None => {
            diags.push(
                Diagnostic::error("section 'terminal' must be a table").with_key("terminal"),
            );
            return;
        }
    };

    const KNOWN: &[&str] = &[
        "scrollback_lines",
        "cursor_style",
        "cursor_blink",
        "option_as_alt",
        "word_separators",
        "osc52",
        "warn_multiline_paste",
        "minimum_contrast",
        "confirm_close_ignore",
    ];
    for (k, v) in tbl {
        match k.as_str() {
            "scrollback_lines" => match v.as_integer() {
                Some(i) if i >= 0 => term.scrollback_lines = i as u32,
                _ => diags.push(
                    Diagnostic::error(
                        "wrong type for 'terminal.scrollback_lines', expected non-negative integer",
                    )
                    .with_key("terminal.scrollback_lines"),
                ),
            },
            "cursor_style" => match v.as_str() {
                Some(s) => term.cursor_style = s.to_string(),
                None => diags.push(
                    Diagnostic::error("wrong type for 'terminal.cursor_style', expected string")
                        .with_key("terminal.cursor_style"),
                ),
            },
            "cursor_blink" => match v.as_bool() {
                Some(b) => term.cursor_blink = b,
                None => diags.push(
                    Diagnostic::error("wrong type for 'terminal.cursor_blink', expected boolean")
                        .with_key("terminal.cursor_blink"),
                ),
            },
            "option_as_alt" => match v.as_str() {
                Some(s) => term.option_as_alt = s.to_string(),
                None => diags.push(
                    Diagnostic::error("wrong type for 'terminal.option_as_alt', expected string")
                        .with_key("terminal.option_as_alt"),
                ),
            },
            "word_separators" => match v.as_str() {
                Some(s) => term.word_separators = s.to_string(),
                None => diags.push(
                    Diagnostic::error(
                        "wrong type for 'terminal.word_separators', expected string",
                    )
                    .with_key("terminal.word_separators"),
                ),
            },
            "osc52" => match v.as_str() {
                Some(s) => term.osc52 = s.to_string(),
                None => diags.push(
                    Diagnostic::error("wrong type for 'terminal.osc52', expected string")
                        .with_key("terminal.osc52"),
                ),
            },
            "warn_multiline_paste" => match v.as_bool() {
                Some(b) => term.warn_multiline_paste = b,
                None => diags.push(
                    Diagnostic::error(
                        "wrong type for 'terminal.warn_multiline_paste', expected boolean",
                    )
                    .with_key("terminal.warn_multiline_paste"),
                ),
            },
            "minimum_contrast" => match as_f32(v) {
                Some(f) if f >= 1.0 => term.minimum_contrast = f,
                _ => diags.push(
                    Diagnostic::error(
                        "wrong type for 'terminal.minimum_contrast', expected number >= 1.0",
                    )
                    .with_key("terminal.minimum_contrast"),
                ),
            },
            "confirm_close_ignore" => match v.as_array() {
                Some(arr) => {
                    let mut list = Vec::new();
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            list.push(s.to_string());
                        }
                    }
                    term.confirm_close_ignore = list;
                }
                None => diags.push(
                    Diagnostic::error(
                        "wrong type for 'terminal.confirm_close_ignore', expected array of strings",
                    )
                    .with_key("terminal.confirm_close_ignore"),
                ),
            },
            other => {
                if !KNOWN.contains(&other) {
                    diags.push(
                        Diagnostic::warning(format!("unknown key 'terminal.{}'", other))
                            .with_key(format!("terminal.{}", other)),
                    );
                }
            }
        }
    }
}

fn parse_shell_section(val: &toml::Value, shell: &mut ShellConfig, diags: &mut Vec<Diagnostic>) {
    let tbl = match val.as_table() {
        Some(t) => t,
        None => {
            diags.push(
                Diagnostic::error("section 'shell' must be a table").with_key("shell"),
            );
            return;
        }
    };

    const KNOWN: &[&str] = &["program", "args", "login", "integration", "env"];
    for (k, v) in tbl {
        match k.as_str() {
            "program" => match v.as_str() {
                Some(s) => shell.program = s.to_string(),
                None => diags.push(
                    Diagnostic::error("wrong type for 'shell.program', expected string")
                        .with_key("shell.program"),
                ),
            },
            "args" => match v.as_array() {
                Some(arr) => {
                    let mut args = Vec::new();
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            args.push(s.to_string());
                        }
                    }
                    shell.args = args;
                }
                None => diags.push(
                    Diagnostic::error("wrong type for 'shell.args', expected array of strings")
                        .with_key("shell.args"),
                ),
            },
            "login" => match v.as_bool() {
                Some(b) => shell.login = b,
                None => diags.push(
                    Diagnostic::error("wrong type for 'shell.login', expected boolean")
                        .with_key("shell.login"),
                ),
            },
            "integration" => match v.as_bool() {
                Some(b) => shell.integration = b,
                None => diags.push(
                    Diagnostic::error("wrong type for 'shell.integration', expected boolean")
                        .with_key("shell.integration"),
                ),
            },
            "env" => match v.as_table() {
                Some(tbl) => {
                    let mut env = HashMap::new();
                    for (env_k, env_v) in tbl {
                        if let Some(s) = env_v.as_str() {
                            env.insert(env_k.clone(), s.to_string());
                        }
                    }
                    shell.env = env;
                }
                None => diags.push(
                    Diagnostic::error("wrong type for 'shell.env', expected table")
                        .with_key("shell.env"),
                ),
            },
            other => {
                if !KNOWN.contains(&other) {
                    diags.push(
                        Diagnostic::warning(format!("unknown key 'shell.{}'", other))
                            .with_key(format!("shell.{}", other)),
                    );
                }
            }
        }
    }
}

fn parse_blocks_section(
    val: &toml::Value,
    blocks: &mut BlocksConfig,
    diags: &mut Vec<Diagnostic>,
) {
    let tbl = match val.as_table() {
        Some(t) => t,
        None => {
            diags.push(
                Diagnostic::error("section 'blocks' must be a table").with_key("blocks"),
            );
            return;
        }
    };

    const KNOWN: &[&str] = &["enabled", "show_duration", "sticky_header"];
    for (k, v) in tbl {
        match k.as_str() {
            "enabled" => match v.as_bool() {
                Some(b) => blocks.enabled = b,
                None => diags.push(
                    Diagnostic::error("wrong type for 'blocks.enabled', expected boolean")
                        .with_key("blocks.enabled"),
                ),
            },
            "show_duration" => match v.as_bool() {
                Some(b) => blocks.show_duration = b,
                None => diags.push(
                    Diagnostic::error(
                        "wrong type for 'blocks.show_duration', expected boolean",
                    )
                    .with_key("blocks.show_duration"),
                ),
            },
            "sticky_header" => match v.as_bool() {
                Some(b) => blocks.sticky_header = b,
                None => diags.push(
                    Diagnostic::error(
                        "wrong type for 'blocks.sticky_header', expected boolean",
                    )
                    .with_key("blocks.sticky_header"),
                ),
            },
            other => {
                if !KNOWN.contains(&other) {
                    diags.push(
                        Diagnostic::warning(format!("unknown key 'blocks.{}'", other))
                            .with_key(format!("blocks.{}", other)),
                    );
                }
            }
        }
    }
}

fn parse_editor_section(
    val: &toml::Value,
    editor: &mut EditorConfig,
    diags: &mut Vec<Diagnostic>,
) {
    let tbl = match val.as_table() {
        Some(t) => t,
        None => {
            diags.push(
                Diagnostic::error("section 'editor' must be a table").with_key("editor"),
            );
            return;
        }
    };

    const KNOWN: &[&str] = &[
        "type_ahead",
        "auto_complete",
        "history_same_cwd_first",
        "syntax_highlighting",
    ];
    for (k, v) in tbl {
        match k.as_str() {
            "type_ahead" => match v.as_bool() {
                Some(b) => editor.type_ahead = b,
                None => diags.push(
                    Diagnostic::error("wrong type for 'editor.type_ahead', expected boolean")
                        .with_key("editor.type_ahead"),
                ),
            },
            "auto_complete" => match v.as_bool() {
                Some(b) => editor.auto_complete = b,
                None => diags.push(
                    Diagnostic::error(
                        "wrong type for 'editor.auto_complete', expected boolean",
                    )
                    .with_key("editor.auto_complete"),
                ),
            },
            "history_same_cwd_first" => match v.as_bool() {
                Some(b) => editor.history_same_cwd_first = b,
                None => diags.push(
                    Diagnostic::error(
                        "wrong type for 'editor.history_same_cwd_first', expected boolean",
                    )
                    .with_key("editor.history_same_cwd_first"),
                ),
            },
            "syntax_highlighting" => match v.as_bool() {
                Some(b) => editor.syntax_highlighting = b,
                None => diags.push(
                    Diagnostic::error(
                        "wrong type for 'editor.syntax_highlighting', expected boolean",
                    )
                    .with_key("editor.syntax_highlighting"),
                ),
            },
            other => {
                if !KNOWN.contains(&other) {
                    diags.push(
                        Diagnostic::warning(format!("unknown key 'editor.{}'", other))
                            .with_key(format!("editor.{}", other)),
                    );
                }
            }
        }
    }
}

fn parse_split_section(val: &toml::Value, split: &mut SplitConfig, diags: &mut Vec<Diagnostic>) {
    let tbl = match val.as_table() {
        Some(t) => t,
        None => {
            diags.push(
                Diagnostic::error("section 'split' must be a table").with_key("split"),
            );
            return;
        }
    };

    for (k, v) in tbl {
        match k.as_str() {
            "inherit_cwd" => match v.as_bool() {
                Some(b) => split.inherit_cwd = b,
                None => diags.push(
                    Diagnostic::error("wrong type for 'split.inherit_cwd', expected boolean")
                        .with_key("split.inherit_cwd"),
                ),
            },
            other => diags.push(
                Diagnostic::warning(format!("unknown key 'split.{}'", other))
                    .with_key(format!("split.{}", other)),
            ),
        }
    }
}

fn parse_pane_section(val: &toml::Value, pane: &mut PaneConfig, diags: &mut Vec<Diagnostic>) {
    let tbl = match val.as_table() {
        Some(t) => t,
        None => {
            diags.push(
                Diagnostic::error("section 'pane' must be a table").with_key("pane"),
            );
            return;
        }
    };

    const KNOWN: &[&str] = &["inactive_dim", "focus_follows_mouse"];
    for (k, v) in tbl {
        match k.as_str() {
            "inactive_dim" => match as_f32(v) {
                Some(f) => pane.inactive_dim = f.clamp(0.0, 1.0),
                None => diags.push(
                    Diagnostic::error("wrong type for 'pane.inactive_dim', expected number")
                        .with_key("pane.inactive_dim"),
                ),
            },
            "focus_follows_mouse" => match v.as_bool() {
                Some(b) => pane.focus_follows_mouse = b,
                None => diags.push(
                    Diagnostic::error(
                        "wrong type for 'pane.focus_follows_mouse', expected boolean",
                    )
                    .with_key("pane.focus_follows_mouse"),
                ),
            },
            other => {
                if !KNOWN.contains(&other) {
                    diags.push(
                        Diagnostic::warning(format!("unknown key 'pane.{}'", other))
                            .with_key(format!("pane.{}", other)),
                    );
                }
            }
        }
    }
}

fn parse_notifications_section(
    val: &toml::Value,
    notif: &mut NotificationsConfig,
    diags: &mut Vec<Diagnostic>,
) {
    let tbl = match val.as_table() {
        Some(t) => t,
        None => {
            diags.push(
                Diagnostic::error("section 'notifications' must be a table")
                    .with_key("notifications"),
            );
            return;
        }
    };

    const KNOWN: &[&str] = &["bell", "system", "long_command_secs", "dock_badge"];
    for (k, v) in tbl {
        match k.as_str() {
            "bell" => match v.as_str() {
                Some(s) => notif.bell = s.to_string(),
                None => diags.push(
                    Diagnostic::error("wrong type for 'notifications.bell', expected string")
                        .with_key("notifications.bell"),
                ),
            },
            "system" => match v.as_bool() {
                Some(b) => notif.system = b,
                None => diags.push(
                    Diagnostic::error("wrong type for 'notifications.system', expected boolean")
                        .with_key("notifications.system"),
                ),
            },
            "long_command_secs" => match v.as_integer() {
                Some(i) if i >= 0 => notif.long_command_secs = i as u64,
                _ => diags.push(
                    Diagnostic::error(
                        "wrong type for 'notifications.long_command_secs', expected non-negative integer",
                    )
                    .with_key("notifications.long_command_secs"),
                ),
            },
            "dock_badge" => match v.as_bool() {
                Some(b) => notif.dock_badge = b,
                None => diags.push(
                    Diagnostic::error(
                        "wrong type for 'notifications.dock_badge', expected boolean",
                    )
                    .with_key("notifications.dock_badge"),
                ),
            },
            other => {
                if !KNOWN.contains(&other) {
                    diags.push(
                        Diagnostic::warning(format!("unknown key 'notifications.{}'", other))
                            .with_key(format!("notifications.{}", other)),
                    );
                }
            }
        }
    }
}

fn parse_keybindings_section<F>(
    val: &toml::Value,
    keybindings: &mut KeybindingsConfig,
    is_valid_action: &F,
    diags: &mut Vec<Diagnostic>,
) where
    F: Fn(&str) -> bool,
{
    let tbl = match val.as_table() {
        Some(t) => t,
        None => {
            diags.push(
                Diagnostic::error("section 'keybindings' must be a table")
                    .with_key("keybindings"),
            );
            return;
        }
    };

    for (chord_str, v) in tbl {
        let binding = match v {
            toml::Value::String(act) => KeybindingDef::Simple(act.clone()),
            toml::Value::Table(def_tbl) => {
                let action = match def_tbl.get("action").and_then(|a| a.as_str()) {
                    Some(a) => a.to_string(),
                    None => {
                        diags.push(
                            Diagnostic::error(format!(
                                "keybinding '{}' definition must include an 'action' string",
                                chord_str
                            ))
                            .with_key(chord_str),
                        );
                        continue;
                    }
                };
                let when = def_tbl
                    .get("when")
                    .and_then(|w| w.as_str())
                    .map(|s| s.to_string());
                let text = def_tbl
                    .get("text")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string());

                KeybindingDef::Complex { action, when, text }
            }
            _ => {
                diags.push(
                    Diagnostic::error(format!(
                        "invalid value for keybinding '{}', expected string action or table",
                        chord_str
                    ))
                    .with_key(chord_str),
                );
                continue;
            }
        };

        let entry_diags = validate_keybinding_entry(
            chord_str,
            binding.action(),
            binding.when(),
            is_valid_action,
        );
        diags.extend(entry_diags);

        keybindings.insert(chord_str.clone(), binding);
    }
}

fn parse_colors_section(val: &toml::Value, colors: &mut ColorsConfig, diags: &mut Vec<Diagnostic>) {
    let tbl = match val.as_table() {
        Some(t) => t,
        None => {
            diags.push(
                Diagnostic::error("section 'colors' must be a table").with_key("colors"),
            );
            return;
        }
    };

    for (k, v) in tbl {
        match v.as_str() {
            Some(color_str) => match parse_color(color_str) {
                Ok(_) => {
                    colors.insert(k.clone(), color_str.to_string());
                }
                Err(err) => {
                    diags.push(
                        Diagnostic::error(format!(
                            "invalid color for 'colors.{}': {}",
                            k, err
                        ))
                        .with_key(format!("colors.{}", k)),
                    );
                }
            },
            None => {
                diags.push(
                    Diagnostic::error(format!(
                        "wrong type for 'colors.{}', expected hex color string",
                        k
                    ))
                    .with_key(format!("colors.{}", k)),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_config() {
        let toml_str = r#"
        [font]
        size = 14.5
        ligatures = false

        [window]
        padding = [12.0, 16.0]
        opacity = 0.95

        [keybindings]
        "cmd+d" = "split_right"
        "cmd+shift+k" = { action = "send_text", text = "clear\r", when = "terminal" }
        "#;

        let (cfg, diags) = parse(toml_str);
        assert!(diags.is_empty(), "expected 0 diagnostics, got {:?}", diags);
        assert_eq!(cfg.font.size, 14.5);
        assert!(!cfg.font.ligatures);
        assert_eq!(cfg.window.padding, [12.0, 16.0]);
        assert_eq!(cfg.window.opacity, 0.95);
        assert_eq!(
            cfg.keybindings.get("cmd+d"),
            Some(&KeybindingDef::Simple("split_right".into()))
        );
    }

    #[test]
    fn test_parse_unknown_keys_and_types() {
        let toml_str = r#"
        [unknown_section]
        foo = 123

        [font]
        size = "not a number"
        unknown_font_key = true
        "#;

        let (cfg, diags) = parse(toml_str);
        // Default size should be kept because string was invalid
        assert_eq!(cfg.font.size, 13.0);

        let warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.level == crate::diagnostic::DiagnosticLevel::Warning)
            .collect();
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.level == crate::diagnostic::DiagnosticLevel::Error)
            .collect();

        assert_eq!(warnings.len(), 2); // unknown section + unknown font key
        assert_eq!(errors.len(), 1); // wrong type for font.size
    }

    #[test]
    fn test_parse_malformed_syntax_returns_defaults() {
        let bad_toml = "[font\nsize = 12";
        let (cfg, diags) = parse(bad_toml);
        assert_eq!(cfg.font.size, 13.0); // default
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].level, crate::diagnostic::DiagnosticLevel::Error);
    }

    #[test]
    fn test_template_parses_cleanly() {
        let tpl = crate::template::template();
        let (cfg, diags) = parse(tpl);
        assert!(diags.is_empty(), "template diagnostics: {:?}", diags);
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn test_partial_sections_and_fallbacks() {
        let partial_toml = r#"
        [terminal]
        scrollback_lines = 50000

        [sidebar]
        visible = false

        [notifications]
        bell = "sound"
        "#;
        let (cfg, diags) = parse(partial_toml);
        assert!(diags.is_empty());
        assert_eq!(cfg.terminal.scrollback_lines, 50000);
        // other terminal fields default
        assert_eq!(cfg.terminal.cursor_style, "bar");
        assert!(!cfg.sidebar.visible);
        assert_eq!(cfg.notifications.bell, "sound");
        assert_eq!(cfg.notifications.long_command_secs, 10);
    }

    #[test]
    fn test_colors_section_parse() {
        let toml_str = r##"
        [colors]
        "bg.app" = "#000000"
        "cursor" = "#FF0000 @ 50%"
        "##;
        let (cfg, diags) = parse(toml_str);
        assert!(diags.is_empty(), "diagnostics: {:?}", diags);
        assert_eq!(cfg.colors.get("bg.app").map(|s| s.as_str()), Some("#000000"));
        assert_eq!(
            cfg.colors.get("cursor").map(|s| s.as_str()),
            Some("#FF0000 @ 50%")
        );
    }

    #[test]
    fn test_fuzz_config_parse_never_panics() {
        // Pseudo-random deterministic generator (xorshift64)
        let mut state: u64 = 0xDEADBEEFCAFE1234;
        let mut rand_u64 = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };

        let seed_corpus = [
            "",
            " ",
            "\n\n\n",
            "\0",
            "[font]\nsize = ",
            "[font]\nsize = -999999999999999999999999999999",
            "[font]\nsize = 1e9999",
            "[font]\nsize = nan",
            "[font]\nsize = inf",
            "[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]",
            "\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"\"",
            "[keybindings]\n\"\" = \"\"",
            "[keybindings]\n\"cmd+++\" = \"test\"",
            "[colors]\n\"bg.app\" = \"#\"",
            "[colors]\n\"bg.app\" = \"#1234567890abcdef\"",
            "[colors]\n\"bg.app\" = \"#FF @ %%%\"",
            "=\n=\n=\n=",
            "a = [ 1, 2, 3, ]",
            "key = \u{10FFFF}\n",
        ];

        // 1. Test all seed corpus entries
        for seed in seed_corpus {
            let _ = parse(seed);
        }

        // 2. Fuzz with 5000 generated mutations
        let chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789[]{}()=+\"-' \t\n\r#@.:,/\0";
        let char_bytes = chars.as_bytes();

        for _ in 0..5000 {
            let len = (rand_u64() % 80) as usize;
            let mut s = String::with_capacity(len);
            for _ in 0..len {
                let idx = (rand_u64() as usize) % char_bytes.len();
                s.push(char_bytes[idx] as char);
            }
            // parse must NEVER panic on any input
            let _ = parse(&s);
        }
    }
}
