//! Shelf atlas packer for glyph textures.
//!
//! Organizes glyphs into horizontal shelves by height buckets with 1 px padding.
//! Automatically doubles dimensions on demand up to a maximum size (default 8192).
//!
//! See `docs/specs/text-and-rendering.md §2.7`.

use termin_core::Rect;

/// A rectangle in the atlas in physical pixels.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct AtlasRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl AtlasRect {
    /// Check whether two atlas rects overlap (inclusive bounds).
    #[inline]
    pub fn intersects(&self, other: &AtlasRect) -> bool {
        if self.w == 0 || self.h == 0 || other.w == 0 || other.h == 0 {
            return false;
        }
        self.x < other.x + other.w
            && self.x + self.w > other.x
            && self.y < other.y + other.h
            && self.y + self.h > other.y
    }
}

impl From<AtlasRect> for Rect {
    fn from(r: AtlasRect) -> Self {
        Rect {
            x: r.x as f32,
            y: r.y as f32,
            w: r.w as f32,
            h: r.h as f32,
        }
    }
}

/// A horizontal shelf in the atlas.
#[derive(Clone, Debug)]
struct Shelf {
    y: u32,
    height: u32,
    cursor_x: u32,
}

/// Shelf atlas packer.
#[derive(Clone, Debug)]
pub struct ShelfAtlas {
    width: u32,
    height: u32,
    max_size: u32,
    padding: u32,
    shelves: Vec<Shelf>,
    did_grow: bool,
}

impl ShelfAtlas {
    /// Create a new shelf atlas packer with initial dimensions and maximum limit.
    pub fn new(initial_width: u32, initial_height: u32, max_size: u32) -> Self {
        Self {
            width: initial_width.min(max_size).max(64),
            height: initial_height.min(max_size).max(64),
            max_size,
            padding: 1,
            shelves: Vec::new(),
            did_grow: false,
        }
    }

    /// Default configuration starting at 512x512 up to 8192x8192.
    pub fn default_atlas() -> Self {
        Self::new(512, 512, 8192)
    }

    /// Current atlas width in pixels.
    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Current atlas height in pixels.
    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns true if the atlas grew in dimensions since last reset.
    #[inline]
    pub fn did_grow(&self) -> bool {
        self.did_grow
    }

    /// Resets the growth flag after GPU texture reallocation/blit.
    #[inline]
    pub fn reset_grow(&mut self) {
        self.did_grow = false;
    }

    /// Clear all shelf allocations (e.g. after eviction or font size change).
    pub fn clear(&mut self) {
        self.shelves.clear();
        self.did_grow = false;
    }

    /// Attempt to allocate a slot for a glyph of `(w, h)` pixels.
    ///
    /// If full, doubles atlas dimensions up to `max_size`. Returns `None` if
    /// the glyph cannot fit even after expanding to `max_size`.
    pub fn allocate(&mut self, w: u32, h: u32) -> Option<AtlasRect> {
        if w == 0 || h == 0 {
            return Some(AtlasRect {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            });
        }

        let pad = self.padding;
        // Total space needed in shelf
        let req_w = w + pad;
        let req_h = h + pad;

        loop {
            // Find existing shelf with best vertical fit (smallest height >= h that has room)
            let mut best_shelf_idx: Option<usize> = None;
            let mut best_height_diff = u32::MAX;

            for (idx, shelf) in self.shelves.iter().enumerate() {
                if shelf.height >= h && shelf.cursor_x + req_w + pad <= self.width {
                    let diff = shelf.height - h;
                    // Prefer exact or close height match; within 8 px difference is good
                    if diff < best_height_diff {
                        best_height_diff = diff;
                        best_shelf_idx = Some(idx);
                        if diff == 0 {
                            break;
                        }
                    }
                }
            }

            if let Some(idx) = best_shelf_idx {
                let shelf = &mut self.shelves[idx];
                let x = shelf.cursor_x + pad;
                let y = shelf.y + pad;
                shelf.cursor_x += req_w;
                return Some(AtlasRect { x, y, w, h });
            }

            // Can we create a new shelf?
            let next_y = self.shelves.last().map_or(0, |s| s.y + s.height + pad);
            if next_y + req_h + pad <= self.height && req_w + pad <= self.width {
                // Round shelf height up to nearest multiple of 4 to group similar heights
                let shelf_h = h.div_ceil(4) * 4;
                let shelf = Shelf {
                    y: next_y,
                    height: shelf_h,
                    cursor_x: pad + req_w,
                };
                let rect = AtlasRect {
                    x: pad + pad,
                    y: next_y + pad,
                    w,
                    h,
                };
                self.shelves.push(shelf);
                return Some(rect);
            }

            // Atlas is full at current size, try growing by doubling
            if self.width < self.max_size || self.height < self.max_size {
                let new_w = (self.width * 2).min(self.max_size);
                let new_h = (self.height * 2).min(self.max_size);
                if new_w == self.width && new_h == self.height {
                    return None;
                }
                self.width = new_w;
                self.height = new_h;
                self.did_grow = true;
                // Re-try allocation with new dimensions
                continue;
            }

            // Cannot grow further and cannot fit
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shelf_packer_no_overlap() {
        let mut atlas = ShelfAtlas::new(128, 128, 1024);
        let mut allocated = Vec::new();

        // Allocate 150 rects of various sizes
        let sizes = [
            (8, 12),
            (10, 16),
            (12, 14),
            (16, 20),
            (7, 10),
            (24, 24),
            (9, 15),
            (14, 18),
        ];

        for i in 0..150 {
            let (w, h) = sizes[i % sizes.len()];
            let rect = atlas.allocate(w, h).expect("allocation should succeed");
            assert_eq!(rect.w, w);
            assert_eq!(rect.h, h);
            assert!(rect.x + rect.w <= atlas.width());
            assert!(rect.y + rect.h <= atlas.height());

            // Verify no overlap with any previously allocated rect
            for prev in &allocated {
                assert!(
                    !rect.intersects(prev),
                    "Rect {:?} overlaps with {:?}",
                    rect,
                    prev
                );
            }
            allocated.push(rect);
        }

        // Verify atlas grew beyond 128x128
        assert!(atlas.width() > 128 || atlas.height() > 128);
    }

    #[test]
    fn test_shelf_packer_growth() {
        let mut atlas = ShelfAtlas::new(64, 64, 512);
        assert_eq!(atlas.width(), 64);
        assert_eq!(atlas.height(), 64);

        // Allocate a rect larger than 64x64
        let rect = atlas.allocate(80, 80).expect("should grow to fit");
        assert!(atlas.did_grow());
        assert!(atlas.width() >= 128);
        assert!(atlas.height() >= 128);
        assert_eq!(rect.w, 80);
        assert_eq!(rect.h, 80);
        assert!(rect.x + rect.w <= atlas.width());
        assert!(rect.y + rect.h <= atlas.height());
    }

    #[test]
    fn test_shelf_packer_clear() {
        let mut atlas = ShelfAtlas::new(64, 64, 256);
        let _ = atlas.allocate(30, 30);
        atlas.clear();
        assert!(!atlas.did_grow());
        let rect = atlas.allocate(30, 30).expect("should allocate after clear");
        assert!(rect.x <= 2);
        assert!(rect.y <= 2);
    }

    #[test]
    fn test_shelf_packer_zero_size() {
        let mut atlas = ShelfAtlas::new(64, 64, 256);
        let rect = atlas.allocate(0, 0).expect("zero size succeeds");
        assert_eq!(rect.w, 0);
        assert_eq!(rect.h, 0);
    }
}
