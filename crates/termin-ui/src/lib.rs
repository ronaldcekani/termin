//! UI layout, theme tokens, widgets, and DrawList builder (`termin-ui`).
//!
//! See `docs/design/design-system.md` and `docs/design/components.md`.

pub mod theme;

pub use theme::Tokens;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokens_default() {
        let tokens = Tokens::default();
        assert_eq!(tokens.bg_app.r(), 0x0C);
    }
}
