# Spec: Text and Rendering (`termin-text`, `termin-render`)

## 1. Goals

- One GPU pass per frame, ≤ 4 draw calls typical (quads, mask glyphs, color glyphs, icons), ≤ 2 ms CPU to build a full 4K frame, **zero** GPU/CPU work when nothing changed.
- Crisp text at any scale factor; procedural box-drawing/block elements so TUIs look seamless.
- Atlas memory starts tiny and grows on demand.

## 2. Fonts (`termin-text`)

### 2.1 Font resolution

- `FontSpec { family: Option<String>, size_pt: f32, weight: Weight, italic: bool }`.
- Terminal font default: system monospaced (`NSFont.monospacedSystemFont(ofSize:weight:)` → SF Mono), 13 pt.
- UI font: system UI font (`.AppleSystemUIFont`), sizes from design tokens.
- Four faces per terminal font: regular, bold, italic, bold-italic. If a face is missing, synthesize: bold via `CTFontCreateCopyWithSymbolicTraits`, else stroke-based fake bold in rasterizer; italic via 12° skew matrix.
- Fallback chain: primary → `CTFontCreateForString` per missing scalar (cached by scalar in `FxHashMap<char, FontId>`) → `Apple Color Emoji` for emoji presentation → last resort `.LastResort`.

### 2.2 Cell metrics

```
cell_w = ceil(advance('0') * scale) / scale         // pixel-aligned in physical px
cell_h = ceil((ascent + descent + leading) * line_height * scale) / scale
baseline = round((cell_h - (ascent+descent)) / 2 + ascent)
```
`line_height` from config (default 1.2 for terminal grid; block-mode adds its own spacing separately). Underline/strikethrough positions from font tables, thickness ≥ 1 physical px.

### 2.3 Glyph lookup fast path

- Per face: `ascii: [GlyphRef; 95]` for printable ASCII — no hashing on the hottest path.
- Else `FxHashMap<(char, FaceIdx), GlyphRef>`.
- Graphemes (from GraphemeTable): shaped via `CTLine`, cached by `(grapheme id, face)`.

### 2.4 Ligatures (D9)

When `font.ligatures = true`:
- For each row, split into **runs** of consecutive cells with identical face + no wide chars + no cursor/selection boundary inside.
- A run containing only ASCII letters/digits/space (no symbol chars from the set `!#$%&*+-./:;<=>?@\^_|~`) skips shaping (no ligature possible) → fast path.
- Otherwise shape with `CTTypesetter`/`CTLine` on the run string with `kCTLigatureAttributeName = 1` (and `calt` enabled). Map output glyphs back to cell columns using string indices; ligature glyph spans N cells and is positioned at the first cell.
- Cache shaped runs: `FxHashMap<u64 /*hash of (face, text)*/, ShapedRunId>` with an LRU of 4096 entries.
- Cursor over a ligature: render the run unshaped for the cursor cell's run (avoid hidden characters).

### 2.5 Rasterization

- CoreGraphics bitmap context: A8 (grayscale coverage) for mask glyphs, BGRA8 premultiplied for color glyphs (emoji, `sbix`/`COLR`).
- Grayscale AA only (macOS has no subpixel AA since 10.14). Apply `font.thicken` option (`CGContextSetShouldSmoothFonts` path) — default on for dark backgrounds, like Terminal.app.
- Rasterize at physical resolution; glyph positioned at integer physical pixel (no subpixel positioning for grid text; UI proportional text uses 3 subpixel x-offsets: 0, 1/3, 2/3).

### 2.6 Procedural glyphs

Generated (not from font) into the atlas at cell size, so lines connect perfectly:
- Box drawing U+2500–U+257F (light/heavy/double/dashed/rounded corners).
- Block elements U+2580–U+259F (eighths, quadrants, shades).
- Braille U+2800–U+28FF.
- Powerline U+E0B0–U+E0B3 triangles.
- Legacy computing sextants U+1FB00–U+1FB3B — **should**.

Implementation: small CPU rasterizer in `termin-text/src/procedural.rs` writing A8 bitmaps with exact pixel math.

### 2.7 Atlas

- Two textures: `mask` (R8Unorm) and `color` (BGRA8Unorm). Start 512×512 each (256 KiB + 1 MiB); color atlas lazily created on first color glyph.
- Packing: shelf packer (rows by height bucket, 1 px padding). Own implementation (~150 lines).
- Growth: when full, double size (max 8192²) and re-upload existing contents via blit (`MTLBlitCommandEncoder` copy) — no re-rasterization.
- Eviction: when at max size and full, clear atlas + glyph cache and rebuild on demand (rare: requires ~100k unique glyphs).
- Font size / scale factor change → clear atlas.
- `GlyphCache` produces `AtlasUpload { atlas, rect, bytes_offset }` list; pixel data lives in one staging `Vec<u8>` reused per frame.

## 3. Renderer (`termin-render`)

### 3.1 Metal setup

