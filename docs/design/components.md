# Design System: Components

Every component lists **anatomy, metrics (pt), tokens, states, behavior**. Metrics reference tokens from `design-system.md`. All components are drawn with `QuadInst` (rounded rects/borders/shadows), glyph runs and icon glyphs — nothing else.

Implementation home: `termin-ui/src/widgets/<component>.rs`. Each widget is a function `fn draw_<widget>(cx: &mut DrawCx, state: &..., rect: Rect) -> Hits` that appends to a `DrawList` layer and returns hit regions. No widget retains state; state lives in the app model.

## 0. Window layout

```
┌──────────────┬──────────────────────────────────────────────────────────┐
│ ● ● ●   ⧉ +  │ [tab][tab]                          [+][⫿][⊟]  │ [tab] ...  │  ← 38 titlebar row = sidebar header | pane tab strips (30) aligned bottom
│              ├──────────────────────────────────────────────┬───────────┤
│  sidebar     │  pane body                                   │ pane body │
│  (cards)     │                                              ├───────────┤
│              │                                              │ [tabs]    │
│              │                                              │ pane body │
└──────────────┴──────────────────────────────────────────────┴───────────┘
```

- Sidebar spans full window height under the traffic lights. Its header row (38) holds: traffic lights (system), then right-aligned **Toggle sidebar** and **New workspace** icon buttons.
- The workspace area starts at y=0 (full-size content view). Top-row panes have their tab strip vertically centered in the 38 pt title row (strip height 30, 4 pt top inset) so traffic lights and tabs share one line — matches cmux density. Non-top panes have their strip at their own top.
- Sidebar hidden → traffic lights overlay the first tab strip; strip gets a 78 pt left inset.

## 1. Sidebar

**Container:** `bg.sidebar`, right border 1 pt `border.subtle`. Scrolls vertically (overlay scrollbar). Padding 8 h, 6 top below header.

### 1.1 Workspace card

```
┌──────────────────────────────────┐  r.md, pad 10/12
│ Title of workspace           ● 2 │  type.title text.primary  | attention dot + count (type.caption, ember)
│ Claude is waiting for your input │  type.meta text.secondary (status line, 1 line, ellipsis)
│ ⎇ main*                          │  type.meta.mono text.secondary (git)
│ ~/Gola…/tiam-ms-address          │  type.meta.mono text.tertiary (paths, ≤3 lines)
│ [:5380] [:5381] +2               │  port chips
└──────────────────────────────────┘
```

| State | Visual |
|---|---|
| default | no bg |
| hover | `bg.surface.hover` |
| active (current workspace) | `bg.selected` fill + 2 pt left bar `tide` inset 6 pt vertically; title `text.primary` |
| attention (unread) | 1.5 pt ring `border.attention` (pulsing per `m.attention`), `bg.attention` tint; status line `ember` colored |
| active + attention | active fill + attention ring |
| dragging | card lifts: `e1` shadow, 0.9 opacity; drop indicator 2 pt `tide` line between cards |
| renaming | title replaced by inline text field (`bg.input`, `border.focus` 1 pt, r.sm) |

Lines collapse when empty (no blank rows). Card min height 34 (title only).

### 1.2 Port chip

Height 18, pad 0/6, `r.xs`, `bg.surface`, `type.meta.mono` `text.secondary`, text `:5380`. Hover `bg.surface.hover` + `text.link` + pointing-hand cursor.

## 2. Tab strip

Height 30. Background `bg.chrome`; bottom hairline `border.subtle`. Left pad 6.

### 2.1 Tab

```
╭─────────────────────────╮
│ ⌘ title text          × │  r.sm top corners; pad 0/10; gap 6
╰─────────────────────────╯
```

- Leading icon `terminal` 12 pt (or spinner while a command runs in that tab, or `ember` dot when attention).
- Label `type.body`; width min 72 max 220; ellipsis.
- Close `×` 14 pt icon button appears on hover and on active tab.

| State | Visual |
|---|---|
| inactive | transparent bg, label `text.secondary` |
| hover | `bg.surface` |
| active (in focused pane) | `bg.app` fill (merges with pane body — "folder tab" effect), label `text.primary`, 2 pt top bar `tide` |
| active (unfocused pane) | `bg.app` fill, no top bar, label `text.secondary` |
| attention | `ember` dot replaces icon |
| renaming | inline field |
| dragging | follows cursor with `e1`; insertion marker 2 pt `tide` vertical line |

