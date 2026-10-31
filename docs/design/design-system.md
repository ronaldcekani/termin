# Design System: Foundations (D11)

**Direction:** own identity, Warp-inspired structure. Quiet graphite chrome that recedes behind content; one cool accent (**Tide**, a sea-glass cyan) for focus/selection; one warm signal (**Ember**, amber) reserved exclusively for *attention* so notification rings are unmistakable. Typography is native macOS (SF Pro for UI, SF Mono for terminal). Density is comfortable, not cramped: terminals are read for hours.

Principles:
1. **Content first** — chrome uses low-contrast neutrals; the brightest pixels on screen are terminal text.
2. **One accent, one alarm** — Tide = "you are here / selected". Ember = "something needs you". Never swap.
3. **Native feel** — system fonts, system traffic lights, native menus and sheets, standard macOS shortcuts.
4. **Calm motion** — short, functional transitions; attention pulse is the only repeating animation and it stops.
5. **Cheap to render** — every visual is a rounded rect, a glyph, or an icon glyph. No gradients except the attention glow, no blur except optional window vibrancy, no images.

All tokens live in code in `termin-ui/src/theme/tokens.rs` as a `Tokens` struct populated from the active theme TOML (`[ui]` section) with fallbacks from §2.4.

## 1. Token naming

`<category>.<role>[.<variant>][.<state>]` — e.g. `bg.surface.hover`, `text.secondary`, `border.focus`, `syntax.flag`. In TOML themes use the same dotted keys under `[ui]`.

## 2. Color

### 2.1 Neutral ramp (graphite, slightly cool)

| Step | Dark theme | Light theme |
|---|---|---|
| `n0` | `#0C0D0F` | `#FFFFFF` |
| `n1` | `#111215` | `#F9FAFB` |
| `n2` | `#16181B` | `#F2F4F6` |
| `n3` | `#1C1E22` | `#E9ECEF` |
| `n4` | `#23262B` | `#DEE2E6` |
| `n5` | `#2C3036` | `#CDD3D9` |
| `n6` | `#3A3F47` | `#B3BAC2` |
| `n7` | `#525862` | `#8C949E` |
| `n8` | `#6E7580` | `#6B737D` |
| `n9` | `#9AA1AB` | `#4B525B` |
| `n10` | `#C4C9D0` | `#2D3238` |
| `n11` | `#E7EAEE` | `#15181B` |

### 2.2 Brand and semantic hues

| Hue | Dark | Light | Use |
|---|---|---|---|
| `tide` (accent) | `#3FD0C9` | `#0E9E98` | focus, selection, active indicators, links |
| `ember` (attention) | `#FFB547` | `#D98500` | attention rings, unread dots, notification status line |
| `success` | `#4ED38A` | `#1E9E5A` | exit 0 badges (used sparingly) |
| `danger` | `#FF6B6B` | `#D93A3A` | failed blocks, invalid command, destructive |
| `info` | `#7AA7FF` | `#2F6BDB` | informational banners |

### 2.3 Semantic tokens (dark values shown; light maps to same neutral steps mirrored)

| Token | Dark | Purpose |
|---|---|---|
| `bg.app` | `n0` | terminal/pane background (overridden by `terminal.background`) |
| `bg.sidebar` | `n1` | sidebar |
| `bg.chrome` | `n1` | tab strip background |
| `bg.surface` | `n2` | block hover, chips |
| `bg.surface.hover` | `n3` | hovered cards/tabs |
| `bg.elevated` | `n3` | palette, menus, completion menu, find bar |
| `bg.input` | `n1` | input editor area |
| `bg.selected` | `tide @ 14%` over bg | selected workspace card, selected block, palette row |
| `bg.attention` | `ember @ 10%` over bg | attention card tint |
| `bg.danger` | `danger @ 8%` over bg | failed block tint |
| `border.subtle` | `n3` | block separators, strip bottom |
| `border.default` | `n4` | panes dividers, input top border |
| `border.strong` | `n6` | elevated surfaces outline |
| `border.focus` | `tide` | focused input/rename field, keyboard focus |
| `border.attention` | `ember` | attention ring |
| `text.primary` | `n11` | titles, command text |
| `text.secondary` | `n9` | metadata, paths |
| `text.tertiary` | `n8` | timestamps, hints, placeholder |
| `text.disabled` | `n7` | disabled |
| `text.on_accent` | `#062B29` | text on solid tide fills |
| `text.link` | `tide` | hyperlinks hover underline |
| `icon.default` | `n8` | strip buttons |
| `icon.hover` | `n10` | |
| `cursor` | `tide` | terminal + editor caret (overridable by terminal theme) |
| `selection` | `tide @ 28%` | text selection |
| `search.match` | `ember @ 30%` | find matches |
| `search.current` | `ember @ 65%` | current match |
| `scrollbar` | `n6 @ 70%` | overlay scrollbar thumb |
| `shadow` | `#000 @ 45%` | elevated surfaces |

