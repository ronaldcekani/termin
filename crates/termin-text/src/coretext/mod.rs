//! CoreText font backend implementation for macOS.
//!
//! Handles font resolution, four-face synthesis, fallback chains,
//! ASCII fast path, ligature shaping, run caching, and CoreGraphics rasterization.
//!
//! See `docs/architecture.md §3.5` and `docs/specs/text-and-rendering.md §2`.

pub mod ffi;

use rustc_hash::FxHashMap;
use std::collections::VecDeque;
use std::ffi::c_void;

use crate::{
    procedural, FontBackend, FontId, FontMetrics, FontSpec, GlyphRef, RasterBuf, RasterInfo,
    ShapedGlyph,
};
use ffi::*;

/// Wrapper managing a CoreText font pointer with retain/release lifecycle.
#[derive(Debug)]
pub struct CTFontWrapper(pub CTFontRef);

impl CTFontWrapper {
    pub fn new(ptr: CTFontRef) -> Self {
        Self(ptr)
    }

    #[inline]
    pub fn as_ptr(&self) -> CTFontRef {
        self.0
    }
}

impl Clone for CTFontWrapper {
    fn clone(&self) -> Self {
        if !self.0.is_null() {
            // SAFETY: Retaining a valid CoreFoundation/CoreText object.
            unsafe {
                CFRetain(self.0);
            }
        }
        Self(self.0)
    }
}

impl Drop for CTFontWrapper {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: Releasing a valid CoreFoundation/CoreText object.
            unsafe {
                CFRelease(self.0);
            }
        }
    }
}

// SAFETY: CTFontRef is an immutable CoreFoundation object safe to send across threads.
unsafe impl Send for CTFontWrapper {}
unsafe impl Sync for CTFontWrapper {}

/// Face index within a font family (Regular=0, Bold=1, Italic=2, BoldItalic=3).
pub const FACE_REGULAR: usize = 0;
pub const FACE_BOLD: usize = 1;
pub const FACE_ITALIC: usize = 2;
pub const FACE_BOLD_ITALIC: usize = 3;

/// A single face in a resolved font family.
struct FontFace {
    ct_font: CTFontWrapper,
    synthetic_bold: bool,
    synthetic_italic: bool,
    /// Fast-path array for printable ASCII characters (32..=126).
    ascii: [GlyphRef; 95],
    /// Cache for non-ASCII characters resolved on this face.
    non_ascii: FxHashMap<char, GlyphRef>,
}

/// An entry in the backend's global glyph registry.
struct GlyphEntry {
    font: CTFontWrapper,
    glyph: CGGlyph,
    is_color: bool,
    is_procedural: bool,
    procedural_char: char,
    synthetic_bold: bool,
    _synthetic_italic: bool,
    font_id: FontId,
}

/// A resolved font family containing four faces and precalculated metrics.
struct LoadedFont {
    faces: [FontFace; 4],
    metrics: FontMetrics,
    _scale: f32,
    _line_height: f32,
    emoji_font: Option<CTFontWrapper>,
    last_resort_font: Option<CTFontWrapper>,
    /// Fallback font cache per character scalar.
    fallback_fonts: FxHashMap<char, CTFontWrapper>,
}

/// CoreText-based font backend for macOS.
pub struct CoreTextBackend {
    fonts: Vec<LoadedFont>,
    glyphs: Vec<GlyphEntry>,
    /// LRU cache for shaped ligature runs (cap 4096).
    shaped_cache: FxHashMap<u64, Vec<ShapedGlyph>>,
    shaped_lru: VecDeque<u64>,
    /// Per-glyph advances (points) for proportional UI fonts, keyed by `GlyphRef`.
    advances: FxHashMap<u32, f32>,
    /// Font smoothing / thicken option for dark backgrounds.
    pub thicken: bool,
    /// Global display scale factor (default 2.0 on Retina).
    pub scale: f32,
}

impl Default for CoreTextBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CoreTextBackend {
    /// Create a new CoreText font backend.
    pub fn new() -> Self {
        Self {
            fonts: Vec::new(),
            glyphs: Vec::new(),
            shaped_cache: FxHashMap::default(),
            shaped_lru: VecDeque::with_capacity(4096),
            advances: FxHashMap::default(),
            thicken: true,
            scale: 2.0,
        }
    }