- `MTLCreateSystemDefaultDevice` (prefer low-power GPU on dual-GPU Macs: pick device with `isLowPower` if present — terminal doesn't need the dGPU).
- `CAMetalLayer`: `pixelFormat = BGRA8Unorm_sRGB`, `framebufferOnly = true`, `presentsWithTransaction = false` (true only during live window resize to avoid jitter), `displaySyncEnabled = true`, `maximumDrawableCount = 2` (lower latency + memory), `colorspace = sRGB` (or Display P3 via config), `opaque` unless window opacity < 1.
- One `MTLCommandQueue`. One render pipeline per primitive kind, created at startup from the embedded `.metallib`.
- Instance buffers: ring of **2** `MTLBuffer`s (`storageModeShared`) per window, grown by doubling, shrunk when < 25 % used for 300 frames. Semaphore of 2 in-flight frames.

### 3.2 Pipelines and shaders (`resources/shaders/termin.metal`)

| Pipeline | Instance | Fragment |
|---|---|---|
| `quad` | `QuadInst` (rect, color, border color, radius, border width, shadow blur) | SDF rounded rect with AA edge, optional border ring, optional soft shadow (single-pass, approximated) |
| `glyph_mask` | `GlyphInst` | sample R8 atlas × color; gamma-correct blending in linear space with a contrast boost param |
| `glyph_color` | `GlyphInst` | sample BGRA atlas (premultiplied), color alpha used as opacity |
| `icon` | `GlyphInst` | same as mask (icons live in mask atlas) |

All use one shared unit-quad vertex buffer (4 verts, triangle strip) + instancing. Uniforms: viewport size in physical px, scale factor, gamma params.

### 3.3 DrawList

See `architecture.md §3.5`. Layers are drawn in order; each layer issues ≤ 4 instanced draws with a scissor rect. Typical frame: layer 0 (window chrome + sidebar), layer 1 per visible pane (clipped), layer N overlays (palette, menus, toasts).

Instance structs are `#[repr(C)]` and `bytemuck`-free: use `unsafe impl` of a local `Pod` marker trait with a compile-time size assert.

### 3.4 Pane render cache (the key optimization)

Each pane holds:

```rust
struct PaneRenderCache {
    rows: Vec<RowInstances>,     // one per visible row
    generation: u64,             // terminal generation last synced
    view_key: ViewKey,           // scroll position, size, font, theme epoch, selection epoch
}
struct RowInstances { bg: SmallVec<[QuadInst; 4]>, glyphs: Vec<GlyphInst>, color_glyphs: SmallVec<[GlyphInst; 2]>, deco: SmallVec<[QuadInst; 2]> }
```

- On sync, only rows in `Damage.rows` are rebuilt. `Damage.scrolled = n` rotates the `rows` vec by n before rebuilding the newly exposed rows.
- Background quads merge horizontal runs with equal bg into one quad; default bg emits nothing.
- Row instances are built in pane-local coordinates; the layer translation is applied via a per-layer offset uniform → scrolling/moving panes never touches instances.
- When `view_key` changes (resize, font, theme), rebuild all.
- Building a row: iterate cells → style lookup (cached resolved colors per style id per theme epoch: `Vec<ResolvedStyle>` parallel to StyleTable) → glyph lookup → push instance.

Cursor, selection, search highlights, block chrome and IME preedit are **not** in the row cache; they are drawn as separate small instance sets each frame.

### 3.5 Frame scheduling

- Frames requested by: PTY output (dirty pane visible), input, animation tick, resize, cursor blink.
- `CVDisplayLink` (macOS 13 compatible; switch to `CADisplayLink` from `NSView.displayLink(target:selector:)` when running on macOS 14+) runs only while frames are requested; stop after 2 idle vsyncs.
- During live resize, render synchronously in `setFrameSize` with `presentsWithTransaction = true`.
- When window occluded (`NSWindow.occlusionState`), skip rendering entirely; mark dirty and render on un-occlude.
- ProMotion: request frames at display rate only while animating or streaming output; text-only updates are fine at the native rate since we only render on change.

### 3.6 Color

- Theme colors are defined as sRGB hex. Instances carry sRGB `u32`; the shader decodes to linear (exact piecewise sRGB curve via a 256-entry LUT in a constant buffer) and blends in linear space.
- Drawable format is `BGRA8Unorm_sRGB`, so hardware re-encodes to sRGB on write.- Glyph coverage gets a configurable contrast/gamma boost (`font.thicken`) so light-on-dark text does not look thin under linear blending.
- Text contrast: apply "minimum contrast" adjustment (config `terminal.minimum_contrast`, default 1.0 = off) in the row builder, not the shader.

### 3.7 Memory

- No retained CPU copies of atlas pixels (only staging for current frame uploads).
- Instance ring buffers sized to the largest frame recently seen.
- `release_idle_resources()` called when all windows hidden/occluded for > 30 s: drop staging buffers, shrink instance buffers to minimum, purge shaped-run LRU.

## 4. UI text

UI labels (sidebar, tabs, palette) use the same glyph pipeline with the proportional UI font:
- `TextLayout` cache keyed by `(string hash, font, max_width)` producing positioned glyph list + measured width, with ellipsis truncation (`…`) at max width (middle-truncation for paths: `~/Gola…/tiam-ms-address`).
- LRU of 512 layouts.

## 5. Testing

- `termin-text`: unit tests for metrics math, shelf packer (no overlap, growth), procedural glyph pixel snapshots (compare A8 buffers to fixtures).
- `termin-render`: offscreen render tests to an `MTLTexture`, read back, compare with tolerance against PNG fixtures (fixtures stored as raw RGBA + small header to avoid an `image` dependency; xtask converts for viewing). macOS-only CI job.
- Bench: build `PaneRenderCache` for a 300×80 screen of mixed SGR text, target ≤ 1.0 ms on M1.
