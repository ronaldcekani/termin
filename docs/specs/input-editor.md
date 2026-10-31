# Spec: Input Editor, History, Completions (`termin-editor`)

`termin-editor` is pure logic (no rendering, no platform). `termin-ui` renders `EditorState` and forwards input; `termin-app` connects it to sessions.

## 1. Editor buffer

```rust
pub struct EditorState {
    text: String,                 // whole command, may contain '\n'
    cursor: usize,                // byte offset, always on a grapheme boundary
    anchor: Option<usize>,        // selection anchor
    undo: UndoStack,              // coalesced by word / 1 s pause, max 200 entries
    tokens: Vec<Token>,           // highlighting cache, recomputed on edit (cheap: commands are short)
    suggestion: Option<Suggestion>,   // ghost text
    menu: Option<CompletionMenu>,
    history_nav: Option<HistoryNav>,  // up/down prefix navigation state
    preferred_col: Option<u16>,   // for vertical movement
}
```

A `String` is enough: commands are rarely > 4 KiB. Edits are O(n) memmove on tiny buffers. Hard cap 1 MiB (paste beyond → offer to write to temp file / send raw).

### 1.1 Editing commands (macOS conventions)

| Input | Effect |
|---|---|
| chars / IME commit | insert at cursor (replace selection) |
| `←/→`, `⌥←/⌥→`, `⌘←/⌘→` | char / word / line start-end; with `⇧` extend selection |
| `↑/↓` | multi-line: move line; on first/last line: history navigation (prefix-filtered by text before cursor) |
| `⌫`, `⌥⌫`, `⌘⌫` | delete char / word / to line start |
| `⌦`, `⌃D` (non-empty) | delete forward |
| `⌃A/⌃E/⌃K/⌃U/⌃W/⌃Y` | emacs bindings (readline muscle memory) |
| `⇧↩` / `⌥↩` | insert newline |
| `↩` | accept completion if menu open; else submit |
| `Tab` | open completion menu / cycle |
| `→` at end, or `⌃F` at end | accept full suggestion; `⌥→` accepts next word |
| `Esc` | close menu → clear suggestion → (2nd press) nothing |
| `⌃C` | clear editor (stored in a kill-ring-like "recently cleared" for `⌘Z`) |
| `⌘Z / ⌘⇧Z` | undo / redo |
| `⌃R` | history search panel |
| mouse | click sets cursor, drag selects, double-click word, triple-click line |

### 1.2 Submit

`submit()` returns the command string; trims trailing whitespace-only lines; empty → sends just `\r` (shell prints new prompt, matching normal behavior). Adds to Termin history after the shell confirms execution (`133;C`), with exit code filled on `133;D`.

## 2. Shell tokenizer and highlighting

Small hand-written lexer (`tokenize(&str) -> Vec<Token>`), POSIX-ish, shared with the completion engine:

```rust
enum TokenKind { Command, Argument, Flag, String, Variable, Operator /* | && || ; & > < >> 2> */, Comment, Path, Subshell, Whitespace, Error /* unterminated quote */ }
struct Token { kind: TokenKind, range: Range<usize>, ctx: CmdIdx }
```

- Splits on operators to find **command positions** (first word after start, `|`, `&&`, `||`, `;`, `(`, `$(`, backticks; skipping env assignments `FOO=bar cmd` and prefixes like `sudo`, `env`, `time`, `nohup`, `exec`).
- `Command` token validity: known builtin / alias (from integration-reported aliases, Phase 4) / executable on `$PATH` (cache, see §4.3) → normal color; unknown → `danger` underline-less tint (`editor.command.invalid` token).
- Colors from design tokens `syntax.*`.

## 3. History

### 3.1 Sources (merged, deduplicated)

1. **Termin history** — `~/Library/Application Support/Termin/history.tsv`, append-only, one line per command:
   `<unix_ts>\t<exit|->\t<duration_ms|->\t<cwd>\t<shell>\t<command with \\n and \\t escaped>`.
   Rotated to the most recent 50 000 entries on startup when file > 2× that.
2. **Shell history file** — path from `OSC 6973 hist=`, default `~/.zsh_history` / `~/.bash_history`. zsh extended format `: <ts>:<dur>;cmd` supported, plus zsh metafied bytes (`0x83` escape) decoding. Read once per shell type on the worker thread, re-read tail on file growth (stat size).

In-memory: `HistoryIndex { entries: Vec<Entry>, by_text: FxHashMap<u64 /*hash*/, u32> }`, newest last. Strings stored in one arena `String` with offsets to avoid per-entry allocations (~50k entries × avg 40 B ≈ 2 MB worst case; lazily loaded on first editor focus).

### 3.2 Features

