//! CoreFoundation, CoreText, and CoreGraphics FFI bindings.
//!
//! See `docs/architecture.md §2, §7` and `docs/specs/text-and-rendering.md §2`.

#![allow(non_upper_case_globals)]
#![allow(non_snake_case)]

use std::ffi::c_void;

pub type CFTypeRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFArrayRef = *const c_void;
pub type CFAttributedStringRef = *const c_void;
pub type CTFontRef = *const c_void;
pub type CTLineRef = *const c_void;
pub type CTRunRef = *const c_void;
pub type CGContextRef = *mut c_void;
pub type CGColorSpaceRef = *const c_void;
pub type CGGlyph = u16;
pub type UniChar = u16;
pub type CFIndex = isize;
pub type CGFloat = f64;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct CGPoint {
    pub x: CGFloat,
    pub y: CGFloat,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct CGSize {
    pub width: CGFloat,
    pub height: CGFloat,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CGAffineTransform {
    pub a: CGFloat,
    pub b: CGFloat,
    pub c: CGFloat,
    pub d: CGFloat,
    pub tx: CGFloat,
    pub ty: CGFloat,
}

impl Default for CGAffineTransform {
    fn default() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CFRange {
    pub location: CFIndex,
    pub length: CFIndex,
}

pub const kCTFontUIFontSystem: u32 = 0;
pub const kCTFontUIFontUserFixedPitch: u32 = 1;

pub const kCTFontItalicTrait: u32 = 1 << 0;
pub const kCTFontBoldTrait: u32 = 1 << 1;
pub const kCTFontColorGlyphsTrait: u32 = 1 << 13;

pub const kCFStringEncodingUTF8: u32 = 0x08000100;

pub const kCGImageAlphaNone: u32 = 0;
pub const kCGImageAlphaPremultipliedLast: u32 = 1;
pub const kCGImageAlphaPremultipliedFirst: u32 = 2;
pub const kCGImageAlphaNoneSkipLast: u32 = 5;
pub const kCGImageAlphaNoneSkipFirst: u32 = 6;
pub const kCGImageAlphaOnly: u32 = 7;
pub const kCGBitmapByteOrder32Little: u32 = 2 << 12;

pub const kCTLineTruncationStart: u32 = 0;
pub const kCTLineTruncationEnd: u32 = 1;
pub const kCTLineTruncationMiddle: u32 = 2;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub static kCFTypeDictionaryKeyCallBacks: [usize; 6];
    pub static kCFTypeDictionaryValueCallBacks: [usize; 5];

    pub fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    pub fn CFRelease(cf: CFTypeRef);

    pub fn CFStringCreateWithBytes(
        alloc: CFTypeRef,
        bytes: *const u8,
        num_bytes: CFIndex,
        encoding: u32,
        is_external_representation: bool,
    ) -> CFStringRef;

    pub fn CFStringGetLength(the_string: CFStringRef) -> CFIndex;
    pub fn CFStringGetCharacters(the_string: CFStringRef, range: CFRange, buffer: *mut UniChar);

    pub fn CFArrayGetCount(the_array: CFArrayRef) -> CFIndex;
    pub fn CFArrayGetValueAtIndex(the_array: CFArrayRef, idx: CFIndex) -> CFTypeRef;

    pub fn CFDictionaryCreate(
        allocator: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        num_values: CFIndex,
        key_callbacks: *const [usize; 6],
        value_callbacks: *const [usize; 5],
    ) -> CFDictionaryRef;

    pub fn CFNumberCreate(
        allocator: CFTypeRef,
        the_type: CFIndex,
        value_ptr: *const c_void,
    ) -> CFTypeRef;
}

#[link(name = "CoreText", kind = "framework")]
extern "C" {
    pub static kCTFontAttributeName: CFStringRef;
    pub static kCTLigatureAttributeName: CFStringRef;

    pub fn CTFontCreateWithName(
        name: CFStringRef,
        size: CGFloat,
        matrix: *const CGAffineTransform,
    ) -> CTFontRef;
    pub fn CTFontCreateUIFontForLanguage(
        ui_type: u32,
        size: CGFloat,
        language: CFStringRef,
    ) -> CTFontRef;
    pub fn CTFontCreateCopyWithSymbolicTraits(
        font: CTFontRef,
        size: CGFloat,
        matrix: *const CGAffineTransform,
        sym_trait_value: u32,
        sym_trait_mask: u32,
    ) -> CTFontRef;
    pub fn CTFontCreateCopyWithAttributes(
        font: CTFontRef,
        size: CGFloat,
        matrix: *const CGAffineTransform,
        attributes: CFTypeRef,
    ) -> CTFontRef;
    pub fn CTFontCreateForString(
        current_font: CTFontRef,
        string: CFStringRef,
        range: CFRange,
    ) -> CTFontRef;

    pub fn CTFontCopyPostScriptName(font: CTFontRef) -> CFStringRef;
    pub fn CTFontCopyFamilyName(font: CTFontRef) -> CFStringRef;
    pub fn CTFontCopyDisplayName(font: CTFontRef) -> CFStringRef;
    pub fn CTFontGetSymbolicTraits(font: CTFontRef) -> u32;

    pub fn CTFontGetGlyphsForCharacters(
        font: CTFontRef,
        characters: *const UniChar,
        glyphs: *mut CGGlyph,
        count: CFIndex,
    ) -> bool;

    pub fn CTFontGetAdvancesForGlyphs(
        font: CTFontRef,
        orientation: u32,
        glyphs: *const CGGlyph,
        advances: *mut CGSize,
        count: CFIndex,
    ) -> f64;

    pub fn CTFontGetBoundingRectsForGlyphs(
        font: CTFontRef,
        orientation: u32,
        glyphs: *const CGGlyph,
        rects: *mut CGRect,
        count: CFIndex,
    ) -> CGRect;

    pub fn CTFontGetAscent(font: CTFontRef) -> CGFloat;
    pub fn CTFontGetDescent(font: CTFontRef) -> CGFloat;
    pub fn CTFontGetLeading(font: CTFontRef) -> CGFloat;
    pub fn CTFontGetUnderlinePosition(font: CTFontRef) -> CGFloat;
    pub fn CTFontGetUnderlineThickness(font: CTFontRef) -> CGFloat;
    pub fn CTFontGetUnitsPerEm(font: CTFontRef) -> u32;

    pub fn CTFontDrawGlyphs(
        font: CTFontRef,
        glyphs: *const CGGlyph,
        positions: *const CGPoint,
        count: usize,
        context: CGContextRef,
    );

    pub fn CFAttributedStringCreate(
        alloc: CFTypeRef,
        str: CFStringRef,
        attributes: CFDictionaryRef,
    ) -> CFAttributedStringRef;

    pub fn CTLineCreateWithAttributedString(attr_str: CFAttributedStringRef) -> CTLineRef;
    pub fn CTLineGetGlyphRuns(line: CTLineRef) -> CFArrayRef;
    pub fn CTLineGetTypographicBounds(
        line: CTLineRef,
        ascent: *mut CGFloat,
        descent: *mut CGFloat,
        leading: *mut CGFloat,
    ) -> f64;
    pub fn CTLineGetOffsetForStringIndex(
        line: CTLineRef,
        char_index: CFIndex,
        secondary_offset: *mut CGFloat,
    ) -> CGFloat;
    pub fn CTLineCreateTruncatedLine(
        line: CTLineRef,
        width: f64,
        truncation_type: u32,
        truncation_token: CTLineRef,
    ) -> CTLineRef;

    pub fn CTRunGetGlyphCount(run: CTRunRef) -> CFIndex;
    pub fn CTRunGetGlyphs(run: CTRunRef, range: CFRange, buffer: *mut CGGlyph);
    pub fn CTRunGetPositions(run: CTRunRef, range: CFRange, buffer: *mut CGPoint);
    pub fn CTRunGetStringIndices(run: CTRunRef, range: CFRange, buffer: *mut CFIndex);
    pub fn CTRunGetAttributes(run: CTRunRef) -> CFDictionaryRef;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    pub fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: CGColorSpaceRef,
        bitmap_info: u32,
    ) -> CGContextRef;

    pub fn CGColorSpaceCreateDeviceGray() -> CGColorSpaceRef;
    pub fn CGColorSpaceCreateDeviceRGB() -> CGColorSpaceRef;

    pub fn CGContextRelease(c: CGContextRef);
    pub fn CGContextSetShouldAntialias(c: CGContextRef, should_antialias: bool);
    pub fn CGContextSetAllowsFontSmoothing(c: CGContextRef, allows_font_smoothing: bool);
    pub fn CGContextSetShouldSmoothFonts(c: CGContextRef, should_smooth_fonts: bool);
    pub fn CGContextSetGrayFillColor(c: CGContextRef, gray: CGFloat, alpha: CGFloat);
    pub fn CGContextSetRGBFillColor(
        c: CGContextRef,
        red: CGFloat,
        green: CGFloat,
        blue: CGFloat,
        alpha: CGFloat,
    );
    pub fn CGContextSetTextDrawingMode(c: CGContextRef, mode: i32);
    pub fn CGContextScaleCTM(c: CGContextRef, sx: CGFloat, sy: CGFloat);
}
