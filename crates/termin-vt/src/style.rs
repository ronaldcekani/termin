//! Interned style, grapheme, and hyperlink tables.
//!
//! See `docs/specs/terminal-core.md §2.2` and `§2.3`.

use rustc_hash::FxHashMap;

use crate::cell::{Cell, Style, GRAPHEME_BIT};

/// Maximum entries in `StyleTable` (u16::MAX).
pub const MAX_STYLES: usize = 65_535;

/// Interned style table.
///
/// See `docs/specs/terminal-core.md §2.2`.
#[derive(Clone, Debug)]
pub struct StyleTable {
    styles: Vec<Style>,
    lookup: FxHashMap<Style, u16>,
}

impl Default for StyleTable {
    fn default() -> Self {
        Self::new()
    }
}

impl StyleTable {
    /// Create a new style table with index 0 initialized to `Style::default()`.
    pub fn new() -> Self {
        let mut lookup = FxHashMap::default();
        let default_style = Style::default();
        lookup.insert(default_style, 0);
        Self {
            styles: vec![default_style],
            lookup,
        }
    }

    /// Retrieve style by index.
    #[inline]
    pub fn get(&self, idx: u16) -> Option<&Style> {
        self.styles.get(idx as usize)
    }

    /// Resolve style by index or return default style if out of bounds.
    #[inline]
    pub fn get_or_default(&self, idx: u16) -> Style {
        self.styles.get(idx as usize).copied().unwrap_or_default()
    }

    /// Insert or intern a style, returning its u16 index.
    pub fn insert(&mut self, style: Style) -> u16 {
        if let Some(&idx) = self.lookup.get(&style) {
            return idx;
        }

        if self.styles.len() >= MAX_STYLES {
            // Table full; return default style as fallback until compacted
            return 0;
        }

        let idx = self.styles.len() as u16;
        self.styles.push(style);
        self.lookup.insert(style, idx);
        idx
    }

    /// Number of styles currently interned.
    pub fn len(&self) -> usize {
        self.styles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }

    /// Compact the style table using a set of currently referenced style indices.
    ///
    /// Updates all cells in `cells_mut` with the new style indices.
    pub fn compact<'a>(&mut self, cells_mut: impl IntoIterator<Item = &'a mut Cell>) {
        let mut cells: Vec<&'a mut Cell> = cells_mut.into_iter().collect();
        let mut live = vec![false; self.styles.len()];
        live[0] = true; // Index 0 (default) is always live

        for cell in &cells {
            let idx = cell.style as usize;
            if idx < live.len() {
                live[idx] = true;
            }
        }

        let mut new_styles = Vec::with_capacity(self.styles.len());
        let mut new_lookup = FxHashMap::default();
        let mut remap = vec![0u16; self.styles.len()];

        for (old_idx, &is_live) in live.iter().enumerate() {
            if is_live {
                let style = self.styles[old_idx];
                let new_idx = new_styles.len() as u16;
                new_styles.push(style);
                new_lookup.insert(style, new_idx);
                remap[old_idx] = new_idx;
            }
        }

        for cell in &mut cells {
            let old_idx = cell.style as usize;
            cell.style = if old_idx < remap.len() {
                remap[old_idx]
            } else {
                0
            };
        }

        self.styles = new_styles;
        self.lookup = new_lookup;
    }
}

/// Deduplicated grapheme cluster table.
///
/// See `docs/specs/terminal-core.md §2.3`.
#[derive(Clone, Debug, Default)]
pub struct GraphemeTable {
    graphemes: Vec<Box<str>>,
    lookup: FxHashMap<Box<str>, u32>,
}

impl GraphemeTable {
    pub fn new() -> Self {
        Self {
            graphemes: Vec::new(),
            lookup: FxHashMap::default(),
        }
    }

    /// Retrieve grapheme cluster string by index.
    #[inline]
    pub fn get(&self, idx: u32) -> Option<&str> {
        self.graphemes.get(idx as usize).map(|s| &**s)
    }

    /// Insert or intern a grapheme cluster string, returning its index.
    pub fn insert(&mut self, s: &str) -> u32 {
        if let Some(&idx) = self.lookup.get(s) {
            return idx;
        }

        let idx = self.graphemes.len() as u32;
        let boxed: Box<str> = s.into();
        self.lookup.insert(boxed.clone(), idx);
        self.graphemes.push(boxed);
        idx
    }

    /// Number of graphemes stored.
    pub fn len(&self) -> usize {
        self.graphemes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.graphemes.is_empty()
    }