Syntax (input editor):

| Token | Dark | |
|---|---|---|
| `syntax.command` | `n11` bold-ish (weight 600) | valid command |
| `syntax.command.invalid` | `danger` | unknown command |
| `syntax.argument` | `n10` | |
| `syntax.flag` | `#8FB7FF` | `-v`, `--release` |
| `syntax.string` | `#B8E07A` | quoted |
| `syntax.variable` | `#E3A6FF` | `$HOME` |
| `syntax.operator` | `tide` | `|`, `&&`, `>` |
| `syntax.path` | `n10` underline-dotted when exists (Phase 4) | |
| `syntax.comment` | `n7` | |
| `syntax.ghost` | `n7` | autosuggestion |

### 2.4 Derivation rules (themes that only define terminal colors)

Given `bg = terminal.background`, `fg = terminal.foreground`, `accent = ui.accent or ansi[6] (cyan)`:
- `n0 = bg`; `n1..n5 = mix(bg, fg, [3,6,9,13,18]%)`; `n6..n11 = mix(bg, fg, [28,40,52,64,80,93]%)`.
- `tide = accent`; `ember = ui.attention or ansi[3]` (yellow) if its contrast vs. bg ≥ 3:1 else `#FFB547`.
- `danger = ansi[1]`, `success = ansi[2]`.
- Mixing in linear space; clamp contrast: `text.secondary` must be ≥ 4.5:1 vs `bg.sidebar`, else push toward fg.

### 2.5 Terminal palette — "Termin Dark"

| | Normal | Bright |
|---|---|---|
| black | `#1C1E22` | `#525862` |
| red | `#FF6B6B` | `#FF8C8C` |
| green | `#4ED38A` | `#7BE3A8` |
| yellow | `#FFC861` | `#FFD98A` |
| blue | `#6FA0FF` | `#97BBFF` |
| magenta | `#D58CFF` | `#E3B0FF` |
| cyan | `#3FD0C9` | `#7DE2DC` |
| white | `#C4C9D0` | `#F4F6F8` |

foreground `#E7EAEE`, background `#0C0D0F`, cursor `#3FD0C9`, cursor_text `#0C0D0F`, selection_bg `#3FD0C9 @ 28%`.

"Termin Light": foreground `#15181B`, background `#FFFFFF`; ANSI normal: black `#2D3238`, red `#C92A2A`, green `#1E8E50`, yellow `#A86A00`, blue `#2F6BDB`, magenta `#9A3FD1`, cyan `#0E8F89`, white `#8C949E`; bright: lighter by +10 % luminance except white `#6B737D`.

All ANSI colors meet ≥ 4.5:1 against their background except black/bright-black (by design).

### 2.6 Contrast requirements

- `text.primary` on any bg token ≥ 7:1; `text.secondary` ≥ 4.5:1; `text.tertiary` ≥ 3:1 (non-essential only).
- Focus and attention indicators ≥ 3:1 against adjacent colors (WCAG 1.4.11).
- `cargo test -p termin-ui theme_contrast` checks built-in themes.

## 3. Typography

| Token | Font | Size / line | Weight | Use |
|---|---|---|---|---|
| `type.mono` | `font.family` or SF Mono | `font.size` (13) / `line_height` (1.2) | 400 | terminal grid, editor, block command |
| `type.mono.bold` | same | same | 700 (or bold face) | SGR bold |
| `type.title` | SF Pro Text | 13 / 16 | 600 | workspace card title |
| `type.body` | SF Pro Text | 12 / 15 | 400 | tab labels, palette rows, menu-like lists |
| `type.meta` | SF Pro Text | 11 / 14 | 400 | card status line, block header meta, chips |
| `type.meta.mono` | SF Mono | 11 / 14 | 400 | paths, git branch, ports in sidebar |
| `type.caption` | SF Pro Text | 10 / 12 | 500, +2% tracking | badges, counts |
| `type.palette.input` | SF Pro Text | 15 / 20 | 400 | palette query |