### 2.2 Strip actions (right side)

Icon buttons 24×24 hit, 14 pt icon, gap 2: `plus` (New Tab), `split-right`, `split-down`. `icon.default` → `icon.hover` + `bg.surface` r.sm on hover; tooltip after 600 ms with action name + shortcut (`e1`, `type.meta`).

## 3. Pane

- Body bg `bg.app` (terminal background). Padding `pad.pane`.
- Divider between panes: 1 pt `border.default`; hover/drag → 2 pt `tide` and resize cursor.
- **Focused** pane: no border (content-first). Optional `pane.inactive_dim` overlay on unfocused panes.
- **Attention ring** (pane visible but unfocused): 2 pt inset ring `border.attention`, radius 0, pulse ×3 then steady; plus a soft inner glow `ember @ 12%`, 8 pt.
- **Zoomed**: small badge top-right `type.caption` "ZOOMED" on `bg.surface` r.xs, 8 pt inset.
- **Exited session**: centered message in classic mode area: "Process exited with code 1 — Press ↩ to restart, ⌘W to close" (`type.body` `text.secondary`).

## 4. Block (block view)

Full-bleed rows separated by 1 pt `border.subtle` hairlines. Block padding 8 v / pane h-padding.

```
│ ~/WebstormProjects/ninja  ⎇ main   1.34s                 ⧉  ⋯ │  header: type.meta text.tertiary; actions on hover
│ cargo build --release                                           │  command: type.mono, syntax tokens
│ (output rows)                                                   │  grid rows: type.mono
```

| State | Visual |
|---|---|
| finished ok | default; duration in header `text.tertiary` |
| failed (exit ≠ 0) | `bg.danger` tint over whole block + 2 pt left bar `danger`; header shows `x-circle` icon + `exit 1` in `danger` |
| running | header: spinner (`m.spinner`) + live duration (updates 1/s, not per frame); no bottom hairline until finished |
| hovered | `bg.surface @ 50%` tint; header actions visible: `copy`, `filter` (Phase 4), `more-horizontal` |
| selected | `bg.selected` tint + 2 pt left bar `tide` (failed+selected: tide bar wins, danger tint stays) |
| multi-selected | same as selected on each |
| sticky header | header pinned at pane top: `bg.chrome` background, bottom hairline, `e0` |
| cleared (⌘K) | not drawn |

Header content order: cwd (`~`-abbrev, middle-ellipsis) · git branch (if changed vs previous block or first block) · duration (≥ 100 ms shown as `0.12s`, ≥ 60 s `1m 03s`) · exit code if ≠ 0. Timestamp shown on hover tooltip.

**New output pill:** when scrolled up during streaming, centered bottom pill above editor: `arrow-down` + "New output", `bg.elevated`, `e1`, r.lg, height 24. Click → scroll to bottom.

## 5. Input editor

```
├──────────────────────────────────────────────────────────────┤  border.default top hairline
│ [📁 ~/proj] [⎇ main] [✕ 1]                                   │  context chips row, gap 6, h 20, pad top 8
│ ▍git push origin ma|in█                      ghost: ster      │  type.mono, pad 8/14, min 1 max 12 lines
└──────────────────────────────────────────────────────────────┘
```

- Area bg `bg.input`.
- **Chips**: cwd (folder icon, click → `cd` completion menu of recent dirs — Phase 4), git branch, last exit code (only if ≠ 0, `danger` text). Chip: h 20, pad 0/8, `r.xs`, `bg.surface`, `type.meta`, `text.secondary`.
- Text: syntax tokens `syntax.*`; ghost suggestion `syntax.ghost`; selection `selection`; caret `cursor` bar 2 pt, blink like terminal cursor.
- Placeholder when empty and first prompt: "Type a command…" `text.tertiary` (hidden after the first command in the tab).

| State | Visual |
|---|---|
| idle (at prompt, focused) | as above, caret visible |
| unfocused pane | caret hidden, chips dimmed (`text.tertiary`) |
| running (command executing) | text area shows type-ahead text in `text.tertiary`; leading spinner glyph; hint right-aligned "Running · input goes to process" `type.meta text.tertiary` |
| raw input fallback (continuation prompt) | editor hidden; classic cursor in grid |
| multi-line | grows upward with `m.slow`; line numbers not shown |

