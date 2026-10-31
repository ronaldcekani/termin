# Spec: Terminal Core (`termin-vt`)

Pure-Rust terminal state machine. No IO, no threads, no platform code. Fully unit-testable and fuzzable.

## 1. Responsibilities

- Drive the `vte::Parser` and implement `vte::Perform` (print, execute, csi_dispatch, esc_dispatch, osc_dispatch, hook/put/unhook for DCS).
- Maintain primary and alternate screens, cursor, modes, tab stops, charsets, scroll region.
- Store scrollback in a two-tier memory-optimized structure.
- Reflow on resize (primary screen only; alt screen truncates/pads like xterm).
- Parse shell-integration OSCs into the **block model**.
- Encode keys and mouse events according to active modes.
- Provide read access for rendering (row views + damage), selection-to-text, and search.

## 2. Cell and row layout

### 2.1 Cell — 8 bytes

```rust
#[repr(C)]
pub struct Cell {
    pub ch: u32,      // Unicode scalar, or GRAPHEME_BIT | index into GraphemeTable
    pub style: u16,   // index into StyleTable (0 = default style)
    pub flags: CellFlags, // u16
}
bitflags! { pub struct CellFlags: u16 {
    WIDE         = 1 << 0,   // first half of a 2-column char
    WIDE_SPACER  = 1 << 1,   // second half (renders nothing)
    WRAPLINE     = 1 << 2,   // only meaningful on last cell: line soft-wraps into next row
    PROTECTED    = 1 << 3,   // DECSCA
    DIRTY_HINT   = 1 << 4,   // reserved
}}
const GRAPHEME_BIT: u32 = 1 << 31;
```

Compared to alacritty's ~24-byte cell, this is ~3x smaller.

### 2.2 StyleTable

```rust
pub struct Style {
    pub fg: Color, pub bg: Color, pub ul_color: Color,   // Color = packed u32: tag (default|indexed|rgb) + value
    pub attrs: Attrs,                                     // u16 bitflags: BOLD, DIM, ITALIC, UNDERLINE{single,double,curly,dotted,dashed}, BLINK, INVERSE, HIDDEN, STRIKE, OVERLINE
    pub link: u16,                                        // 0 = none, else index into HyperlinkTable (OSC 8)
}
pub struct StyleTable { styles: Vec<Style>, lookup: FxHashMap<Style, u16>, live: BitVec /* mark during compaction */ }
```

- Interned on SGR change, not per cell: the parser keeps `current_style_id`; SGR updates compute a new `Style` and look it up/insert.
- Capacity 65 535. When full: run **compaction** — mark styles referenced by hot rows (cold tier stores resolved styles, see 3.2), rebuild table, remap hot cells. Amortized rare.
- `HyperlinkTable`: `Vec<Box<str>>` URIs + id param; same compaction.

### 2.3 GraphemeTable

Clusters that are not a single scalar (combining marks, ZWJ emoji, VS16) are stored as `Box<str>` in a `Vec`, deduplicated through an `FxHashMap<Box<str>, u32>`. Compacted with styles.

### 2.4 Row

```rust
pub struct Row {
    cells: Box<[Cell]>,   // length == cols at time of write
    len: u16,             // index past last non-blank cell (trailing blanks are implicit)
    flags: RowFlags,      // WRAPPED (soft wrap continues into next row), BLOCK_MARK bits, DIRTY
}
```

Allocation strategy: hot rows recycled through a free list keyed by `cols` so steady-state scrolling performs **zero allocations**.

## 3. Scrollback: two tiers

```
 ┌──────────────────────┐   oldest
 │ Cold tier: chunks    │   logical lines, compact UTF-8 + style runs, optional LZ4
 ├──────────────────────┤
 │ Hot tier: ring<Row>  │   last `hot_rows` scrollback rows + visible screen, full cells
 └──────────────────────┘   newest (visible screen at the bottom of the ring)
```

### 3.1 Hot tier

- `VecDeque<Row>`-like ring with capacity `screen_rows + hot_scrollback` where `hot_scrollback = clamp(2 × screen_rows, 200, 2000)`.
- When a row leaves the hot tier it is **appended to the cold builder** (rows are joined across `WRAPPED` boundaries into logical lines before encoding).

### 3.2 Cold tier

```rust
pub struct ColdChunk {
    first_line: u64,             // absolute logical line number
    line_count: u16,             // ≤ 256 logical lines per chunk
    text: Box<[u8]>,             // UTF-8 of all lines concatenated (no separators)
    line_ends: Box<[u32]>,       // byte offsets
    runs: Box<[StyleRun]>,       // (byte_len: u16, style: PackedStyle) — PackedStyle is the resolved Style, not an id
    wrapped_rows_cache: Cell<(u16 /*cols*/, u32 /*row count*/)>,
    compressed: bool,            // LZ4 over text+runs when chunk is sealed and older than N chunks
}
```

