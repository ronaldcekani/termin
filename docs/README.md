# Termin — Blueprint Index

Termin is a native macOS terminal written in Rust. It combines the **workspace / per-pane tabs / split** model of cmux with the **command blocks, modern input editor, completions and command palette** of Warp. No accounts, no browser, no cloud, no AI.

Primary goals, in priority order:

1. **Correct terminal** — a real VT emulator that runs vim, htop, tmux, ssh, Claude Code, etc. without glitches.
2. **Minimal memory** — every byte per cell, per row, per pane is budgeted (see `specs/performance.md`).
3. **Small bundle** — tight dependency allowlist, no bundled fonts, LTO, stripped single binary.
4. **Low latency, low idle cost** — zero CPU when idle, render only on change.
5. **Portable later** — macOS only now, but every platform touchpoint sits behind a trait so Linux/Windows backends can be added without touching core crates.

## Document map

| Doc | What it defines |
|---|---|
| [architecture.md](architecture.md) | Crates, threads, data flow, cross-crate contracts (traits/types), dependency policy, build profile |
| [specs/terminal-core.md](specs/terminal-core.md) | VT parser integration, grid, cell layout, scrollback tiers, reflow, modes, OSC handling, blocks model |
| [specs/pty-and-shell.md](specs/pty-and-shell.md) | PTY spawning, IO reactor, shell integration scripts for zsh/bash, env vars |
| [specs/text-and-rendering.md](specs/text-and-rendering.md) | Fonts, CoreText shaping/rasterization, glyph atlas, Metal pipelines, draw list, damage tracking |
| [specs/platform-macos.md](specs/platform-macos.md) | Platform trait, AppKit window/view, input + IME, display link, clipboard, system notifications |
| [specs/app-model-and-ui.md](specs/app-model-and-ui.md) | Windows → workspaces → split tree → panes → tabs; sidebar, tab strip, focus, actions, keybindings, palette, find, attention/notifications, sidebar metadata |
| [specs/input-editor.md](specs/input-editor.md) | Block-mode input editor, highlighting, history, autosuggest, completion engine + specs |
| [specs/config.md](specs/config.md) | TOML schema, defaults, themes, hot reload |
| [specs/ipc-cli.md](specs/ipc-cli.md) | Unix socket protocol, `termin` CLI subcommands |
| [specs/persistence.md](specs/persistence.md) | Session restore format and lifecycle |
| [specs/performance.md](specs/performance.md) | Memory/size/latency budgets, benchmarks, profiling, CI gates |
| [design/design-system.md](design/design-system.md) | Tokens: color, type, spacing, radius, elevation, motion, icons, terminal palette |
| [design/components.md](design/components.md) | Every UI component: anatomy, metrics, states, behavior |
| [implementation-phases.md](implementation-phases.md) | Phased, parallelizable work plan with file ownership and acceptance criteria |

## Decision log

Decisions confirmed with the product owner on 2026-10-31. Changing any of these requires updating this table first.

| # | Decision | Choice |
|---|---|---|
| D1 | Platforms | macOS only for v1. Linux/Windows later → platform + renderer behind traits |
| D2 | Window/GPU layer | Native backends: AppKit + Metal via `objc2` crates. No winit, no wgpu |
| D3 | VT core | Own grid/scrollback on top of the `vte` parser crate |
| D4 | Warp features | Command blocks, modern input editor, completions + history search, command palette |
| D5 | cmux features | Vertical workspace sidebar, attention/notification rings, session restore, CLI/socket control |
| D6 | Config | TOML file + hot reload. No settings UI |
| D7 | Shell integration | zsh and bash |
| D8 | Name | `termin` (binary), `Termin.app` |
| D9 | Ligatures | Supported, toggleable (`font.ligatures`) |
| D10 | Min macOS | 13.0 Ventura |
| D11 | Visual direction | Own identity, Warp-inspired structure |
| D12 | Completions | Native engine + built-in specs (no shell round-trip) |
| D13 | Tab model | cmux model: Workspace → split tree → Pane → Tabs |
| D14 | Windows | Multiple OS windows; each window owns a list of workspaces; workspaces can move between windows |
| D15 | Fallback | Classic terminal mode when shell integration is absent and for alt-screen apps. No ssh auto-integration in v1 |
| D16 | Docs | Markdown in `docs/` |

## Out of scope (v1)

Account login, cloud sync, browser panes, AI features, remote/ssh integration bootstrap, settings GUI, image protocols (sixel/kitty graphics/iTerm inline images), plugin system, tmux control mode, Linux/Windows backends.

## Glossary

- **Window** — an OS window (`NSWindow`). Contains a sidebar and exactly one *active* workspace view.
- **Workspace** — a named container shown as one card in the sidebar. Owns one split tree.
- **Split tree** — binary tree whose leaves are panes; inner nodes are horizontal/vertical splits with a ratio.
- **Pane** — a leaf of the split tree. Has a tab strip and one active tab.
- **Tab** (a.k.a. *session*) — one PTY + one `Terminal` state + optional input editor. The unit that runs a shell.
- **Block** — one command execution inside a tab: prompt, command text, output, exit status, timing. Requires shell integration.
- **Block mode** — tab state where shell integration is active and the main screen is shown as blocks with the input editor below.
- **Classic mode** — plain terminal grid, keys go straight to the PTY. Used when integration is missing, in alt screen, or when the user toggles it.
- **Attention** — a pane/tab/workspace state raised by BEL, OSC 9/777/99 notifications, or long command completion while unfocused.