    /// Set the display scale factor.
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale.max(1.0);
    }

    /// Helper to convert a Rust string to a CFStringRef.
    fn make_cf_string(s: &str) -> CFStringRef {
        // SAFETY: Creating CFString from UTF-8 bytes.
        unsafe {
            CFStringCreateWithBytes(
                std::ptr::null(),
                s.as_ptr(),
                s.len() as CFIndex,
                kCFStringEncodingUTF8,
                false,
            )
        }
    }

    /// Resolve or synthesize the 4 faces for a font specification.
    fn create_faces(
        spec: &FontSpec,
        scale: f32,
        ui: bool,
    ) -> ([FontFace; 4], Option<CTFontWrapper>, Option<CTFontWrapper>) {
        let size_pt = spec.size_pt.max(4.0) as CGFloat;

        // Base regular font
        let ui_font: CTFontRef = if ui {
            let class = if spec.weight == crate::FontWeight::Regular {
                kCTFontUIFontSystemReal
            } else {
                kCTFontUIFontEmphasizedSystem
            };
            // SAFETY: Creating the proportional system UI font.
            unsafe { CTFontCreateUIFontForLanguage(class, size_pt, std::ptr::null()) }
        } else {
            std::ptr::null()
        };
        let base_font: CTFontRef = if !ui_font.is_null() {
            ui_font
        } else if let Some(ref family) = spec.family {
            let cf_name = Self::make_cf_string(family);
            // SAFETY: Calling CTFontCreateWithName with valid CFStringRef.
            let f = unsafe { CTFontCreateWithName(cf_name, size_pt, std::ptr::null()) };
            // SAFETY: Releasing temporary CFString.
            unsafe { CFRelease(cf_name) };
            if f.is_null() {
                // SAFETY: Fallback to system monospaced font if requested family was not found.
                unsafe {
                    CTFontCreateUIFontForLanguage(
                        kCTFontUIFontUserFixedPitch,
                        size_pt,
                        std::ptr::null(),
                    )
                }
            } else {
                f
            }
        } else {
            // SAFETY: Creating system monospaced font (SF Mono).
            unsafe {
                CTFontCreateUIFontForLanguage(
                    kCTFontUIFontUserFixedPitch,
                    size_pt,
                    std::ptr::null(),
                )
            }
        };

        let base_font = if base_font.is_null() {
            // Fallback to Menlo if system monospaced returned null
            let cf_menlo = Self::make_cf_string("Menlo");
            // SAFETY: Creating Menlo font fallback.
            let f = unsafe { CTFontCreateWithName(cf_menlo, size_pt, std::ptr::null()) };
            // SAFETY: Releasing temporary CFString.
            unsafe { CFRelease(cf_menlo) };
            f
        } else {
            base_font
        };

        // 12° italic skew transform: tan(12°) ≈ 0.21255656167
        let skew_matrix = CGAffineTransform {
            a: 1.0,
            b: 0.0,
            c: 0.21255656167,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        };

        // 1. Regular face
        let reg_wrapper = CTFontWrapper::new(base_font);

        // 2. Bold face: try symbolic traits, fallback to synthetic fake bold
        // SAFETY: Copying font with bold trait.
        let bold_raw = unsafe {
            CTFontCreateCopyWithSymbolicTraits(
                base_font,
                0.0,
                std::ptr::null(),
                kCTFontBoldTrait,
                kCTFontBoldTrait,
            )
        };
        let (bold_wrapper, syn_bold) = if !bold_raw.is_null() {
            (CTFontWrapper::new(bold_raw), false)
        } else {
            (reg_wrapper.clone(), true)
        };

        // 3. Italic face: try symbolic traits, fallback to 12° skew transform
        // SAFETY: Copying font with italic trait.
        let italic_raw = unsafe {
            CTFontCreateCopyWithSymbolicTraits(
                base_font,
                0.0,
                std::ptr::null(),
                kCTFontItalicTrait,
                kCTFontItalicTrait,
            )
        };
        let (italic_wrapper, syn_italic) = if !italic_raw.is_null() {
            (CTFontWrapper::new(italic_raw), false)
        } else {
            // SAFETY: Creating skewed font copy with 12 degree skew matrix.
            let skewed = unsafe {
                CTFontCreateCopyWithAttributes(base_font, 0.0, &skew_matrix, std::ptr::null())
            };
            if !skewed.is_null() {
                (CTFontWrapper::new(skewed), true)
            } else {
                (reg_wrapper.clone(), true)
            }
        };

        // 4. Bold-Italic face
        // SAFETY: Copying font with bold and italic traits.
        let bi_raw = unsafe {
            CTFontCreateCopyWithSymbolicTraits(
                base_font,
                0.0,
                std::ptr::null(),
                kCTFontBoldTrait | kCTFontItalicTrait,
                kCTFontBoldTrait | kCTFontItalicTrait,
            )
        };
        let (bi_wrapper, syn_bi_bold, syn_bi_italic) = if !bi_raw.is_null() {
            (CTFontWrapper::new(bi_raw), false, false)
        } else if !bold_raw.is_null() {
            // SAFETY: Skewing the bold font if bold-italic trait is missing.
            let bi_skewed = unsafe {
                CTFontCreateCopyWithAttributes(bold_raw, 0.0, &skew_matrix, std::ptr::null())
            };
            if !bi_skewed.is_null() {
                (CTFontWrapper::new(bi_skewed), false, true)
            } else {
                (bold_wrapper.clone(), false, true)
            }
        } else {
            (italic_wrapper.clone(), true, syn_italic)
        };

        // Fallback fonts: Apple Color Emoji and LastResort
        let cf_emoji = Self::make_cf_string("Apple Color Emoji");
        // SAFETY: Creating Apple Color Emoji font.
        let emoji_raw = unsafe { CTFontCreateWithName(cf_emoji, size_pt, std::ptr::null()) };
        // SAFETY: Releasing temporary CFString.
        unsafe { CFRelease(cf_emoji) };
        let emoji_font = if !emoji_raw.is_null() {
            Some(CTFontWrapper::new(emoji_raw))
        } else {
            None
        };

        // SAFETY: Creating system fallback font using official CTFontCreateUIFontForLanguage API.
        let last_raw = unsafe {
            CTFontCreateUIFontForLanguage(kCTFontUIFontSystem, size_pt, std::ptr::null())
        };
        let last_resort_font = if !last_raw.is_null() {
            Some(CTFontWrapper::new(last_raw))
        } else {
            None
        };

        let dummy_ref = GlyphRef(u32::MAX);
        let faces = [
            FontFace {
                ct_font: reg_wrapper,
                synthetic_bold: false,
                synthetic_italic: false,
                ascii: [dummy_ref; 95],
                non_ascii: FxHashMap::default(),
            },
            FontFace {
                ct_font: bold_wrapper,
                synthetic_bold: syn_bold,
                synthetic_italic: false,
                ascii: [dummy_ref; 95],
                non_ascii: FxHashMap::default(),
            },
            FontFace {
                ct_font: italic_wrapper,
                synthetic_bold: false,
                synthetic_italic: syn_italic,
                ascii: [dummy_ref; 95],
                non_ascii: FxHashMap::default(),
            },
            FontFace {
                ct_font: bi_wrapper,
                synthetic_bold: syn_bi_bold,
                synthetic_italic: syn_bi_italic,
                ascii: [dummy_ref; 95],
                non_ascii: FxHashMap::default(),
            },
        ];

        let _ = scale;
        (faces, emoji_font, last_resort_font)
    }

    /// Compute cell metrics pixel-aligned at display scale factor.
    fn compute_metrics(font: CTFontRef, scale: f32, line_height: f32) -> FontMetrics {
        // SAFETY: Querying typographic metrics from CoreText font.
        let (ascent, descent, leading, ul_pos, ul_thick) = unsafe {
            (
                CTFontGetAscent(font) as f32,
                CTFontGetDescent(font) as f32,
                CTFontGetLeading(font) as f32,
                CTFontGetUnderlinePosition(font) as f32,
                CTFontGetUnderlineThickness(font) as f32,
            )
        };

        // Measure advance of character '0'
        let mut glyph_0 = [0u16];
        let ch_0 = ['0' as u16];
        // SAFETY: Getting glyph ID for character '0'.
        let has_zero =
            unsafe { CTFontGetGlyphsForCharacters(font, ch_0.as_ptr(), glyph_0.as_mut_ptr(), 1) };
        let advance_0 = if has_zero && glyph_0[0] != 0 {
            let mut adv = [CGSize::default()];
            // SAFETY: Measuring advance of glyph '0'.
            unsafe {
                CTFontGetAdvancesForGlyphs(font, 0, glyph_0.as_ptr(), adv.as_mut_ptr(), 1);
            }
            adv[0].width as f32
        } else {
            ascent * 0.6
        };

        // Cell math from specs/text-and-rendering.md §2.2
        // ceil() per spec, except when the advance is only a hair above an integer pixel
        // (<= 0.05 px of float noise/rounding), where ceil would add a whole spurious
        // device pixel of gap between every cell.
        let adv_px = advance_0 * scale;
        let cell_w = if adv_px - adv_px.floor() <= 0.05 {
            adv_px.floor().max(1.0)
        } else {
            adv_px.ceil()
        } / scale;
        let cell_h = (((ascent + descent + leading) * line_height) * scale).ceil() / scale;

        FontMetrics {
            cell_width: cell_w,
            cell_height: cell_h,
            ascent,
            descent,
            leading,
            underline_position: ul_pos,
            underline_thickness: ul_thick.max(1.0 / scale),
        }
    }

    /// Resolve a character glyph using the fallback chain:
    /// primary face → CTFontCreateForString → Apple Color Emoji → .LastResort.
    fn resolve_fallback(&mut self, font_idx: usize, face_idx: usize, c: char) -> GlyphRef {
        let loaded = &mut self.fonts[font_idx];
        let font_id = FontId(font_idx as u32);
        let face = &loaded.faces[face_idx];

        // 1. Procedural glyph check
        if procedural::is_procedural(c) {
            let glyph_ref = GlyphRef(self.glyphs.len() as u32);
            self.glyphs.push(GlyphEntry {
                font: face.ct_font.clone(),
                glyph: 0,
                is_color: false,
                is_procedural: true,
                procedural_char: c,
                synthetic_bold: false,
                _synthetic_italic: false,
                font_id,
            });
            return glyph_ref;
        }

        // Encode character as UTF-16
        let mut u16_buf = [0u16; 2];
        let u16_slice = c.encode_utf16(&mut u16_buf);
        let mut glyph_buf = [0u16; 2];

        // 2. Try primary face
        // SAFETY: Checking primary font for glyph.
        let primary_found = unsafe {
            CTFontGetGlyphsForCharacters(
                face.ct_font.as_ptr(),
                u16_slice.as_ptr(),
                glyph_buf.as_mut_ptr(),
                u16_slice.len() as CFIndex,
            )
        };
        if primary_found && glyph_buf[0] != 0 {
            let glyph_ref = GlyphRef(self.glyphs.len() as u32);
            self.glyphs.push(GlyphEntry {
                font: face.ct_font.clone(),
                glyph: glyph_buf[0],
                is_color: false,
                is_procedural: false,
                procedural_char: c,
                synthetic_bold: face.synthetic_bold,
                _synthetic_italic: face.synthetic_italic,
                font_id,
            });
            return glyph_ref;
        }

        // 3. Fallback: check cached fallback font for this scalar
        if let Some(cached_font) = loaded.fallback_fonts.get(&c) {
            // SAFETY: Checking cached fallback font.
            let ok = unsafe {
                CTFontGetGlyphsForCharacters(
                    cached_font.as_ptr(),
                    u16_slice.as_ptr(),
                    glyph_buf.as_mut_ptr(),
                    u16_slice.len() as CFIndex,
                )
            };
            if ok && glyph_buf[0] != 0 {
                // SAFETY: Checking if font has color glyphs trait.
                let traits = unsafe { CTFontGetSymbolicTraits(cached_font.as_ptr()) };
                let is_color = (traits & kCTFontColorGlyphsTrait) != 0;
                let glyph_ref = GlyphRef(self.glyphs.len() as u32);
                self.glyphs.push(GlyphEntry {
                    font: cached_font.clone(),
                    glyph: glyph_buf[0],
                    is_color,
                    is_procedural: false,
                    procedural_char: c,
                    synthetic_bold: false,
                    _synthetic_italic: false,
                    font_id,
                });
                return glyph_ref;
            }
        }

        // 4. Fallback: CTFontCreateForString
        let mut utf8_bytes = [0u8; 4];
        let utf8_str = c.encode_utf8(&mut utf8_bytes);
        let cf_str = Self::make_cf_string(utf8_str);
        let range = CFRange {
            location: 0,
            length: u16_slice.len() as CFIndex,
        };

        // SAFETY: Calling system CTFontCreateForString fallback.
        let fallback_font_raw =
            unsafe { CTFontCreateForString(face.ct_font.as_ptr(), cf_str, range) };
        // SAFETY: Releasing temporary CFString.
        unsafe { CFRelease(cf_str) };

        if !fallback_font_raw.is_null() {
            let fallback_wrapper = CTFontWrapper::new(fallback_font_raw);
            // SAFETY: Checking fallback font for glyph.
            let ok = unsafe {
                CTFontGetGlyphsForCharacters(
                    fallback_font_raw,
                    u16_slice.as_ptr(),
                    glyph_buf.as_mut_ptr(),
                    u16_slice.len() as CFIndex,
                )
            };
            if ok && glyph_buf[0] != 0 {
                // SAFETY: Checking if fallback font has color glyphs.
                let traits = unsafe { CTFontGetSymbolicTraits(fallback_font_raw) };
                let is_color = (traits & kCTFontColorGlyphsTrait) != 0;
                loaded.fallback_fonts.insert(c, fallback_wrapper.clone());

                let glyph_ref = GlyphRef(self.glyphs.len() as u32);
                self.glyphs.push(GlyphEntry {
                    font: fallback_wrapper,
                    glyph: glyph_buf[0],
                    is_color,
                    is_procedural: false,
                    procedural_char: c,
                    synthetic_bold: false,
                    _synthetic_italic: false,
                    font_id,
                });
                return glyph_ref;
            }
        }

        // 5. Fallback: Apple Color Emoji
        if let Some(ref emoji_font) = loaded.emoji_font {
            // SAFETY: Checking Apple Color Emoji.
            let ok = unsafe {
                CTFontGetGlyphsForCharacters(
                    emoji_font.as_ptr(),
                    u16_slice.as_ptr(),
                    glyph_buf.as_mut_ptr(),
                    u16_slice.len() as CFIndex,
                )
            };
            if ok && glyph_buf[0] != 0 {
                let glyph_ref = GlyphRef(self.glyphs.len() as u32);
                self.glyphs.push(GlyphEntry {
                    font: emoji_font.clone(),
                    glyph: glyph_buf[0],
                    is_color: true,
                    is_procedural: false,
                    procedural_char: c,
                    synthetic_bold: false,
                    _synthetic_italic: false,
                    font_id,
                });
                return glyph_ref;
            }
        }

        // 6. Fallback: LastResort font
        if let Some(ref last_resort) = loaded.last_resort_font {
            // SAFETY: Checking LastResort font.
            let ok = unsafe {
                CTFontGetGlyphsForCharacters(
                    last_resort.as_ptr(),
                    u16_slice.as_ptr(),
                    glyph_buf.as_mut_ptr(),
                    u16_slice.len() as CFIndex,
                )
            };
            if ok && glyph_buf[0] != 0 {
                let glyph_ref = GlyphRef(self.glyphs.len() as u32);
                self.glyphs.push(GlyphEntry {
                    font: last_resort.clone(),
                    glyph: glyph_buf[0],
                    is_color: false,
                    is_procedural: false,
                    procedural_char: c,
                    synthetic_bold: false,
                    _synthetic_italic: false,
                    font_id,
                });
                return glyph_ref;
            }
        }

        // Absolute fallback: primary font glyph 0 (.notdef)
        let glyph_ref = GlyphRef(self.glyphs.len() as u32);
        self.glyphs.push(GlyphEntry {
            font: face.ct_font.clone(),
            glyph: 0,
            is_color: false,
            is_procedural: false,
            procedural_char: c,
            synthetic_bold: false,
            _synthetic_italic: false,
            font_id,
        });
        glyph_ref
    }
}