    /// Compact graphemes referenced by hot cells.
    pub fn compact<'a>(&mut self, cells_mut: impl IntoIterator<Item = &'a mut Cell>) {
        let mut cells: Vec<&'a mut Cell> = cells_mut.into_iter().collect();
        let mut live = vec![false; self.graphemes.len()];

        for cell in &cells {
            if let Some(idx) = cell.grapheme_index() {
                if (idx as usize) < live.len() {
                    live[idx as usize] = true;
                }
            }
        }

        let mut new_graphemes = Vec::with_capacity(self.graphemes.len());
        let mut new_lookup = FxHashMap::default();
        let mut remap = vec![0u32; self.graphemes.len()];

        for (old_idx, &is_live) in live.iter().enumerate() {
            if is_live {
                let g = self.graphemes[old_idx].clone();
                let new_idx = new_graphemes.len() as u32;
                new_lookup.insert(g.clone(), new_idx);
                new_graphemes.push(g);
                remap[old_idx] = new_idx;
            }
        }

        for cell in &mut cells {
            if let Some(old_idx) = cell.grapheme_index() {
                let new_idx = if (old_idx as usize) < remap.len() {
                    remap[old_idx as usize]
                } else {
                    0
                };
                cell.ch = GRAPHEME_BIT | new_idx;
            }
        }

        self.graphemes = new_graphemes;
        self.lookup = new_lookup;
    }
}

/// Hyperlink entry storing optional id and target URI (OSC 8).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HyperlinkEntry {
    pub id: Option<Box<str>>,
    pub uri: Box<str>,
}

/// Hyperlink table interning OSC 8 hyperlinks.
///
/// Index 0 is reserved for "no hyperlink".
#[derive(Clone, Debug)]
pub struct HyperlinkTable {
    links: Vec<HyperlinkEntry>,
    lookup: FxHashMap<(Option<Box<str>>, Box<str>), u16>,
}

impl Default for HyperlinkTable {
    fn default() -> Self {
        Self::new()
    }
}

impl HyperlinkTable {
    pub fn new() -> Self {
        // Reserve index 0 as unused / no link
        Self {
            links: vec![HyperlinkEntry {
                id: None,
                uri: Box::from(""),
            }],
            lookup: FxHashMap::default(),
        }
    }

    /// Retrieve hyperlink by 1-based index (0 returns None).
    #[inline]
    pub fn get(&self, idx: u16) -> Option<&HyperlinkEntry> {
        if idx == 0 {
            None
        } else {
            self.links.get(idx as usize)
        }
    }

    /// Insert or intern a hyperlink, returning its 1-based index (or 0 if capacity reached).
    pub fn insert(&mut self, id: Option<&str>, uri: &str) -> u16 {
        let key = (id.map(Box::from), Box::from(uri));
        if let Some(&idx) = self.lookup.get(&key) {
            return idx;
        }

        if self.links.len() >= u16::MAX as usize {
            return 0;
        }

        let idx = self.links.len() as u16;
        self.lookup.insert(key.clone(), idx);
        self.links.push(HyperlinkEntry {
            id: key.0,
            uri: key.1,
        });
        idx
    }

    pub fn len(&self) -> usize {
        self.links.len()
    }

    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::{Attrs, Color};

    #[test]
    fn test_style_table_interning_and_compaction() {
        let mut table = StyleTable::new();
        assert_eq!(table.len(), 1); // default style at 0

        let s1 = Style {
            fg: Color::Indexed(1),
            bg: Color::Default,
            ul_color: Color::Default,
            attrs: Attrs::BOLD,
            link: 0,
        };
        let idx1 = table.insert(s1);
        assert_eq!(idx1, 1);
        assert_eq!(table.insert(s1), 1); // deduplicated

        let s2 = Style {
            fg: Color::Indexed(2),
            bg: Color::Default,
            ul_color: Color::Default,
            attrs: Attrs::ITALIC,
            link: 0,
        };
        let idx2 = table.insert(s2);
        assert_eq!(idx2, 2);

        // Compact with only s2 referenced
        let mut cell = Cell {
            ch: 'a' as u32,
            style: idx2,
            flags: Default::default(),
        };
        table.compact([&mut cell]);

        assert_eq!(cell.style, 1);
        assert_eq!(table.get(1), Some(&s2));
    }

    #[test]
    fn test_grapheme_table() {
        let mut table = GraphemeTable::new();
        let idx = table.insert("👨‍👩‍👧‍👦");
        assert_eq!(table.insert("👨‍👩‍👧‍👦"), idx);
        assert_eq!(table.get(idx), Some("👨‍👩‍👧‍👦"));
    }

    #[test]
    fn test_hyperlink_table() {
        let mut table = HyperlinkTable::new();
        assert_eq!(table.get(0), None);
        let link_id = table.insert(Some("id1"), "https://example.com");
        assert!(link_id > 0);
        let entry = table.get(link_id).unwrap();
        assert_eq!(entry.uri.as_ref(), "https://example.com");
        assert_eq!(entry.id.as_deref(), Some("id1"));
    }
}
