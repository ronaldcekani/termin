//! Compact string type inline up to 22 bytes, else heap allocated.
//!
//! See `docs/architecture.md §3.1`.

use std::borrow::Borrow;
use std::fmt;
use std::ops::Deref;

const INLINE_CAP: usize = 22;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Repr {
    Inline { len: u8, buf: [u8; INLINE_CAP] },
    Heap(Box<str>),
}

/// Compact string storing up to 22 bytes inline without heap allocation.
///
/// Exactly 24 bytes on 64-bit architectures.
/// See `docs/architecture.md §3.1`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SmallStr(Repr);

impl SmallStr {
    /// Create a new empty `SmallStr`.
    pub const fn new() -> Self {
        Self(Repr::Inline {
            len: 0,
            buf: [0; INLINE_CAP],
        })
    }

    /// View as string slice.
    pub fn as_str(&self) -> &str {
        match &self.0 {
            Repr::Inline { len, buf } => {
                // SAFETY: We only write valid UTF-8 into buf in constructors.
                unsafe { std::str::from_utf8_unchecked(&buf[..*len as usize]) }
            }
            Repr::Heap(s) => s.as_ref(),
        }
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        match &self.0 {
            Repr::Inline { len, .. } => *len as usize,
            Repr::Heap(s) => s.len(),
        }
    }

    /// Returns true if empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns true if string is stored inline without heap allocation.
    pub fn is_inline(&self) -> bool {
        matches!(self.0, Repr::Inline { .. })
    }
}

impl Default for SmallStr {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for SmallStr {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl AsRef<str> for SmallStr {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for SmallStr {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for SmallStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for SmallStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

impl From<&str> for SmallStr {
    fn from(s: &str) -> Self {
        if s.len() <= INLINE_CAP {
            let mut buf = [0u8; INLINE_CAP];
            buf[..s.len()].copy_from_slice(s.as_bytes());
            Self(Repr::Inline {
                len: s.len() as u8,
                buf,
            })
        } else {
            Self(Repr::Heap(s.into()))
        }
    }
}

impl From<String> for SmallStr {
    fn from(s: String) -> Self {
        if s.len() <= INLINE_CAP {
            Self::from(s.as_str())
        } else {
            Self(Repr::Heap(s.into_boxed_str()))
        }
    }
}

impl From<Box<str>> for SmallStr {
    fn from(s: Box<str>) -> Self {
        if s.len() <= INLINE_CAP {
            Self::from(&*s)
        } else {
            Self(Repr::Heap(s))
        }
    }
}

impl PartialEq<str> for SmallStr {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for SmallStr {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_small_str_size() {
        assert_eq!(std::mem::size_of::<SmallStr>(), 24);
    }

    #[test]
    fn test_inline_small_str() {
        let empty = SmallStr::new();
        assert!(empty.is_inline());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.as_str(), "");

        let s22 = "1234567890123456789012";
        assert_eq!(s22.len(), 22);
        let sm = SmallStr::from(s22);
        assert!(sm.is_inline());
        assert_eq!(sm.as_str(), s22);
        assert_eq!(sm.len(), 22);
    }

    #[test]
    fn test_heap_small_str() {
        let s23 = "12345678901234567890123";
        assert_eq!(s23.len(), 23);
        let sm = SmallStr::from(s23);
        assert!(!sm.is_inline());
        assert_eq!(sm.as_str(), s23);
        assert_eq!(sm.len(), 23);
    }
}
