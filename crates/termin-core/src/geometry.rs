//! Geometry primitives in logical points (f32).
//!
//! Physical = logical * scale_factor.
//! See `docs/architecture.md §3.1`.

/// Point in 2D space (logical points).
///
/// See `docs/architecture.md §3.1`.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    /// Create a new point.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Scale point by a factor.
    pub fn scale(self, factor: f32) -> Self {
        Self {
            x: self.x * factor,
            y: self.y * factor,
        }
    }
}

/// Size in 2D space (logical points).
///
/// See `docs/architecture.md §3.1`.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl Size {
    /// Create a new size.
    pub const fn new(w: f32, h: f32) -> Self {
        Self { w, h }
    }

    /// Area of the size (w * h).
    pub fn area(self) -> f32 {
        self.w * self.h
    }

    /// Scale size by a factor.
    pub fn scale(self, factor: f32) -> Self {
        Self {
            w: self.w * factor,
            h: self.h * factor,
        }
    }
}

/// Rectangle in 2D space (logical points).
///
/// See `docs/architecture.md §3.1`.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    /// Create a new rectangle.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Create a rectangle from origin point and size.
    pub const fn from_origin_size(origin: Point, size: Size) -> Self {
        Self {
            x: origin.x,
            y: origin.y,
            w: size.w,
            h: size.h,
        }
    }

    /// Left coordinate (x).
    pub const fn left(self) -> f32 {
        self.x
    }

    /// Right coordinate (x + w).
    pub fn right(self) -> f32 {
        self.x + self.w
    }

    /// Top coordinate (y).
    pub const fn top(self) -> f32 {
        self.y
    }

    /// Bottom coordinate (y + h).
    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    /// Origin of rectangle.
    pub const fn origin(self) -> Point {
        Point {
            x: self.x,
            y: self.y,
        }
    }

    /// Size of rectangle.
    pub const fn size(self) -> Size {
        Size {
            w: self.w,
            h: self.h,
        }
    }

    /// Check if point is inside this rectangle.
    pub fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }

    /// Check if another rectangle intersects with this one.
    pub fn intersects(self, other: Rect) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }

    /// Compute intersection with another rectangle.
    pub fn intersection(self, other: Rect) -> Option<Rect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self.right().min(other.right());
        let y2 = self.bottom().min(other.bottom());

        if x2 > x1 && y2 > y1 {
            Some(Rect {
                x: x1,
                y: y1,
                w: x2 - x1,
                h: y2 - y1,
            })
        } else {
            None
        }
    }

    /// Inset rectangle by horizontal and vertical amounts.
    pub fn inset(self, dx: f32, dy: f32) -> Rect {
        Rect {
            x: self.x + dx,
            y: self.y + dy,
            w: (self.w - 2.0 * dx).max(0.0),
            h: (self.h - 2.0 * dy).max(0.0),
        }
    }

    /// Scale rect by a factor.
    pub fn scale(self, factor: f32) -> Self {
        Self {
            x: self.x * factor,
            y: self.y * factor,
            w: self.w * factor,
            h: self.h * factor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_point_and_size() {
        let p = Point::new(10.0, 20.0).scale(2.0);
        assert_eq!(p.x, 20.0);
        assert_eq!(p.y, 40.0);

        let s = Size::new(50.0, 30.0);
        assert_eq!(s.area(), 1500.0);
    }

    #[test]
    fn test_rect_contains_and_intersection() {
        let r = Rect::new(10.0, 10.0, 100.0, 100.0);
        assert!(r.contains(Point::new(50.0, 50.0)));
        assert!(!r.contains(Point::new(5.0, 50.0)));

        let r2 = Rect::new(60.0, 60.0, 100.0, 100.0);
        assert!(r.intersects(r2));

        let inter = r.intersection(r2).unwrap();
        assert_eq!(inter, Rect::new(60.0, 60.0, 50.0, 50.0));
    }
}