impl CoreTextBackend {
    fn resolve_impl(&mut self, spec: &FontSpec, ui: bool) -> FontId {
        let (faces, emoji_font, last_resort_font) = Self::create_faces(spec, self.scale, ui);
        let line_height = if ui { 1.0 } else { 1.2 };
        let metrics = Self::compute_metrics(
            faces[FACE_REGULAR].ct_font.as_ptr(),
            self.scale,
            line_height,
        );

        let font_id = FontId(self.fonts.len() as u32);
        let font_idx = font_id.0 as usize;

        let loaded = LoadedFont {
            faces,
            metrics,
            _scale: self.scale,
            _line_height: line_height,
            emoji_font,
            last_resort_font,
            fallback_fonts: FxHashMap::default(),
        };
        self.fonts.push(loaded);

        // Pre-populate ASCII fast path [GlyphRef; 95] for characters 32..=126 across all 4 faces
        for face_idx in 0..4 {
            for ascii_code in 32u8..=126u8 {
                let ch = ascii_code as char;
                let glyph_ref = self.resolve_fallback(font_idx, face_idx, ch);
                self.fonts[font_idx].faces[face_idx].ascii[(ascii_code - 32) as usize] = glyph_ref;
            }
        }

        font_id
    }

    /// Glyph lookup in a specific face (regular/bold/italic/bold-italic).
    #[inline]
    fn glyph_face(&mut self, f: FontId, c: char, face_idx: usize) -> GlyphRef {
        let font_idx = f.0 as usize;

        // ASCII fast path: 32 (space) to 126 (~) direct array index lookup
        let ascii_idx = (c as u32).wrapping_sub(32);
        if ascii_idx < 95 {
            return self.fonts[font_idx].faces[face_idx].ascii[ascii_idx as usize];
        }

        // HashMap fallback for non-ASCII
        if let Some(&g) = self.fonts[font_idx].faces[face_idx].non_ascii.get(&c) {
            return g;
        }

        let g = self.resolve_fallback(font_idx, face_idx, c);
        self.fonts[font_idx].faces[face_idx].non_ascii.insert(c, g);
        g
    }
}