- Lines are width-independent → **reflow is free** for cold data; only `wrapped_rows_cache` is recomputed lazily for the current width.
- Styles are stored resolved so cold data never pins StyleTable entries.
- Wide chars and graphemes are naturally represented in UTF-8; widths recomputed with `unicode-width` on read.
- Compression: optional feature `cold-lz4` (crate `lz4_flex`, `default-features=false, features=["safe-encode","safe-decode"]`). Measure; enable only if it wins ≥ 30 % on typical logs for ≤ 40 KiB binary growth. Default off until measured.
- Reading cold rows for display (user scrolled up) decodes a chunk into a small LRU of 4 decoded chunks.
- Limit enforcement: total logical lines ≤ `scrollback_lines`; drop whole oldest chunks.

### 3.3 Absolute line numbering

Every row has an absolute index `AbsLine(u64)` that never changes while the row exists (increments as lines are produced). Blocks, selections, search matches and marks reference `AbsLine`. Evicted lines make referencing structures clamp/drop.

> Note on rows vs logical lines: hot tier is indexed by rows; cold tier by logical lines. `AbsLine` counts **rows at time of production**. After a resize, cold-tier mapping of `AbsLine` → (chunk, line, wrap-offset) uses the per-chunk cached row counts. Implementation must expose `fn locate(abs: AbsLine) -> Location` and keep this logic in one module (`scrollback/index.rs`).

## 4. Screens, cursor, modes

- Primary + alternate screen (`?47`, `?1047`, `?1049` with cursor save). Alt screen has no scrollback.
- Cursor: position, pending-wrap flag, saved cursor (DECSC/DECRC) per screen, shape (DECSCUSR 0-6), visibility (`?25`), blink.
- Scroll region DECSTBM, left/right margins DECSLRM (`?69`) — **should**.
- Origin mode `?6`, autowrap `?7`, insert mode IRM, LNM.
- Charsets G0–G3 with DEC special graphics (line drawing).
- Tab stops (HTS, TBC, CHT, CBT).
- Modes tracked in `Modes` (bitflags) exposed to the app:
  - Keyboard: DECCKM `?1`, DECKPAM/DECKPNM, kitty keyboard protocol flag stack (`CSI > u`, `CSI < u`, `CSI = u`, `CSI ? u`), modifyOtherKeys (`CSI > 4 ; n m`).
  - Mouse: X10 `?9`, normal `?1000`, button `?1002`, any `?1003`, SGR `?1006`, SGR-pixels `?1016`, focus events `?1004`.
  - Bracketed paste `?2004`, synchronized output `?2026`, alt-scroll `?1007`.
  - Reports: DA1 (`CSI c` → `CSI ? 62 ; 22 c`), DA2, XTVERSION (`CSI > q` → `DCS > | termin(<version>) ST`), DSR/CPR, DECRQM for supported modes, XTWINOPS 14/16/18 size queries (pixel sizes from `cell_px`), OSC 4/10/11/12 color queries (answered from theme palette pushed into `Terminal` via `set_palette`).

### 4.1 Synchronized output (`?2026`)

While set, `generation` does not bump and damage accumulates; on reset (or 150 ms timeout tracked by the app), emit a single change. This is critical for flicker-free TUIs (Claude Code, neovim).

## 5. OSC / DCS handling

| Sequence | Handling |
|---|---|
| OSC 0/1/2 | `Effect::Title` |
| OSC 4, 10, 11, 12, 104, 110-112 | palette query/set (set is per-terminal override) |
| OSC 7 `file://host/path` | `Effect::Cwd` (percent-decoded) |
| OSC 8 | hyperlinks → HyperlinkTable, style.link |
| OSC 9 `;text` | `Effect::Notify { title: "", body }` (iTerm2 style). `OSC 9;4;…` progress → ignored v1 |
| OSC 777 `;notify;title;body` | `Effect::Notify` |
| OSC 99 (kitty) | `Effect::Notify` — basic `d=1` payloads only |
| OSC 52 | clipboard: write → `Effect::ClipboardSet` if policy allows (default: allow write, deny read) |
| OSC 133 A/B/C/D | blocks (section 6) |
| OSC 1337 `;SetUserVar=` / `CurrentDir=` | CurrentDir → Cwd; others ignored |
| OSC 6973 `;termin;...` | Termin private channel (section 6.3) |
| DCS `+q` (XTGETTCAP) | answer for `TN`, `Co`, `RGB` |
| DCS sixel / kitty graphics APC | ignored (consume and drop) |

OSC payload size cap: 64 KiB (OSC 52 cap 1 MiB); excess dropped.

## 6. Blocks model

### 6.1 Marks

Shell integration (see `pty-and-shell.md`) emits FinalTerm/OSC 133 marks:

| Mark | Meaning | Data |
|---|---|---|
| `133;A` | prompt start | — |
| `133;B` | prompt end / command input start | — |
| `133;C` | command executed, output starts | optional `;cmdline_url=<percent-encoded command>` (Termin extension) |
| `133;D[;exit]` | command finished | exit code |

