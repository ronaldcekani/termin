//! Color parsing, linear blending, and contrast calculations.
//!
//! See `docs/design/design-system.md §2`.

use serde::{Deserialize, Deserializer, Serializer};
use std::fmt;
use termin_core::Rgba;

/// Error returned when color parsing fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColorParseError {
    Empty,
    InvalidHex(String),
    InvalidLength(usize),
    InvalidPercentage(String),
}

impl fmt::Display for ColorParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "color string cannot be empty"),
            Self::InvalidHex(s) => write!(f, "invalid hex characters in color '{}'", s),
            Self::InvalidLength(len) => {
                write!(
                    f,
                    "invalid hex color length {}, expected 3, 4, 6, or 8",
                    len
                )
            }
            Self::InvalidPercentage(s) => write!(f, "invalid alpha percentage '{}'", s),
        }
    }
}

impl std::error::Error for ColorParseError {}

/// Parse a color string in hex format (`#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`,
/// or `#RRGGBB @ XX%`).
pub fn parse_color(s: &str) -> Result<Rgba, ColorParseError> {
    let s = s.trim();
    if s.is_empty() {
        return Err(ColorParseError::Empty);
    }

    // Check for alpha percentage suffix, e.g. "#3FD0C9 @ 28%" or "#3FD0C9 @ 0.28"
    if let Some((hex_part, alpha_part)) = s.split_once('@') {
        let base = parse_color(hex_part)?;
        let alpha_str = alpha_part.trim().trim_end_matches('%').trim();
        let alpha_frac = if alpha_part.contains('%') {
            alpha_str
                .parse::<f32>()
                .map(|p| p / 100.0)
                .map_err(|_| ColorParseError::InvalidPercentage(alpha_part.to_string()))?
        } else {
            alpha_str
                .parse::<f32>()
                .map_err(|_| ColorParseError::InvalidPercentage(alpha_part.to_string()))?
        };
        return Ok(with_alpha(base, alpha_frac));
    }

    let hex = s.strip_prefix('#').unwrap_or(s);
    match hex.len() {
        3 => {
            // #RGB -> #RRGGBB (alpha 255)
            let r = parse_hex_digit(hex.as_bytes()[0])?;
            let g = parse_hex_digit(hex.as_bytes()[1])?;
            let b = parse_hex_digit(hex.as_bytes()[2])?;
            Ok(Rgba::from_rgba(r * 17, g * 17, b * 17, 255))
        }
        4 => {
            // #RGBA -> #RRGGBBAA
            let r = parse_hex_digit(hex.as_bytes()[0])?;
            let g = parse_hex_digit(hex.as_bytes()[1])?;
            let b = parse_hex_digit(hex.as_bytes()[2])?;
            let a = parse_hex_digit(hex.as_bytes()[3])?;
            Ok(Rgba::from_rgba(r * 17, g * 17, b * 17, a * 17))
        }
        6 => {
            // #RRGGBB
            let r = parse_hex_byte(&hex[0..2])?;
            let g = parse_hex_byte(&hex[2..4])?;
            let b = parse_hex_byte(&hex[4..6])?;
            Ok(Rgba::from_rgba(r, g, b, 255))
        }
        8 => {
            // #RRGGBBAA
            let r = parse_hex_byte(&hex[0..2])?;
            let g = parse_hex_byte(&hex[2..4])?;
            let b = parse_hex_byte(&hex[4..6])?;
            let a = parse_hex_byte(&hex[6..8])?;
            Ok(Rgba::from_rgba(r, g, b, a))
        }
        other => Err(ColorParseError::InvalidLength(other)),
    }
}

fn parse_hex_digit(byte: u8) -> Result<u8, ColorParseError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(ColorParseError::InvalidHex((byte as char).to_string())),
    }
}

fn parse_hex_byte(s: &str) -> Result<u8, ColorParseError> {
    u8::from_str_radix(s, 16).map_err(|_| ColorParseError::InvalidHex(s.to_string()))
}

/// Format an `Rgba` value as a hex string (`#RRGGBB` or `#RRGGBBAA`).
pub fn format_hex_color(c: Rgba) -> String {
    if c.a() == 255 {
        format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", c.r(), c.g(), c.b(), c.a())
    }
}