impl FontBackend for CoreTextBackend {
    fn resolve(&mut self, spec: &FontSpec) -> FontId {
        self.resolve_impl(spec, false)
    }

    fn resolve_ui(&mut self, size_pt: f32, bold: bool) -> FontId {
        let spec = FontSpec {
            family: None,
            size_pt,
            weight: if bold {
                crate::FontWeight::Bold
            } else {
                crate::FontWeight::Regular
            },
            italic: false,
        };
        self.resolve_impl(&spec, true)
    }

    fn glyph_advance(&mut self, f: FontId, g: GlyphRef) -> f32 {
        if let Some(&a) = self.advances.get(&g.0) {
            return a;
        }
        let entry = &self.glyphs[g.0 as usize];
        let a = if entry.is_procedural {
            self.fonts[entry.font_id.0 as usize].metrics.cell_width
        } else {
            let mut adv = [CGSize::default()];
            // SAFETY: Measuring the advance of one glyph of a live CTFont.
            unsafe {
                CTFontGetAdvancesForGlyphs(
                    entry.font.as_ptr(),
                    0,
                    &entry.glyph,
                    adv.as_mut_ptr(),
                    1,
                );
            }
            adv[0].width as f32
        };
        let _ = f;
        self.advances.insert(g.0, a);
        a
    }