Rules: UI text scales with `font.size` only for mono contexts; UI chrome text sizes are fixed (Apple HIG). Truncation: titles end-ellipsis; paths middle-ellipsis.

## 4. Spacing and sizing

Base unit **4 pt**. Scale: `s0=0, s1=2, s2=4, s3=6, s4=8, s5=12, s6=16, s7=20, s8=24, s9=32`.

| Token | Value |
|---|---|
| `size.titlebar` | 38 (traffic-light row; sidebar header + top of tab strip share it) |
| `size.sidebar.default` | 240 (min 180, max 400) |
| `size.tabstrip` | 30 |
| `size.tab.min` / `max` | 72 / 220 |
| `size.icon` | 14 (hit target 24) |
| `size.divider` | 1 visual / 6 hit |
| `size.chip` | 20 high |
| `size.block.header` | 22 |
| `size.editor.min_lines` / `max_lines` | 1 / 12 |
| `size.palette.width` | 600 (max 80% window) |
| `size.palette.row` | 34 |
| `size.menu.row` (completion) | 26 |
| `pad.pane` | 10 v / 14 h (config `window.padding`) |
| `pad.card` | 10 v / 12 h |
| `gap.cards` | 4 |

## 5. Radius

`r.xs=3` (chips inner, badges), `r.sm=5` (tabs, buttons, inputs), `r.md=8` (workspace cards, completion menu, find bar), `r.lg=12` (palette, banners). Blocks: `0` (full-bleed, separated by hairlines — Warp-like), selected block uses a 2 pt left bar, not a rounded box.

## 6. Elevation

Only two levels; shadows are cheap single-pass SDF shadows in the quad shader.

| Level | Use | Shadow | Border |
|---|---|---|---|
| `e0` | everything in-flow | none | none / hairlines |
| `e1` | completion menu, find bar, tooltips, toasts | `0 4 16 shadow` | `border.strong` 1 pt |
| `e2` | command palette, history search | `0 12 40 shadow` | `border.strong` 1 pt |

Palette open adds a scrim `n0 @ 35%` over the window (no blur).

## 7. Motion

| Token | Duration | Easing | Use |
|---|---|---|---|
| `m.instant` | 0 | — | focus changes, selection |
| `m.fast` | 90 ms | `cubic(0.2, 0, 0, 1)` | hover fades, tab close |
| `m.base` | 140 ms | `cubic(0.2, 0, 0, 1)` | palette/menu appear (opacity + 4 pt translate) |
| `m.slow` | 200 ms | `cubic(0.3, 0, 0, 1)` | sidebar toggle, editor height change |
| `m.attention` | 1100 ms × 3 | `ease-in-out sine` | attention ring pulse (alpha 100% → 35% → 100%), then steady at 100% |
| `m.spinner` | 900 ms/rev | linear | running block spinner (8-step glyph, not continuous redraw — 8 fps) |

Respect `NSWorkspace.accessibilityDisplayShouldReduceMotion`: all durations → 0, attention pulse → steady ring. Animations request frames only while active (render on demand).

## 8. Iconography

- ~24 icons, 14 pt grid, 1.5 pt stroke, rounded caps/joins, drawn as vector paths authored as SVG in `resources/icons/*.svg`, converted at build time into compact path command arrays (`termin-ui/build.rs`), rasterized once per scale into the mask atlas via CoreGraphics. No icon font, no PNGs.
- Set: `terminal, plus, split-right, split-down, close, chevron-down, chevron-right, folder, git-branch, bell, search, filter, more-horizontal, check, x-circle, clock, copy, rerun, arrow-down, workspace, port, warning, info, history`.
- Icons are tinted at draw time with `icon.*` tokens.

## 9. Cursor and selection

- Terminal cursor default `bar` 2 pt wide in `cursor` color; `block` draws cell bg `cursor` + glyph in `cursor_text`; unfocused pane → hollow block outline 1 pt.
- Blink: 530 ms on / 530 ms off, resets on keypress, stops after 10 s idle (saves power) — stays solid.
- Editor caret same as terminal cursor style `bar`.

## 10. Accessibility

- Contrast rules §2.6; reduced motion §7; all actions reachable by keyboard; palette lists every action.
- Minimum hit target 24×24 pt for icon buttons.
- Font size zoom (⌘+/⌘−) affects terminal + editor; UI chrome stays per HIG.