/// Convert normalized sRGB channel to linear light.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Convert linear light channel to normalized sRGB.
pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Mix two colors in linear space according to factor `t` in range 0.0..=1.0.
///
/// See `docs/design/design-system.md §2.4`.
pub fn mix_linear(c1: Rgba, c2: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    let r1 = srgb_to_linear(c1.r() as f32 / 255.0);
    let g1 = srgb_to_linear(c1.g() as f32 / 255.0);
    let b1 = srgb_to_linear(c1.b() as f32 / 255.0);

    let r2 = srgb_to_linear(c2.r() as f32 / 255.0);
    let g2 = srgb_to_linear(c2.g() as f32 / 255.0);
    let b2 = srgb_to_linear(c2.b() as f32 / 255.0);

    let r_lin = r1 * (1.0 - t) + r2 * t;
    let g_lin = g1 * (1.0 - t) + g2 * t;
    let b_lin = b1 * (1.0 - t) + b2 * t;

    let r = (linear_to_srgb(r_lin) * 255.0).round().clamp(0.0, 255.0) as u8;
    let g = (linear_to_srgb(g_lin) * 255.0).round().clamp(0.0, 255.0) as u8;
    let b = (linear_to_srgb(b_lin) * 255.0).round().clamp(0.0, 255.0) as u8;

    Rgba::from_rgba(r, g, b, 255)
}

/// Compute relative luminance under WCAG 2.1 definitions.
pub fn relative_luminance(c: Rgba) -> f32 {
    let r = srgb_to_linear(c.r() as f32 / 255.0);
    let g = srgb_to_linear(c.g() as f32 / 255.0);
    let b = srgb_to_linear(c.b() as f32 / 255.0);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Compute WCAG contrast ratio between two colors.
///
/// Returns a ratio in range 1.0..=21.0.
pub fn contrast_ratio(c1: Rgba, c2: Rgba) -> f32 {
    let l1 = relative_luminance(c1);
    let l2 = relative_luminance(c2);
    let lighter = l1.max(l2);
    let darker = l1.min(l2);
    (lighter + 0.05) / (darker + 0.05)
}

/// Create a new color with an adjusted alpha channel (0.0..=1.0).
pub fn with_alpha(c: Rgba, alpha_frac: f32) -> Rgba {
    let a = (alpha_frac.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgba::from_rgba(c.r(), c.g(), c.b(), a)
}

/// Serde helper for `Rgba` as hex strings.
pub mod serde_rgba {
    use super::*;

    pub fn serialize<S>(color: &Rgba, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format_hex_color(*color))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Rgba, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        parse_color(&s).map_err(serde::de::Error::custom)
    }
}

/// Serde helper for `Option<Rgba>` as hex strings.
pub mod serde_opt_rgba {
    use super::*;

    pub fn serialize<S>(color: &Option<Rgba>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match color {
            Some(c) => serializer.serialize_some(&format_hex_color(*c)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Rgba>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<String>::deserialize(deserializer)?;
        match opt {
            Some(s) => parse_color(&s).map(Some).map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

/// Serde helper for `[Rgba; 16]` ANSI color arrays.
pub mod serde_ansi_array {
    use super::*;

    pub fn serialize<S>(ansi: &[Rgba; 16], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(16))?;
        for c in ansi {
            seq.serialize_element(&format_hex_color(*c))?;
        }
        seq.end()
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[Rgba; 16], D::Error>
    where
        D: Deserializer<'de>,
    {
        let list = Vec::<String>::deserialize(deserializer)?;
        if list.len() != 16 {
            return Err(serde::de::Error::custom(format!(
                "expected 16 colors for ansi, got {}",
                list.len()
            )));
        }
        let mut res = [Rgba::BLACK; 16];
        for (i, s) in list.iter().enumerate() {
            res[i] = parse_color(s).map_err(serde::de::Error::custom)?;
        }
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_color_formats() {
        assert_eq!(parse_color("#FFF").unwrap(), Rgba::from_rgb(255, 255, 255));
        assert_eq!(
            parse_color("#3FD0C9").unwrap(),
            Rgba::from_rgb(0x3F, 0xD0, 0xC9)
        );
        let c_alpha = parse_color("#3FD0C9 @ 28%").unwrap();
        assert_eq!(c_alpha.r(), 0x3F);
        assert_eq!(c_alpha.g(), 0xD0);
        assert_eq!(c_alpha.b(), 0xC9);
        assert_eq!(c_alpha.a(), 71);
    }

    #[test]
    fn test_contrast_ratio() {
        let white = Rgba::from_rgb(255, 255, 255);
        let black = Rgba::from_rgb(0, 0, 0);
        let ratio = contrast_ratio(white, black);
        assert!((ratio - 21.0).abs() < 0.1);
    }
}