    #[inline]
    fn metrics(&self, f: FontId) -> FontMetrics {
        self.fonts[f.0 as usize].metrics
    }

    #[inline]
    fn glyph(&mut self, f: FontId, c: char) -> GlyphRef {
        self.glyph_face(f, c, FACE_REGULAR)
    }

    #[inline]
    fn glyph_styled(&mut self, f: FontId, c: char, bold: bool, italic: bool) -> GlyphRef {
        let face = match (bold, italic) {
            (false, false) => FACE_REGULAR,
            (true, false) => FACE_BOLD,
            (false, true) => FACE_ITALIC,
            (true, true) => FACE_BOLD_ITALIC,
        };
        self.glyph_face(f, c, face)
    }

    fn shape(&mut self, f: FontId, text: &str, ligatures: bool, out: &mut Vec<ShapedGlyph>) {
        if text.is_empty() {
            return;
        }

        let font_idx = f.0 as usize;
        let cell_w = self.fonts[font_idx].metrics.cell_width;

        // Check if run can skip ligature shaping:
        // When ligatures = false, or when text contains only ASCII letters/digits/spaces
        let skip_shaping = !ligatures
            || !text.chars().any(|c| {
                matches!(
                    c,
                    '!' | '#'
                        | '$'
                        | '%'
                        | '&'
                        | '*'
                        | '+'
                        | '-'
                        | '.'
                        | '/'
                        | ':'
                        | ';'
                        | '<'
                        | '='
                        | '>'
                        | '?'
                        | '@'
                        | '\\'
                        | '^'
                        | '_'
                        | '|'
                        | '~'
                )
            });

        if skip_shaping {
            for (col, ch) in text.chars().enumerate() {
                let g = self.glyph(f, ch);
                out.push(ShapedGlyph {
                    glyph: g,
                    x_offset: 0.0,
                    y_offset: 0.0,
                    x_advance: cell_w,
                    cluster: col as u32,
                });
            }
            return;
        }

        // Ligatures enabled: check LRU cache
        let mut hasher = rustc_hash::FxHasher::default();
        use std::hash::{Hash, Hasher};
        f.0.hash(&mut hasher);
        text.hash(&mut hasher);
        let key = hasher.finish();

        if let Some(cached) = self.shaped_cache.get(&key) {
            out.extend_from_slice(cached);
            return;
        }

        // Shape with CoreText line
        let cf_text = Self::make_cf_string(text);
        let ct_font = self.fonts[font_idx].faces[FACE_REGULAR].ct_font.as_ptr();

        let num_val: i32 = 2; // All ligatures
                              // SAFETY: Creating CFNumber for kCTLigatureAttributeName.
        let cf_num = unsafe {
            CFNumberCreate(
                std::ptr::null(),
                3, /* kCFNumberSInt32Type */
                &num_val as *const i32 as *const c_void,
            )
        };

        // SAFETY: Creating attributes dictionary with font and ligature options.
        let keys = unsafe { [kCTFontAttributeName, kCTLigatureAttributeName] };
        let vals = [ct_font, cf_num];
        let dict = unsafe {
            CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                vals.as_ptr(),
                2,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            )
        };
        // SAFETY: Releasing CFNumber.
        unsafe { CFRelease(cf_num) };