## 6. Completion menu

Anchored to the token start, opens **above** the editor (below if not enough room). `e1`, `bg.elevated`, r.md, width = clamp(longest label + description, 280, 560), max 10 rows × `size.menu.row` (26), pad 4.

Row: `[icon 14] label (type.mono 12, text.primary, matched chars in tide) ··· description (type.meta, text.tertiary, right-aligned, ellipsis)`. Selected row `bg.selected` r.sm. Scroll indicator if > 10. Footer (only when async generators pending): tiny spinner + "Loading…".

## 7. History search (⌃R)

Overlay docked above the editor, full pane width minus 14 pt margins, `e2`, `bg.elevated`, r.lg, max height 50% pane. Top: query field (`type.mono`, `search` icon). Rows (h 30): status icon (`check` success / `x-circle` danger / none), command (`type.mono`, matched chars `tide`), right: cwd (`type.meta.mono` tertiary) + relative time ("3h"). Selected row `bg.selected`.

## 8. Command palette (⌘⇧P / ⌘P)

Centered horizontally, top at 18% of window height. `e2`, `bg.elevated`, r.lg, width `size.palette.width`, scrim behind.

```
╭────────────────────────────────────────────────╮
│ 🔍  Search actions, workspaces, tabs…           │  input row h 48, type.palette.input; mode prefix shown as chip ("Actions")
├────────────────────────────────────────────────┤
│ ⫿  Split Right                          ⌘D     │  rows h 34: icon, label (type.body, matches tide), right: shortcut keycaps
│ ⊟  Split Down                          ⇧⌘D     │
│ ⧉  Workspace: NARUTO     ~/WebstormProjects/…  │  secondary text type.meta text.tertiary
╰────────────────────────────────────────────────╯   max 10 rows, then scroll
```

- Keycaps: each key glyph in a `r.xs` box `bg.surface`, `type.caption` `text.secondary`, h 16, min w 16.
- Section headers when no query: "Recent", "Workspaces", "Actions" — `type.caption` uppercase `text.tertiary`, h 24.
- Empty result: "No matches" centered `text.tertiary`.
- Appear: `m.base` (opacity 0→1, y +4→0). Dismiss: Esc, click scrim, action executed.

## 9. Find bar (⌘F)

Top-right of pane, 8 pt inset, `e1`, `bg.elevated`, r.md, h 32, width 320. Contents: `search` icon, field (`type.body`), count "3 / 27" (`type.meta text.tertiary`), `Aa` toggle (case), `chevron` up/down, `close`. No matches → count `danger`. Matches highlighted with `search.match`, current `search.current`.

## 10. Banner (config errors, update notes)

Top of window workspace area, full width, h 32, `bg.elevated` with left 3 pt bar (`danger` for errors, `info` for info), `type.body`, actions as text buttons right (`text.link`), close `×`. Not modal. Auto-dismiss: never for errors; 6 s for info.

## 11. Toast (ephemeral, e.g. "Copied output")

Bottom-center of focused pane, above editor, `e1`, `bg.elevated`, r.lg, h 28, `type.body`, 1.6 s then fade `m.base`.

## 12. Scrollbar

Overlay, appears while scrolling or hovering the right 10 pt of pane, width 6 (hover 8), thumb `scrollbar` r = width/2, no track. Fades out 800 ms after last scroll. Block view: tick marks on the scrollbar track for failed blocks (`danger`, 2 pt) and search matches (`ember`) — Phase 4.

## 13. Tooltip

`e1`, `bg.elevated`, r.sm, pad 4/8, `type.meta text.primary`, shortcut in `text.tertiary`. Delay 600 ms, no delay when moving between adjacent tooltip targets within 300 ms.

## 14. Native components (not custom-drawn)

Use AppKit for: main menu, right-click context menus (`NSMenu`), close-confirmation sheet (`NSAlert`), save dialog (`NSSavePanel`), About panel. This saves code and gives a native feel; they follow the window appearance (dark/light).

## 15. Component → phase map

| Component | Phase |
|---|---|
| Pane (classic), tab strip (basic), divider, scrollbar | 2 |
| Sidebar + cards, attention rings, port chips | 3 |
| Block, input editor, chips, completion menu, history search, palette | 3 |
| Find bar, banner, toast, tooltip | 3 |
| Drag-to-split zones, block filter, scrollbar ticks, cwd chip menu | 4 |
