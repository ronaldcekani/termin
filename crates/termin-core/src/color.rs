//! Packed RGBA8 color representation.
//!
//! See `docs/architecture.md §3.1`.

use std::fmt;

/// Packed RGBA8 color, straight alpha (4 bytes).
///
/// Encoded as `0xRRGGBBAA`.
/// See `docs/architecture.md §3.1`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Rgba(pub u32);

impl Rgba {
    pub const TRANSPARENT: Self = Self(0);
    pub const BLACK: Self = Self::from_rgb(0, 0, 0);
    pub const WHITE: Self = Self::from_rgb(255, 255, 255);

    /// Create an `Rgba` value from raw components (0-255).
    pub const fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | (a as u32))
    }

    /// Create an opaque `Rgba` value from RGB components (alpha = 255).
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self::from_rgba(r, g, b, 255)
    }

    /// Create from packed `u32` value (`0xRRGGBBAA`).
    pub const fn from_u32(val: u32) -> Self {
        Self(val)
    }

    /// Raw packed `u32` value.
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    /// Red channel (0-255).
    pub const fn r(self) -> u8 {
        ((self.0 >> 24) & 0xFF) as u8
    }

    /// Green channel (0-255).
    pub const fn g(self) -> u8 {
        ((self.0 >> 16) & 0xFF) as u8
    }

    /// Blue channel (0-255).
    pub const fn b(self) -> u8 {
        ((self.0 >> 8) & 0xFF) as u8
    }

    /// Alpha channel (0-255).
    pub const fn a(self) -> u8 {
        (self.0 & 0xFF) as u8
    }

    /// Normalized float components `[r, g, b, a]` in range 0.0..1.0.
    pub fn to_array_f32(self) -> [f32; 4] {
        [
            self.r() as f32 / 255.0,
            self.g() as f32 / 255.0,
            self.b() as f32 / 255.0,
            self.a() as f32 / 255.0,
        ]
    }
}

impl fmt::Display for Rgba {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "#{:02X}{:02X}{:02X}{:02X}",
            self.r(),
            self.g(),
            self.b(),
            self.a()
        )
    }
}

impl From<u32> for Rgba {
    fn from(val: u32) -> Self {
        Self(val)
    }
}

impl From<Rgba> for u32 {
    fn from(c: Rgba) -> Self {
        c.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgba_size_and_channels() {
        assert_eq!(std::mem::size_of::<Rgba>(), 4);
        let color = Rgba::from_rgba(0x12, 0x34, 0x56, 0x78);
        assert_eq!(color.r(), 0x12);
        assert_eq!(color.g(), 0x34);
        assert_eq!(color.b(), 0x56);
        assert_eq!(color.a(), 0x78);
        assert_eq!(color.as_u32(), 0x12345678);
    }
}