        // SAFETY: Creating CFAttributedString and CTLine.
        let attr_str = unsafe { CFAttributedStringCreate(std::ptr::null(), cf_text, dict) };
        let line = unsafe { CTLineCreateWithAttributedString(attr_str) };
        // SAFETY: Releasing dictionary, attr_str, and cf_text.
        unsafe {
            CFRelease(dict);
            CFRelease(attr_str);
            CFRelease(cf_text);
        }

        let mut shaped_run = Vec::new();
        // SAFETY: Extracting runs from CTLine.
        let runs = unsafe { CTLineGetGlyphRuns(line) };
        let run_count = if !runs.is_null() {
            // SAFETY: Querying run count from CFArray.
            unsafe { CFArrayGetCount(runs) }
        } else {
            0
        };

        let chars_vec: Vec<char> = text.chars().collect();
        let total_chars = chars_vec.len();

        for i in 0..run_count {
            // SAFETY: Getting CTRun from CFArray.
            let run = unsafe { CFArrayGetValueAtIndex(runs, i) };
            if run.is_null() {
                continue;
            }
            // SAFETY: Querying glyph count in CTRun.
            let count = unsafe { CTRunGetGlyphCount(run) } as usize;
            if count == 0 {
                continue;
            }

            let mut glyph_buf = vec![0u16; count];
            let mut pos_buf = vec![CGPoint::default(); count];
            let mut str_indices = vec![0isize; count];

            let range = CFRange {
                location: 0,
                length: count as CFIndex,
            };
            // SAFETY: Extracting glyphs, positions, and string indices from CTRun.
            unsafe {
                CTRunGetGlyphs(run, range, glyph_buf.as_mut_ptr());
                CTRunGetPositions(run, range, pos_buf.as_mut_ptr());
                CTRunGetStringIndices(run, range, str_indices.as_mut_ptr());
            }

            for j in 0..count {
                let glyph_id = glyph_buf[j];
                let cluster = str_indices[j].max(0) as u32;

                // Register glyph entry if not present
                let glyph_ref = GlyphRef(self.glyphs.len() as u32);
                self.glyphs.push(GlyphEntry {
                    font: self.fonts[font_idx].faces[FACE_REGULAR].ct_font.clone(),
                    glyph: glyph_id,
                    is_color: false,
                    is_procedural: false,
                    procedural_char: if (cluster as usize) < total_chars {
                        chars_vec[cluster as usize]
                    } else {
                        ' '
                    },
                    synthetic_bold: false,
                    _synthetic_italic: false,
                    font_id: f,
                });

                let x_advance = if j + 1 < count {
                    (pos_buf[j + 1].x - pos_buf[j].x) as f32
                } else {
                    cell_w
                };

                shaped_run.push(ShapedGlyph {
                    glyph: glyph_ref,
                    x_offset: pos_buf[j].x as f32,
                    y_offset: pos_buf[j].y as f32,
                    x_advance,
                    cluster,
                });
            }
        }

        // SAFETY: Releasing CTLine.
        unsafe { CFRelease(line) };

        // Insert into shaped run cache with LRU 4096 cap
        if self.shaped_cache.len() >= 4096 {
            if let Some(old_key) = self.shaped_lru.pop_front() {
                self.shaped_cache.remove(&old_key);
            }
        }
        self.shaped_lru.push_back(key);
        self.shaped_cache.insert(key, shaped_run.clone());

