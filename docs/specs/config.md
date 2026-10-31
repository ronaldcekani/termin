# Spec: Configuration (`termin-config`) (D6)

## 1. Files

| Path | Purpose |
|---|---|
| `$XDG_CONFIG_HOME/termin/config.toml` (default `~/.config/termin/config.toml`) | user config; never written by the app except creating a commented template on first "Open Config" if missing |
| `~/.config/termin/themes/<name>.toml` | user themes |
| built-in themes | embedded: `termin-dark`, `termin-light` (see `design/design-system.md`) |
| `~/Library/Application Support/Termin/` | app-owned state: session, history, shell scripts, persist state |

Missing file → defaults. Every key optional.

## 2. Schema (v1) with defaults

```toml
[font]
family = ""              # "" = system monospaced (SF Mono)
size = 13.0
line_height = 1.2
ligatures = true
thicken = true
bold_is_bright = false

[theme]
name = "auto"            # "auto" | built-in or user theme name
light = "termin-light"
dark = "termin-dark"

[window]
padding = [10, 14]       # [vertical, horizontal] pt inside pane body
opacity = 1.0
blur = false
restore_session = true
quit_on_last_window_close = false
colorspace = "srgb"      # "srgb" | "display-p3"

[sidebar]
visible = true
width = 240
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
env = {}                 # extra env vars

[blocks]
enabled = true           # block mode when integration present
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
system = true            # macOS notifications when app inactive
long_command_secs = 10
dock_badge = true

[keybindings]
# "<chord>" = "<action>" or { action = "...", when = "editor|terminal|block_selected|always" }
# chord: modifiers cmd ctrl alt shift joined by "+", key name lowercase: "cmd+shift+d", "ctrl+r", "cmd+1"
# value "none" unbinds a default
"cmd+d" = "split_right"
"cmd+shift+d" = "split_down"
# send_text example:
# "cmd+shift+k" = { action = "send_text", text = "clear\r" }

[colors]                 # optional per-key overrides on top of the active theme (same keys as theme files)
```

Validation: unknown keys → warning list (shown in banner once), wrong types → error for that key, default used. Errors never block startup.

## 3. Themes

Theme TOML has two sections: `[ui]` (design tokens, see `design-system.md §2`) and `[terminal]` (`foreground`, `background`, `cursor`, `cursor_text`, `selection_bg`, `selection_fg`, `ansi = [16 colors]`). Missing UI tokens are derived from `terminal.background/foreground` + accent using the derivation rules in `design-system.md §2.4`, so a theme can be just the 20 terminal colors.

## 4. Hot reload

- Reactor watches the config **directory** (`EVFILT_VNODE` on dir fd for `NOTE_WRITE`) and the file itself; editors that save via rename are handled by re-opening the file fd after `NOTE_DELETE|NOTE_RENAME`.
- Debounce 50 ms, then the main thread re-parses (file ≤ 64 KiB; parse < 1 ms).
- `ConfigDiff` computed between old and new `Config`; each subsystem applies only what changed:

| Change | Effect |
|---|---|
| font.* | rebuild text system faces, clear atlas, re-layout all panes, resize PTYs |
| theme/colors | bump theme epoch → rebuild pane render caches; update palette in Terminals (OSC 4 answers) |
| keybindings | rebuild keymap + regenerate main menu |
| terminal.scrollback_lines | applied to existing terminals (trim if lowered) |
| shell.* | new tabs only |
| sidebar/window/etc. | immediate re-layout |

- Parse errors → keep previous config, show banner `"config.toml: line 12: expected a number"` with "Open" button.

## 5. API

```rust
pub struct Config { pub font: FontConfig, pub theme: ThemeConfig, ... }   // plain structs, Default impl = table above
pub fn load(path: &Path) -> (Config, Vec<Diagnostic>);
pub fn parse(src: &str) -> (Config, Vec<Diagnostic>);
pub fn diff(old: &Config, new: &Config) -> ConfigDiff;   // bitflags of changed sections
pub fn load_theme(name: &str, user_dir: &Path) -> Result<Theme, Diagnostic>;
pub fn template() -> &'static str;                     // commented default config
```

Parsing via `serde` + `toml` into an intermediate `RawConfig` with all-`Option` fields, then merged onto defaults with per-key diagnostics. Keybinding chord parsing and action names are validated against `termin-app`'s action registry via a `fn(&str) -> bool` passed in (keeps `termin-config` independent of `termin-app`).
