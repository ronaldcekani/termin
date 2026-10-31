# Termin — Agent Guide

Native macOS terminal in Rust: cmux-style workspaces/per-pane tabs/splits + Warp-style blocks, input editor, completions, palette. Optimized for memory, bundle size and latency.

## Read before coding

1. `docs/README.md` — decisions (D1–D16), scope, glossary.
2. `docs/architecture.md` — crates, threads, contracts, dependency allowlist, build profile.
3. Your track in `docs/implementation-phases.md` — scope, owned files, acceptance criteria.
4. The spec docs your track lists, plus `docs/specs/performance.md §6` (hot-path rules).

The docs are the source of truth. If code and docs disagree, fix the code or open a `contract:` PR updating the doc first.

## Hard rules

- **Ownership:** edit only files your track owns. Request changes elsewhere in your PR description.
- **Contracts frozen:** `architecture.md §3` types/traits change only via a dedicated `contract:` PR. Additive, non-breaking changes are OK.
- **Dependencies:** only the allowlist in `architecture.md §7`. No async runtime, no winit/wgpu, no anyhow/thiserror, no clap, no regex, no tracing.
- **objc2 only** in `termin-platform`, `termin-text` (CoreText module), `termin-render` (Metal module).
- **Portable crates** (`termin-core`, `termin-vt`, `termin-editor`, `termin-config`, `termin-ipc`, `termin-persist`) must build and test without macOS frameworks.
- **unsafe:** only in crates listed in `performance.md §6`; every block has `// SAFETY:`.
- **No panics on input:** malformed escape sequences, config, IPC requests, history files never panic.
- **Hot paths:** no allocation per byte/cell/row/frame in steady state, no `format!`, FxHashMap not SipHash, no locks across syscalls on main.
- **Never write `config.toml`** from the app (user-owned). App state goes to `~/Library/Application Support/Termin/`.

## Conventions

- Rust stable (pinned in `rust-toolchain.toml`), edition 2021+.
- Errors: small hand-written enums implementing `Display` + `std::error::Error`.
- Logging: `log` macros; `debug!`/`trace!` compiled out in release.
- Naming follows the glossary: Window, Workspace, Pane, Tab (= session), Block.
- Tests next to code (`#[cfg(test)]`) for units; `tests/` for integration; benches with criterion in `benches/`.
- Commits: conventional style (`feat(vt): …`, `fix(render): …`, `contract: …`).

## Commands

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo xtask size        # binary size table vs budgets.toml
cargo xtask mem --quick # memory budget check (Phase 4)
```