        out.extend(shaped_run);
    }

    fn rasterize(&mut self, g: GlyphRef, scale: f32, out: &mut RasterBuf) -> RasterInfo {
        let entry = &self.glyphs[g.0 as usize];

        // 1. Procedural glyph rasterization
        if entry.is_procedural {
            let m = self.fonts[entry.font_id.0 as usize].metrics;
            let w_px = (m.cell_width * scale).round() as u32;
            let h_px = (m.cell_height * scale).round() as u32;
            let len = (w_px * h_px) as usize;
            out.data.resize(len, 0);
            procedural::rasterize_procedural(entry.procedural_char, w_px, h_px, &mut out.data);
            return RasterInfo::Mask {
                width: w_px,
                height: h_px,
                left: 0,
                top: h_px as i32,
            };
        }

        // 2. Measure glyph bounds via CoreText
        let mut bounds = CGRect::default();
        // SAFETY: Measuring glyph bounding box in CoreText.
        unsafe {
            CTFontGetBoundingRectsForGlyphs(entry.font.as_ptr(), 0, &entry.glyph, &mut bounds, 1);
        }

        let s = scale as f64;
        // Bitmap placement: `left`/`bottom` are the bitmap's lower-left corner in device
        // pixels relative to the glyph origin (y up). The glyph is drawn at
        // `(-left/s, -bottom/s)` so bitmap pixel (0,0) sits exactly at (left, bottom) and
        // the returned `left`/`top` agree with what was drawn, sub-pixel offset included.
        let extra_right = if !entry.is_color && entry.synthetic_bold {
            1
        } else {
            0
        };
        let (left, bottom, w, h) = glyph_box(
            bounds.origin.x,
            bounds.origin.y,
            bounds.size.width,
            bounds.size.height,
            s,
            extra_right,
        );
        let (w, h) = (w as usize, h as usize);

        if bounds.size.width <= 0.0 || bounds.size.height <= 0.0 {
            out.data.clear();
            return if entry.is_color {
                RasterInfo::Color {
                    width: 0,
                    height: 0,
                    left,
                    top: bottom + h as i32,
                }
            } else {
                RasterInfo::Mask {
                    width: 0,
                    height: 0,
                    left,
                    top: bottom + h as i32,
                }
            };
        }

        // 3. Color glyphs (Apple Color Emoji) -> BGRA8 premultiplied
        if entry.is_color {
            let byte_len = w * h * 4;
            out.data.resize(byte_len, 0);
            out.data.fill(0);

            // SAFETY: Creating BGRA8 bitmap context for color emoji.
            unsafe {
                let rgb_space = CGColorSpaceCreateDeviceRGB();
                let ctx = CGBitmapContextCreate(
                    out.data.as_mut_ptr() as *mut c_void,
                    w,
                    h,
                    8,
                    w * 4,
                    rgb_space,
                    kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little,
                );
                CGContextSetShouldAntialias(ctx, true);
                // Fonts are sized in points; scale the context so the glyph fills the
                // device-pixel bitmap instead of 1/scale of it.
                CGContextScaleCTM(ctx, s, s);

                let pos = CGPoint {
                    x: -(left as f64) / s,
                    y: -(bottom as f64) / s,
                };
                CTFontDrawGlyphs(entry.font.as_ptr(), &entry.glyph, &pos, 1, ctx);

                CGContextRelease(ctx);
                CFRelease(rgb_space);
            }

            return RasterInfo::Color {
                width: w as u32,
                height: h as u32,
                left,
                top: bottom + h as i32,
            };
        }

        // 4. Text glyphs -> Grayscale coverage A8
        let byte_len = w * h;
        out.data.resize(byte_len, 0);
        out.data.fill(0);

        // SAFETY: Creating A8 bitmap context with font smoothing for crisp text coverage.
        unsafe {
            let ctx = CGBitmapContextCreate(
                out.data.as_mut_ptr() as *mut c_void,
                w,
                h,
                8,
                w,
                std::ptr::null(), // NULL for kCGImageAlphaOnly
                kCGImageAlphaOnly,
            );
            CGContextSetShouldAntialias(ctx, true);
            CGContextSetAllowsFontSmoothing(ctx, true);
            CGContextSetShouldSmoothFonts(ctx, self.thicken);
            CGContextSetGrayFillColor(ctx, 1.0, 1.0);
            // Fonts are sized in points; scale the context to device pixels.
            CGContextScaleCTM(ctx, s, s);

            let pos = CGPoint {
                x: -(left as f64) / s,
                y: -(bottom as f64) / s,
            };
            CTFontDrawGlyphs(entry.font.as_ptr(), &entry.glyph, &pos, 1, ctx);

            // Fake bold synthesis: draw again offset by 1 physical pixel
            if entry.synthetic_bold {
                let pos_bold = CGPoint {
                    x: pos.x + 1.0 / s,
                    y: pos.y,
                };
                CTFontDrawGlyphs(entry.font.as_ptr(), &entry.glyph, &pos_bold, 1, ctx);
            }

            CGContextRelease(ctx);
        }

        RasterInfo::Mask {
            width: w as u32,
            height: h as u32,
            left,
            top: bottom + h as i32,
        }
    }
}

