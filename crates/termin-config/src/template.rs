//! Default commented configuration template.
//!
//! See `docs/specs/config.md §2`.

pub const DEFAULT_CONFIG_TEMPLATE: &str = r##"# Termin Configuration (TOML)
# See https://github.com/termin/termin/blob/main/docs/specs/config.md

[font]
family = ""              # "" = system monospaced (SF Mono)
size = 13.0
line_height = 1.2
ligatures = true
thicken = true
bold_is_bright = false

[theme]
name = "auto"            # "auto" | built-in ("termin-dark", "termin-light") or user theme name
light = "termin-light"
dark = "termin-dark"

[window]
padding = [10.0, 14.0]   # [vertical, horizontal] pt inside pane body
opacity = 1.0
blur = false
restore_session = true
quit_on_last_window_close = false
colorspace = "srgb"      # "srgb" | "display-p3"

[sidebar]
visible = true
width = 240.0
show_git = true
show_ports = true
show_paths = true

[terminal]
scrollback_lines = 10000
cursor_style = "bar"     # "block" | "bar" | "underline"
cursor_blink = true
option_as_alt = "left"   # "none" | "left" | "right" | "both"
word_separators = " ,│`|:\"'()[]{}<>\t"
osc52 = "write"          # "none" | "write" | "read-write"
warn_multiline_paste = true
minimum_contrast = 1.0
confirm_close_ignore = ["zsh", "bash", "fish", "sh", "login", "tmux"]

[shell]
program = ""             # "" = $SHELL
args = []
login = true
integration = true
env = {}                 # extra environment variables

[blocks]
enabled = true           # block mode when shell integration is present
show_duration = true
sticky_header = true

[editor]
type_ahead = true
auto_complete = false
history_same_cwd_first = true
syntax_highlighting = true

[split]
inherit_cwd = true

[pane]
inactive_dim = 0.0
focus_follows_mouse = false

[notifications]
bell = "attention"       # "attention" | "sound" | "none"
system = true            # macOS notifications when app is inactive
long_command_secs = 10
dock_badge = true

[keybindings]
# "<chord>" = "<action>" or { action = "...", when = "editor|terminal|block_selected|always" }
# chord: modifiers cmd ctrl alt shift joined by "+", key name lowercase: "cmd+shift+d", "ctrl+r", "cmd+1"
# value "none" unbinds a default
"cmd+d" = "split_right"
"cmd+shift+d" = "split_down"

[colors]
# Optional per-key color overrides on top of active theme (e.g. "bg.app" = "#000000")
"##;

/// Return the complete commented default configuration template.
pub fn template() -> &'static str {
    DEFAULT_CONFIG_TEMPLATE
}