### 6.2 Block

```rust
pub struct Block {
    pub id: BlockId,
    pub prompt_start: AbsLine,       // A
    pub input_start: Option<AbsLine>,// B
    pub output_start: Option<AbsLine>, // C
    pub end: Option<AbsLine>,        // D (exclusive end = row of next A or cursor row at D)
    pub command: Option<Box<str>>,   // from C's cmdline_url, or from editor submit
    pub exit: Option<i32>,
    pub started_at: Option<Instant>, pub finished_at: Option<Instant>,
    pub cwd: Option<Box<str>>,       // last OSC 7 seen before A
    pub flags: BlockFlags,           // ALT_SCREEN_USED, CLEARED, TRUNCATED (start evicted), INTERRUPTED
}
pub struct BlockList { blocks: VecDeque<Block>, active: Option<BlockId> }
pub enum BlockEvent { PromptStarted(BlockId), CommandStarted(BlockId), CommandFinished(BlockId, i32) }
```

Rules:
- `A` while a block is active without `D` → close it with `exit = None`, flag `INTERRUPTED`.
- `ED 2`/`ED 3` (clear screen / clear scrollback) or `clear` → mark all finished blocks with `end ≤ cursor` as `CLEARED` (UI hides them). `ED 3` additionally drops scrollback.
- Alt-screen enter during a block → set `ALT_SCREEN_USED`; output of alt-screen apps is not part of the block.
- Blocks whose rows were all evicted are dropped from `BlockList`.
- Cap: 10 000 blocks per tab (oldest dropped).

### 6.3 Termin private OSC

`OSC 6973 ; termin ; <key>=<value> ST` — reserved for the integration scripts to report extra metadata without polluting standard sequences. v1 keys: `shell=zsh|bash`, `version=<integration version>`, `hist=<path to history file>`.

## 7. Resize and reflow

- Primary screen: rewrap hot rows into logical lines and re-split for new width, keeping the cursor attached to its logical position. Preserve block `AbsLine` references by mapping old→new row indices during reflow (reflow returns a mapping function used by `BlockList::remap`).
- Alt screen: no reflow; truncate or pad.
- Cold tier: only invalidate `wrapped_rows_cache`.
- Resize is debounced by the app (live resize issues at most one `resize` per frame).

## 8. Selection and text extraction

- `Selection { kind: Simple | Word | Line | Block(BlockId) | Rect, anchor: AbsPos, head: AbsPos }`.
- Word boundaries configurable (`terminal.word_separators`, default `` ,│`|:"' ()[]{}<>\t``).
- `text_range` joins soft-wrapped rows without newline, trims trailing spaces per logical line.
- Semantic helpers: `block_command_text(id)`, `block_output_text(id)`.

## 9. Search

- `SearchQuery { text, case_sensitive, whole_word }`, plain substring only in v1 (regex later behind feature).
- Hot tier: scan rows converting cells to UTF-8 on the fly into a reusable buffer, `memchr::memmem::Finder`.
- Cold tier: search directly in decoded chunk UTF-8.
- Incremental API: `search_step(query, cursor, budget_lines) -> SearchProgress` so the UI can search 1M lines without blocking a frame (budget ~20k lines per frame).

## 10. Key & mouse encoding

`encode_key` covers:
- Legacy xterm encoding (CSI/SS3, DECCKM, modifiers as `;1+mods`).
- `option_as_alt` behavior passed in via config (left/right/both/none) — the app sets `Terminal::set_option_as_alt`.
- Kitty keyboard protocol flags 0b1 (disambiguate), 0b10 (report events), 0b100 (alternate keys), 0b1000 (all keys as escapes), 0b10000 (associated text).
- modifyOtherKeys level 2.

`encode_mouse` covers X10/normal/button/any with SGR and SGR-pixel encodings; returns `None` when reporting is off (app then uses mouse for selection).

## 11. Damage and generation

- `Damage { rows: BitVec (visible rows), cursor_moved: bool, all: bool, scrolled: i32 }`.
- `scrolled` lets the renderer shift cached row instances instead of rebuilding (cheap scroll of a full screen).
- `generation: u64` increments once per `advance` call that changed visible state.

## 12. Testing requirements

- Unit tests per CSI/ESC/OSC handler.
- Golden tests: feed recorded byte streams (`tests/corpus/*.bin`: vim session, htop, `ls --color`, Claude Code session, emoji/CJK samples, progress bars), assert screen text + style snapshot.
- Conformance: port relevant subset of `vttest` expectations and esctest cases as data-driven tests.
- Property tests (`proptest`): random byte streams never panic; reflow round-trip (resize A→B→A) preserves logical text; cold tier encode/decode round-trip.
- Fuzz target `fuzz_advance` (cargo-fuzz) in CI nightly.
- Benchmarks (criterion): `advance` throughput on ASCII-heavy, SGR-heavy, unicode-heavy, scrolling corpora; memory per 10k lines (see `performance.md`).