/// Device-pixel bitmap box for a glyph with bounds `(ox, oy, bw, bh)` (points, y up) at
/// scale `s`: returns `(left, bottom, width, height)` where `left`/`bottom` is the bitmap's
/// lower-left corner relative to the glyph origin. Two pixels of padding on every side keep
/// anti-aliased and font-smoothing-dilated edges (and the fractional part of the origin) inside the bitmap;
/// `extra_right` widens the box for fake-bold smear.
fn glyph_box(ox: f64, oy: f64, bw: f64, bh: f64, s: f64, extra_right: i32) -> (i32, i32, u32, u32) {
    const PAD: i32 = 2;
    let left = (ox * s).floor() as i32 - PAD;
    let bottom = (oy * s).floor() as i32 - PAD;
    let right = ((ox + bw) * s).ceil() as i32 + PAD + extra_right;
    let top = ((oy + bh) * s).ceil() as i32 + PAD;
    (
        left,
        bottom,
        (right - left).max(1) as u32,
        (top - bottom).max(1) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FontWeight;

    #[test]
    fn glyph_box_covers_fractional_bounds() {
        // origin 0.3pt, width 5pt at 2x: ink spans device px [0.6, 10.6].
        let (left, bottom, w, h) = glyph_box(0.3, -0.7, 5.0, 8.0, 2.0, 0);
        assert_eq!(left, -2);
        assert_eq!(bottom, -4);
        // right = ceil(10.6)+2 = 13, top = ceil(14.6)+2 = 17.
        assert_eq!(w, 15);
        assert_eq!(h, 21);
        let (_, _, w2, _) = glyph_box(0.3, -0.7, 5.0, 8.0, 2.0, 1);
        assert_eq!(w2, 16);
    }

    #[test]
    fn rasterized_ink_stays_inside_bitmap() {
        let mut b = CoreTextBackend::new();
        let f = b.resolve(&FontSpec::default());
        let g = b.glyph(f, 'W');
        let mut buf = RasterBuf::default();
        let RasterInfo::Mask { width, height, .. } = b.rasterize(g, 2.0, &mut buf) else {
            panic!("expected mask");
        };
        let (w, h) = (width as usize, height as usize);
        assert!(buf.data.iter().any(|&p| p > 0));
        // Padding rows/columns on all four sides stay (nearly) empty.
        let max_edge = (0..h)
            .flat_map(|y| [buf.data[y * w], buf.data[y * w + w - 1]])
            .chain((0..w).flat_map(|x| [buf.data[x], buf.data[(h - 1) * w + x]]))
            .max()
            .unwrap();
        let dbg = (
            (0..h).map(|y| buf.data[y * w]).max(),
            (0..h).map(|y| buf.data[y * w + w - 1]).max(),
            (0..w).map(|x| buf.data[x]).max(),
            (0..w).map(|x| buf.data[(h - 1) * w + x]).max(),
            w,
            h,
        );
        assert!(max_edge < 64, "{dbg:?} ink touches bitmap edge: {max_edge}");
    }

    #[test]
    fn test_coretext_font_resolution() {
        let mut backend = CoreTextBackend::new();
        let spec = FontSpec::default();
        let id = backend.resolve(&spec);
        assert_eq!(id, FontId(0));

        let m = backend.metrics(id);
        assert!(m.cell_width > 0.0);
        assert!(m.cell_height > 0.0);
        assert!(m.ascent > 0.0);
    }

    #[test]
    fn test_ligature_shaping() {
        let mut backend = CoreTextBackend::new();
        let ligature_fonts = ["Fira Code", "JetBrains Mono", "Cascadia Code", "Hasklig"];
        let mut tested = false;

        for font_name in ligature_fonts {
            let spec = FontSpec {
                family: Some(font_name.to_string()),
                size_pt: 13.0,
                weight: FontWeight::Regular,
                italic: false,
            };
            let id = backend.resolve(&spec);

            let mut out_lig = Vec::new();
            backend.shape(id, "->", true, &mut out_lig);
            let mut out_no_lig = Vec::new();
            backend.shape(id, "->", false, &mut out_no_lig);

            assert_eq!(out_no_lig.len(), 2);
            if out_lig.len() < 2 {
                println!("Tested ligature font '{}' successfully!", font_name);
                tested = true;
                break;
            }
        }

        if !tested {
            println!("No ligature font found installed, skipped ligature font test.");
        }
    }

    #[test]
    fn test_ascii_fast_path_lookups() {
        let mut backend = CoreTextBackend::new();
        let id = backend.resolve(&FontSpec::default());

        // Printable ASCII
        for c in ' '..='~' {
            let g = backend.glyph(id, c);
            assert!(g.0 < u32::MAX);
        }
    }

    #[test]
    fn test_fallback_chain_cjk_emoji_symbols() {
        let mut backend = CoreTextBackend::new();
        let id = backend.resolve(&FontSpec::default());

        // CJK
        let g_cjk = backend.glyph(id, '你');
        assert!(g_cjk.0 < u32::MAX);

        // Emoji
        let g_emoji = backend.glyph(id, '🚀');
        assert!(g_emoji.0 < u32::MAX);

        // Symbol
        let g_sym = backend.glyph(id, '∑');
        assert!(g_sym.0 < u32::MAX);

        // Procedural
        let g_box = backend.glyph(id, '─');
        assert!(g_box.0 < u32::MAX);

        let mut buf = RasterBuf::default();

        // Rasterize CJK (mask)
        let info_cjk = backend.rasterize(g_cjk, 2.0, &mut buf);
        assert!(matches!(info_cjk, RasterInfo::Mask { .. }));
        assert!(!buf.data.is_empty());

        // Rasterize Emoji (color)
        let info_emoji = backend.rasterize(g_emoji, 2.0, &mut buf);
        assert!(matches!(info_emoji, RasterInfo::Color { .. }));
        assert!(!buf.data.is_empty());

        // Rasterize Procedural (mask)
        let info_box = backend.rasterize(g_box, 2.0, &mut buf);
        assert!(matches!(info_box, RasterInfo::Mask { .. }));
        assert!(!buf.data.is_empty());
    }

    #[test]
    fn test_glyph_ink_fills_scaled_bitmap() {
        let mut backend = CoreTextBackend::new();
        let id = backend.resolve(&FontSpec::default());
        let mut buf = RasterBuf::default();

        for c in ['M', 'r', 'W'] {
            let g = backend.glyph(id, c);
            let RasterInfo::Mask { width, height, .. } = backend.rasterize(g, 2.0, &mut buf) else {
                panic!("expected mask for {c:?}");
            };
            let (w, h) = (width as usize, height as usize);
            let inked = |x: usize, y: usize| buf.data[y * w + x] > 64;
            let ink_rows = (0..h).filter(|&y| (0..w).any(|x| inked(x, y))).count();
            let ink_cols = (0..w).filter(|&x| (0..h).any(|y| inked(x, y))).count();
            // Bitmap is the glyph bounds at device scale plus a 1px border on each side;
            // the drawn ink must cover it, not 1/scale of it.
            assert!(
                ink_rows * 4 >= (h - 2) * 3,
                "{c:?}: {ink_rows} ink rows of {h}"
            );
            assert!(
                ink_cols * 4 >= (w - 2) * 3,
                "{c:?}: {ink_cols} ink cols of {w}"
            );
        }
    }

    #[test]
    fn test_benchmark_10k_ascii_lookups() {
        let mut backend = CoreTextBackend::new();
        let id = backend.resolve(&FontSpec::default());

        let chars: Vec<char> = (0..10_000)
            .map(|i| ((32 + (i % 95)) as u8) as char)
            .collect();

        let start = std::time::Instant::now();
        let mut sum = 0u64;
        for &ch in &chars {
            let g = backend.glyph(id, ch);
            sum += g.0 as u64;
        }
        let elapsed = start.elapsed();
        assert!(sum > 0);
        println!("10k ASCII glyph lookups took: {:?}", elapsed);
        let budget_us = if cfg!(debug_assertions) { 200 } else { 50 };
        assert!(
            elapsed.as_micros() < budget_us,
            "10k ASCII glyph lookups took {} µs, expected < {} µs",
            elapsed.as_micros(),
            budget_us
        );
    }
}