- **Up/Down prefix navigation**: filter by the text before cursor at first press; prefer entries from the same cwd first (configurable).
- **Autosuggest (ghost text)**: on each edit, find most recent entry starting with the full buffer (same cwd first, then global). O(n) scan with early exit over newest-first entries; cap scan at 20k entries per keystroke, else use a prefix trie built lazily (Phase 4 optimization if profiling shows need).
- **History search panel** (`⌃R`): overlay above editor, fuzzy (`nucleo-matcher`) across all entries, shows command, cwd, relative time, exit status icon. `Enter` puts into editor; `⌘Enter` executes immediately.

## 4. Completion engine (D12)

### 4.1 Pipeline

```
complete(buffer, cursor, ctx) -> CompletionResult
  1. tokenize; find current command segment and the token under cursor (or empty token)
  2. resolve spec for the command (by name, following aliases from shell if known)
  3. walk spec: subcommands consumed so far → current node
  4. decide what is expected at cursor: subcommand | option | option-arg | positional arg
  5. collect candidates from: spec entries, generators (path, dir, git refs, env vars, …), history args
  6. fuzzy-rank (prefix matches first, then fuzzy), dedupe, cap 200
```

`ctx` = `{ cwd, env snapshot (PATH, HOME), shell kind }`.

Synchronous parts must complete in < 2 ms. Generators marked `async` (git branches, npm scripts reading `package.json`, make targets) run on the worker with a 300 ms timeout; menu shows sync results immediately and merges async ones when they arrive (stable ordering: async results append unless they rank above the 5th item).

### 4.2 Specs

Source format `resources/specs/<command>.toml` (authoring format, compiled by build.rs into `static` tables):

```toml
name = "git"
description = "Distributed version control"
[[subcommands]]
name = "checkout"
aliases = ["co"]          # user alias names are added at runtime if reported by shell (Phase 4)
description = "Switch branches or restore files"
args = [{ name = "branch", generators = ["git_branches", "git_tags"], optional = true }]
options = [
  { names = ["-b"], description = "Create and switch to new branch", args = [{ name = "new-branch" }] },
  { names = ["--force", "-f"], description = "Force checkout" },
]
```

Schema: `Command { name, aliases, description, subcommands, options, args, persistent_options }`, `Option { names, description, args, repeatable, exclusive_with }`, `Arg { name, optional, variadic, suggestions: [str], generators: [GeneratorId], template: "filepaths"|"folders"|none }`.

Built-in spec set for v1 (≈ 25 commands, keep each terse): `git, cargo, npm, pnpm, yarn, bun, node, docker, docker compose, kubectl (top-level only), brew, cd, ls, cat, cp, mv, rm, mkdir, ssh (hosts from ~/.ssh/config + known_hosts), make, go, python/python3, pip, rustup, code/open`.

Size budget: compiled spec tables ≤ 150 KiB in the binary. Description strings are deduplicated at build time.

Unknown commands: fall back to `filepaths` template for args and history-derived arguments (`argument history`: tokens previously used after this command, ranked by frequency).

### 4.3 Generators (built-in, Rust functions)

| Id | Behavior | Sync/async |
|---|---|---|
| `filepaths` / `folders` | read_dir of the dir part of token (relative to cwd, `~` expanded), hide dotfiles unless token starts with `.`, append `/` for dirs; escape spaces/specials on insert | sync, cached per dir 2 s |
| `executables` | `$PATH` scan → names; cached; invalidated by PATH change or 30 s | sync after first (first scan on worker at startup) |
| `env_vars` | after `$` | sync |
| `git_branches`, `git_tags`, `git_remotes` | read `.git/refs/...` + `packed-refs` directly (no git spawn) | sync |
| `git_changed_files` | `git status --porcelain -z` | async |
| `npm_scripts` | parse `package.json` `scripts` keys (tiny ad-hoc JSON scan via `serde_json` on worker) | async |
| `make_targets` | regex-free scan of `Makefile` for `^target:` lines | async |
| `ssh_hosts` | `~/.ssh/config` `Host` lines + `known_hosts` first field | sync, cached |
| `history_args` | per-command argument frequency | sync |

### 4.4 Completion menu behavior

- Opens on `Tab` (and automatically while typing after a space/`-` if `editor.auto_complete = true`, default false → Warp-like explicit Tab).
- Single candidate → insert directly (plus trailing space for non-dir).
- Common prefix of all candidates longer than token → insert it first.
- Menu: up to 10 rows visible, each `[icon] label   description` (icon per kind: subcommand, option, file, folder, branch, history). `↑/↓` or `Tab/⇧Tab` move, `↩`/`Tab` accept, `Esc` close. Typing filters live.

## 5. Testing

- Tokenizer table tests incl. quotes, escapes, `$(...)`, heredoc start (treated as rest-of-line), comments.
- Spec compiler tests (build.rs logic lives in a `spec_compile` module unit-tested normally).
- Completion golden tests: `("git ch|", cwd fixture) → ["checkout", "cherry-pick", ...]`.
- History parser tests: zsh extended + metafied input, bash plain, Termin TSV escape round-trip.
- Bench: autosuggest over 50k entries per keystroke < 0.5 ms; completion sync phase < 2 ms on fixture dir with 5k files.
