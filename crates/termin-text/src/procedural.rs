//! Pixel-perfect CPU rasterizer for procedural terminal glyphs.
//!
//! Generates box drawing (U+2500–U+257F), block elements (U+2580–U+259F),
//! braille patterns (U+2800–U+28FF), powerline triangles (U+E0B0–U+E0B3),
//! and legacy computing sextants (U+1FB00–U+1FB3B).
//!
//! See `docs/specs/text-and-rendering.md §2.6`.

/// Check if a character should be rasterized procedurally rather than loaded from font.
pub fn is_procedural(c: char) -> bool {
    matches!(
        c,
        '\u{2500}'..='\u{257F}' // Box drawing
        | '\u{2580}'..='\u{259F}' // Block elements
        | '\u{2800}'..='\u{28FF}' // Braille patterns
        | '\u{E0B0}'..='\u{E0B3}' // Powerline triangles
        | '\u{1FB00}'..='\u{1FB3B}' // Legacy computing sextants
    )
}

/// Rasterize a procedural character into an A8 coverage buffer of `width * height` bytes.
///
/// Returns `true` if the character was recognized and rendered, `false` otherwise.
pub fn rasterize_procedural(c: char, width: u32, height: u32, out: &mut [u8]) -> bool {
    let len = (width * height) as usize;
    if width == 0 || height == 0 || out.len() < len {
        return false;
    }

    // Clear output buffer to 0 (transparent)
    out[..len].fill(0);

    match c {
        '\u{2500}'..='\u{257F}' => render_box_drawing(c, width, height, out),
        '\u{2580}'..='\u{259F}' => render_block_element(c, width, height, out),
        '\u{2800}'..='\u{28FF}' => render_braille(c, width, height, out),
        '\u{E0B0}'..='\u{E0B3}' => render_powerline(c, width, height, out),
        '\u{1FB00}'..='\u{1FB3B}' => render_sextant(c, width, height, out),
        _ => false,
    }
}

#[inline]
fn set_pixel(out: &mut [u8], width: u32, height: u32, x: u32, y: u32, val: u8) {
    if x < width && y < height {
        let idx = (y * width + x) as usize;
        out[idx] = out[idx].max(val);
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_rect(out: &mut [u8], width: u32, height: u32, x0: u32, y0: u32, x1: u32, y1: u32, val: u8) {
    let x_start = x0.min(width);
    let x_end = x1.min(width);
    let y_start = y0.min(height);
    let y_end = y1.min(height);

    for y in y_start..y_end {
        let row_start = (y * width) as usize;
        for x in x_start..x_end {
            let idx = row_start + x as usize;
            out[idx] = out[idx].max(val);
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Branch {
    None,
    Light,
    Heavy,
    Double,
    Dashed2Light,
    Dashed2Heavy,
    Dashed3Light,
    Dashed3Heavy,
    Dashed4Light,
    Dashed4Heavy,
}

fn render_box_drawing(c: char, width: u32, height: u32, out: &mut [u8]) -> bool {
    let cp = c as u32;

    // Special cases: rounded corners and diagonals
    match cp {
        0x256D => return render_arc_corner(out, width, height, ArcCorner::DownRight),
        0x256E => return render_arc_corner(out, width, height, ArcCorner::DownLeft),
        0x256F => return render_arc_corner(out, width, height, ArcCorner::UpLeft),
        0x2570 => return render_arc_corner(out, width, height, ArcCorner::UpRight),
        0x2571 => return render_diagonal(out, width, height, false, true),
        0x2572 => return render_diagonal(out, width, height, true, false),
        0x2573 => return render_diagonal(out, width, height, true, true),
        _ => {}
    }

    let (up, down, left, right) = decode_box_branches(cp);
    if up == Branch::None && down == Branch::None && left == Branch::None && right == Branch::None {
        return false;
    }

    let t_light = (height / 14).max(1);
    let t_heavy = (t_light * 2).max(t_light + 1);

    let cx = width / 2;
    let cy = height / 2;

    // Helper for line coordinates
    let get_span = |center: u32, thick: u32, limit: u32| -> (u32, u32) {
        let half = thick / 2;
        let start = center.saturating_sub(half);
        let end = (start + thick).min(limit);
        (start, end)
    };

    let (lx0, lx1) = get_span(cx, t_light, width);
    let (hx0, hx1) = get_span(cx, t_heavy, width);
    let (ly0, ly1) = get_span(cy, t_light, height);
    let (hy0, hy1) = get_span(cy, t_heavy, height);

    let d_spacing = (t_light + 2).max(2);
    let d_thick = t_light;
    let d_top = cy.saturating_sub(d_spacing / 2 + d_thick);
    let d_bot = cy + (d_spacing / 2);
    let d_left = cx.saturating_sub(d_spacing / 2 + d_thick);
    let d_right = cx + (d_spacing / 2);

    // Draw West (left)
    match left {
        Branch::Light => fill_rect(out, width, height, 0, ly0, cx + 1, ly1, 255),
        Branch::Heavy => fill_rect(out, width, height, 0, hy0, cx + 1, hy1, 255),
        Branch::Double => {
            fill_rect(out, width, height, 0, d_top, cx + 1, d_top + d_thick, 255);
            fill_rect(out, width, height, 0, d_bot, cx + 1, d_bot + d_thick, 255);
        }
        Branch::Dashed2Light | Branch::Dashed3Light | Branch::Dashed4Light => {
            render_dashed_h(out, width, height, 0, cx, ly0, ly1, left)
        }
        Branch::Dashed2Heavy | Branch::Dashed3Heavy | Branch::Dashed4Heavy => {
            render_dashed_h(out, width, height, 0, cx, hy0, hy1, left)
        }
        Branch::None => {}
    }

    // Draw East (right)
    match right {
        Branch::Light => fill_rect(out, width, height, cx, ly0, width, ly1, 255),
        Branch::Heavy => fill_rect(out, width, height, cx, hy0, width, hy1, 255),
        Branch::Double => {
            fill_rect(out, width, height, cx, d_top, width, d_top + d_thick, 255);
            fill_rect(out, width, height, cx, d_bot, width, d_bot + d_thick, 255);
        }
        Branch::Dashed2Light | Branch::Dashed3Light | Branch::Dashed4Light => {
            render_dashed_h(out, width, height, cx, width, ly0, ly1, right)
        }
        Branch::Dashed2Heavy | Branch::Dashed3Heavy | Branch::Dashed4Heavy => {
            render_dashed_h(out, width, height, cx, width, hy0, hy1, right)
        }
        Branch::None => {}
    }

    // Draw North (up)
    match up {
        Branch::Light => fill_rect(out, width, height, lx0, 0, lx1, cy + 1, 255),
        Branch::Heavy => fill_rect(out, width, height, hx0, 0, hx1, cy + 1, 255),
        Branch::Double => {
            fill_rect(out, width, height, d_left, 0, d_left + d_thick, cy + 1, 255);
            fill_rect(out, width, height, d_right, 0, d_right + d_thick, cy + 1, 255);
        }
        Branch::Dashed2Light | Branch::Dashed3Light | Branch::Dashed4Light => {
            render_dashed_v(out, width, height, 0, cy, lx0, lx1, up)
        }
        Branch::Dashed2Heavy | Branch::Dashed3Heavy | Branch::Dashed4Heavy => {
            render_dashed_v(out, width, height, 0, cy, hx0, hx1, up)
        }
        Branch::None => {}
    }

    // Draw South (down)
    match down {
        Branch::Light => fill_rect(out, width, height, lx0, cy, lx1, height, 255),
        Branch::Heavy => fill_rect(out, width, height, hx0, cy, hx1, height, 255),
        Branch::Double => {
            fill_rect(out, width, height, d_left, cy, d_left + d_thick, height, 255);
            fill_rect(out, width, height, d_right, cy, d_right + d_thick, height, 255);
        }
        Branch::Dashed2Light | Branch::Dashed3Light | Branch::Dashed4Light => {
            render_dashed_v(out, width, height, cy, height, lx0, lx1, down)
        }
        Branch::Dashed2Heavy | Branch::Dashed3Heavy | Branch::Dashed4Heavy => {
            render_dashed_v(out, width, height, cy, height, hx0, hx1, down)
        }
        Branch::None => {}
    }

    // Center junction fills if branches meet
    let has_h = left != Branch::None || right != Branch::None;
    let has_v = up != Branch::None || down != Branch::None;
    if has_h && has_v {
        let x0 = if left == Branch::Heavy || right == Branch::Heavy {
            hx0
        } else {
            lx0
        };
        let x1 = if left == Branch::Heavy || right == Branch::Heavy {
            hx1
        } else {
            lx1
        };
        let y0 = if up == Branch::Heavy || down == Branch::Heavy {
            hy0
        } else {
            ly0
        };
        let y1 = if up == Branch::Heavy || down == Branch::Heavy {
            hy1
        } else {
            ly1
        };
        fill_rect(out, width, height, x0, y0, x1, y1, 255);
    }

    true
}

enum ArcCorner {
    DownRight, // ╭
    DownLeft,  // ╮
    UpLeft,    // ╯
    UpRight,   // ╰
}

fn render_arc_corner(out: &mut [u8], width: u32, height: u32, corner: ArcCorner) -> bool {
    let t = (height / 14).max(1) as f32;
    let cx = width / 2;
    let cy = height / 2;
    let r = (cx.min(cy) as f32).max(2.0);

    // Center of circle depending on corner
    let (center_x, center_y) = match corner {
        ArcCorner::DownRight => (cx as f32 + r, cy as f32 + r),
        ArcCorner::DownLeft => (cx as f32 - r, cy as f32 + r),
        ArcCorner::UpLeft => (cx as f32 - r, cy as f32 - r),
        ArcCorner::UpRight => (cx as f32 + r, cy as f32 - r),
    };

    let r_inner = (r - t / 2.0).max(0.0);
    let r_outer = r + t / 2.0;

    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 - center_x;
            let dy = y as f32 - center_y;
            let dist = (dx * dx + dy * dy).sqrt();

            let in_quadrant = match corner {
                ArcCorner::DownRight => dx <= 0.0 && dy <= 0.0,
                ArcCorner::DownLeft => dx >= 0.0 && dy <= 0.0,
                ArcCorner::UpLeft => dx >= 0.0 && dy >= 0.0,
                ArcCorner::UpRight => dx <= 0.0 && dy >= 0.0,
            };

            if in_quadrant && dist >= r_inner && dist <= r_outer {
                set_pixel(out, width, height, x, y, 255);
            }
        }
    }

    // Connect arc ends straight to cell boundaries
    let (start_y, end_y) = (cy, cy + t as u32);
    let (start_x, end_x) = (cx, cx + t as u32);
    match corner {
        ArcCorner::DownRight => {
            fill_rect(out, width, height, cx, start_y, width, end_y, 255);
            fill_rect(out, width, height, start_x, cy, end_x, height, 255);
        }
        ArcCorner::DownLeft => {
            fill_rect(out, width, height, 0, start_y, cx + 1, end_y, 255);
            fill_rect(out, width, height, start_x, cy, end_x, height, 255);
        }
        ArcCorner::UpLeft => {
            fill_rect(out, width, height, 0, start_y, cx + 1, end_y, 255);
            fill_rect(out, width, height, start_x, 0, end_x, cy + 1, 255);
        }
        ArcCorner::UpRight => {
            fill_rect(out, width, height, cx, start_y, width, end_y, 255);
            fill_rect(out, width, height, start_x, 0, end_x, cy + 1, 255);
        }
    }

    true
}

fn render_diagonal(out: &mut [u8], width: u32, height: u32, top_left_to_bottom_right: bool, bottom_left_to_top_right: bool) -> bool {
    let t = (height / 14).max(1);
    let half_t = t / 2;

    for y in 0..height {
        let fy = y as f32 / height as f32;

        if top_left_to_bottom_right {
            let target_x = (fy * width as f32).round() as u32;
            let x0 = target_x.saturating_sub(half_t);
            let x1 = (x0 + t).min(width);
            for x in x0..x1 {
                set_pixel(out, width, height, x, y, 255);
            }
        }

        if bottom_left_to_top_right {
            let target_x = ((1.0 - fy) * width as f32).round() as u32;
            let x0 = target_x.saturating_sub(half_t);
            let x1 = (x0 + t).min(width);
            for x in x0..x1 {
                set_pixel(out, width, height, x, y, 255);
            }
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn render_dashed_h(out: &mut [u8], width: u32, height: u32, x0: u32, x1: u32, y0: u32, y1: u32, branch: Branch) {
    let count = match branch {
        Branch::Dashed2Light | Branch::Dashed2Heavy => 2,
        Branch::Dashed3Light | Branch::Dashed3Heavy => 3,
        _ => 4,
    };
    let total_w = x1.saturating_sub(x0);
    if total_w == 0 {
        return;
    }
    let dash_w = (total_w / (count * 2)).max(1);
    for i in 0..count {
        let start = x0 + i * dash_w * 2;
        let end = (start + dash_w).min(x1);
        fill_rect(out, width, height, start, y0, end, y1, 255);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_dashed_v(out: &mut [u8], width: u32, height: u32, y0: u32, y1: u32, x0: u32, x1: u32, branch: Branch) {
    let count = match branch {
        Branch::Dashed2Light | Branch::Dashed2Heavy => 2,
        Branch::Dashed3Light | Branch::Dashed3Heavy => 3,
        _ => 4,
    };
    let total_h = y1.saturating_sub(y0);
    if total_h == 0 {
        return;
    }
    let dash_h = (total_h / (count * 2)).max(1);
    for i in 0..count {
        let start = y0 + i * dash_h * 2;
        let end = (start + dash_h).min(y1);
        fill_rect(out, width, height, x0, start, x1, end, 255);
    }
}

fn decode_box_branches(cp: u32) -> (Branch, Branch, Branch, Branch) {
    use Branch::*;
    match cp {
        0x2500 => (None, None, Light, Light),
        0x2501 => (None, None, Heavy, Heavy),
        0x2502 => (Light, Light, None, None),
        0x2503 => (Heavy, Heavy, None, None),
        0x2504 => (None, None, Dashed3Light, Dashed3Light),
        0x2505 => (None, None, Dashed3Heavy, Dashed3Heavy),
        0x2506 => (Dashed3Light, Dashed3Light, None, None),
        0x2507 => (Dashed3Heavy, Dashed3Heavy, None, None),
        0x2508 => (None, None, Dashed4Light, Dashed4Light),
        0x2509 => (None, None, Dashed4Heavy, Dashed4Heavy),
        0x250A => (Dashed4Light, Dashed4Light, None, None),
        0x250B => (Dashed4Heavy, Dashed4Heavy, None, None),
        0x250C => (None, Light, None, Light),
        0x250D => (None, Light, None, Heavy),
        0x250E => (None, Heavy, None, Light),
        0x250F => (None, Heavy, None, Heavy),
        0x2510 => (None, Light, Light, None),
        0x2511 => (None, Light, Heavy, None),
        0x2512 => (None, Heavy, Light, None),
        0x2513 => (None, Heavy, Heavy, None),
        0x2514 => (Light, None, None, Light),
        0x2515 => (Light, None, None, Heavy),
        0x2516 => (Heavy, None, None, Light),
        0x2517 => (Heavy, None, None, Heavy),
        0x2518 => (Light, None, Light, None),
        0x2519 => (Light, None, Heavy, None),
        0x251A => (Heavy, None, Light, None),
        0x251B => (Heavy, None, Heavy, None),
        0x251C => (Light, Light, None, Light),
        0x251D => (Light, Light, None, Heavy),
        0x251E => (Heavy, Light, None, Light),
        0x251F => (Light, Heavy, None, Light),
        0x2520 => (Heavy, Heavy, None, Light),
        0x2521 => (Heavy, Light, None, Heavy),
        0x2522 => (Light, Heavy, None, Heavy),
        0x2523 => (Heavy, Heavy, None, Heavy),
        0x2524 => (Light, Light, Light, None),
        0x2525 => (Light, Light, Heavy, None),
        0x2526 => (Heavy, Light, Light, None),
        0x2527 => (Light, Heavy, Light, None),
        0x2528 => (Heavy, Heavy, Light, None),
        0x2529 => (Heavy, Light, Heavy, None),
        0x252A => (Light, Heavy, Heavy, None),
        0x252B => (Heavy, Heavy, Heavy, None),
        0x252C => (None, Light, Light, Light),
        0x252D => (None, Light, Heavy, Light),
        0x252E => (None, Light, Light, Heavy),
        0x252F => (None, Light, Heavy, Heavy),
        0x2530 => (None, Heavy, Light, Light),
        0x2531 => (None, Heavy, Heavy, Light),
        0x2532 => (None, Heavy, Light, Heavy),
        0x2533 => (None, Heavy, Heavy, Heavy),
        0x2534 => (Light, None, Light, Light),
        0x2535 => (Light, None, Heavy, Light),
        0x2536 => (Light, None, Light, Heavy),
        0x2537 => (Light, None, Heavy, Heavy),
        0x2538 => (Heavy, None, Light, Light),
        0x2539 => (Heavy, None, Heavy, Light),
        0x253A => (Heavy, None, Light, Heavy),
        0x253B => (Heavy, None, Heavy, Heavy),
        0x253C => (Light, Light, Light, Light),
        0x253D => (Light, Light, Heavy, Light),
        0x253E => (Light, Light, Light, Heavy),
        0x253F => (Light, Light, Heavy, Heavy),
        0x2540 => (Heavy, Light, Light, Light),
        0x2541 => (Light, Heavy, Light, Light),
        0x2542 => (Heavy, Heavy, Light, Light),
        0x2543 => (Heavy, Light, Heavy, Light),
        0x2544 => (Heavy, Light, Light, Heavy),
        0x2545 => (Light, Heavy, Heavy, Light),
        0x2546 => (Light, Heavy, Light, Heavy),
        0x2547 => (Heavy, Light, Heavy, Heavy),
        0x2548 => (Light, Heavy, Heavy, Heavy),
        0x2549 => (Heavy, Heavy, Heavy, Light),
        0x254A => (Heavy, Heavy, Light, Heavy),
        0x254B => (Heavy, Heavy, Heavy, Heavy),
        0x254C => (None, None, Dashed2Light, Dashed2Light),
        0x254D => (None, None, Dashed2Heavy, Dashed2Heavy),
        0x254E => (Dashed2Light, Dashed2Light, None, None),
        0x254F => (Dashed2Heavy, Dashed2Heavy, None, None),
        0x2550 => (None, None, Double, Double),
        0x2551 => (Double, Double, None, None),
        0x2552 => (None, Light, None, Double),
        0x2553 => (None, Double, None, Light),
        0x2554 => (None, Double, None, Double),
        0x2555 => (None, Light, Double, None),
        0x2556 => (None, Double, Light, None),
        0x2557 => (None, Double, Double, None),
        0x2558 => (Light, None, None, Double),
        0x2559 => (Double, None, None, Light),
        0x255A => (Double, None, None, Double),
        0x255B => (Light, None, Double, None),
        0x255C => (Double, None, Light, None),
        0x255D => (Double, None, Double, None),
        0x255E => (Light, Light, None, Double),
        0x255F => (Double, Double, None, Light),
        0x2560 => (Double, Double, None, Double),
        0x2561 => (Light, Light, Double, None),
        0x2562 => (Double, Double, Light, None),
        0x2563 => (Double, Double, Double, None),
        0x2564 => (None, Light, Double, Double),
        0x2565 => (None, Double, Light, Light),
        0x2566 => (None, Double, Double, Double),
        0x2567 => (Light, None, Double, Double),
        0x2568 => (Double, None, Light, Light),
        0x2569 => (Double, None, Double, Double),
        0x256A => (Light, Light, Double, Double),
        0x256B => (Double, Double, Light, Light),
        0x256C => (Double, Double, Double, Double),
        0x2574 => (None, None, Light, None),
        0x2575 => (Light, None, None, None),
        0x2576 => (None, None, None, Light),
        0x2577 => (None, Light, None, None),
        0x2578 => (None, None, Heavy, None),
        0x2579 => (Heavy, None, None, None),
        0x257A => (None, None, None, Heavy),
        0x257B => (None, Heavy, None, None),
        0x257C => (None, None, Light, Heavy),
        0x257D => (Light, Heavy, None, None),
        0x257E => (None, None, Heavy, Light),
        0x257F => (Heavy, Light, None, None),
        _ => (None, None, None, None),
    }
}

fn render_block_element(c: char, width: u32, height: u32, out: &mut [u8]) -> bool {
    let cp = c as u32;
    match cp {
        0x2580 => {
            // Upper half block
            fill_rect(out, width, height, 0, 0, width, height / 2, 255);
            true
        }
        0x2581..=0x2587 => {
            // Lower 1/8 to 7/8
            let n = cp - 0x2580;
            let h_fill = ((height * n + 4) / 8).min(height);
            fill_rect(out, width, height, 0, height - h_fill, width, height, 255);
            true
        }
        0x2588 => {
            // Full block
            fill_rect(out, width, height, 0, 0, width, height, 255);
            true
        }
        0x2589..=0x258F => {
            // Left 7/8 to 1/8
            let n = 8 - (cp - 0x2588);
            let w_fill = ((width * n + 4) / 8).min(width);
            fill_rect(out, width, height, 0, 0, w_fill, height, 255);
            true
        }
        0x2590 => {
            // Right half block
            fill_rect(out, width, height, width / 2, 0, width, height, 255);
            true
        }
        0x2591 => {
            // Light shade 25% (ordered dither 1 in 4 pixels)
            for y in 0..height {
                for x in 0..width {
                    if (x + 2 * y) % 4 == 0 {
                        set_pixel(out, width, height, x, y, 255);
                    }
                }
            }
            true
        }
        0x2592 => {
            // Medium shade 50% (checkerboard)
            for y in 0..height {
                for x in 0..width {
                    if (x + y) % 2 == 0 {
                        set_pixel(out, width, height, x, y, 255);
                    }
                }
            }
            true
        }
        0x2593 => {
            // Dark shade 75%
            for y in 0..height {
                for x in 0..width {
                    if (x + 2 * y) % 4 != 0 {
                        set_pixel(out, width, height, x, y, 255);
                    }
                }
            }
            true
        }
        0x2594 => {
            // Upper 1/8
            let h_fill = ((height + 4) / 8).min(height);
            fill_rect(out, width, height, 0, 0, width, h_fill, 255);
            true
        }
        0x2595 => {
            // Right 1/8
            let w_fill = ((width + 4) / 8).min(width);
            fill_rect(out, width, height, width - w_fill, 0, width, height, 255);
            true
        }
        0x2596..=0x259F => {
            // Quadrants
            let (tl, tr, bl, br) = match cp {
                0x2596 => (false, false, true, false),
                0x2597 => (false, false, false, true),
                0x2598 => (true, false, false, false),
                0x2599 => (true, false, true, true),
                0x259A => (true, false, false, true),
                0x259B => (true, true, true, false),
                0x259C => (true, true, false, true),
                0x259D => (false, true, false, false),
                0x259E => (false, true, true, false),
                0x259F => (false, true, true, true),
                _ => (false, false, false, false),
            };
            let mid_x = width / 2;
            let mid_y = height / 2;
            if tl {
                fill_rect(out, width, height, 0, 0, mid_x, mid_y, 255);
            }
            if tr {
                fill_rect(out, width, height, mid_x, 0, width, mid_y, 255);
            }
            if bl {
                fill_rect(out, width, height, 0, mid_y, mid_x, height, 255);
            }
            if br {
                fill_rect(out, width, height, mid_x, mid_y, width, height, 255);
            }
            true
        }
        _ => false,
    }
}

fn render_braille(c: char, width: u32, height: u32, out: &mut [u8]) -> bool {
    let pattern = (c as u32) - 0x2800;
    if pattern > 0xFF {
        return false;
    }

    let col_x = [(width * 3) / 10, (width * 7) / 10];
    let row_y = [
        (height * 3) / 24,
        (height * 9) / 24,
        (height * 15) / 24,
        (height * 21) / 24,
    ];

    let radius = (width.min(height) / 8).max(1) as i32;
    let r2 = radius * radius;

    // 8 dots:
    // bit 0: Dot 1 (col 0, row 0)
    // bit 1: Dot 2 (col 0, row 1)
    // bit 2: Dot 3 (col 0, row 2)
    // bit 3: Dot 4 (col 1, row 0)
    // bit 4: Dot 5 (col 1, row 1)
    // bit 5: Dot 6 (col 1, row 2)
    // bit 6: Dot 7 (col 0, row 3)
    // bit 7: Dot 8 (col 1, row 3)
    let dots = [
        (0, 0, pattern & 0x01 != 0),
        (0, 1, pattern & 0x02 != 0),
        (0, 2, pattern & 0x04 != 0),
        (1, 0, pattern & 0x08 != 0),
        (1, 1, pattern & 0x10 != 0),
        (1, 2, pattern & 0x20 != 0),
        (0, 3, pattern & 0x40 != 0),
        (1, 3, pattern & 0x80 != 0),
    ];

    for (col, row, active) in dots {
        if !active {
            continue;
        }
        let cx = col_x[col] as i32;
        let cy = row_y[row] as i32;

        let y_min = (cy - radius).max(0) as u32;
        let y_max = (cy + radius + 1).min(height as i32) as u32;
        let x_min = (cx - radius).max(0) as u32;
        let x_max = (cx + radius + 1).min(width as i32) as u32;

        for y in y_min..y_max {
            for x in x_min..x_max {
                let dx = x as i32 - cx;
                let dy = y as i32 - cy;
                if dx * dx + dy * dy <= r2 {
                    set_pixel(out, width, height, x, y, 255);
                }
            }
        }
    }

    true
}

fn render_powerline(c: char, width: u32, height: u32, out: &mut [u8]) -> bool {
    let t = (height / 14).max(1);
    let half_h = height as f32 / 2.0;
    if half_h <= 0.0 {
        return false;
    }

    match c {
        '\u{E0B0}' => {
            // Right-pointing solid triangle
            for y in 0..height {
                let fy = y as f32;
                let factor = if fy <= half_h {
                    fy / half_h
                } else {
                    (height as f32 - 1.0 - fy) / half_h
                };
                let x_limit = ((factor * width as f32).round() as u32).min(width);
                fill_rect(out, width, height, 0, y, (x_limit + 1).min(width), y + 1, 255);
            }
            true
        }
        '\u{E0B1}' => {
            // Right-pointing outline
            for y in 0..height {
                let fy = y as f32;
                let factor = if fy <= half_h {
                    fy / half_h
                } else {
                    (height as f32 - 1.0 - fy) / half_h
                };
                let x_pos = ((factor * width as f32).round() as u32).min(width.saturating_sub(1));
                let x0 = x_pos.saturating_sub(t / 2);
                let x1 = (x0 + t).min(width);
                fill_rect(out, width, height, x0, y, x1, y + 1, 255);
            }
            true
        }
        '\u{E0B2}' => {
            // Left-pointing solid triangle
            for y in 0..height {
                let fy = y as f32;
                let factor = if fy <= half_h {
                    1.0 - (fy / half_h)
                } else {
                    1.0 - ((height as f32 - 1.0 - fy) / half_h)
                };
                let x_start = ((factor * width as f32).round() as u32).min(width);
                fill_rect(out, width, height, x_start.saturating_sub(1), y, width, y + 1, 255);
            }
            true
        }
        '\u{E0B3}' => {
            // Left-pointing outline
            for y in 0..height {
                let fy = y as f32;
                let factor = if fy <= half_h {
                    1.0 - (fy / half_h)
                } else {
                    1.0 - ((height as f32 - 1.0 - fy) / half_h)
                };
                let x_pos = ((factor * width as f32).round() as u32).min(width.saturating_sub(1));
                let x0 = x_pos.saturating_sub(t / 2);
                let x1 = (x0 + t).min(width);
                fill_rect(out, width, height, x0, y, x1, y + 1, 255);
            }
            true
        }
        _ => false,
    }
}

fn render_sextant(c: char, width: u32, height: u32, out: &mut [u8]) -> bool {
    let cp = c as u32;
    if !(0x1FB00..=0x1FB3B).contains(&cp) {
        return false;
    }
    let offset = cp - 0x1FB00;
    // Decode sextant mask (6 bits: 0=TL, 1=TR, 2=ML, 3=MR, 4=BL, 5=BR)
    // Characters 0x1FB00..=0x1FB3B cover 59 patterns in index order
    let mask = sextant_mask_for_codepoint(offset);

    let mid_x = width / 2;
    let y0 = 0;
    let y1 = height / 3;
    let y2 = (2 * height) / 3;
    let y3 = height;

    if mask & (1 << 0) != 0 {
        fill_rect(out, width, height, 0, y0, mid_x, y1, 255);
    }
    if mask & (1 << 1) != 0 {
        fill_rect(out, width, height, mid_x, y0, width, y1, 255);
    }
    if mask & (1 << 2) != 0 {
        fill_rect(out, width, height, 0, y1, mid_x, y2, 255);
    }
    if mask & (1 << 3) != 0 {
        fill_rect(out, width, height, mid_x, y1, width, y2, 255);
    }
    if mask & (1 << 4) != 0 {
        fill_rect(out, width, height, 0, y2, mid_x, y3, 255);
    }
    if mask & (1 << 5) != 0 {
        fill_rect(out, width, height, mid_x, y2, width, y3, 255);
    }

    true
}

/// Map U+1FB00..=U+1FB3B index to the 6-bit sextant mask.
fn sextant_mask_for_codepoint(offset: u32) -> u8 {
    // 59 patterns where empty (0), full (63), and 4 block elements are skipped
    let mut current_idx = 0;
    for m in 1u8..63 {
        // Skip patterns that already exist in Unicode Block Elements (upper half, lower half, etc.)
        if m == 0b000111 || m == 0b111000 || m == 0b010101 || m == 0b101010 {
            continue;
        }
        if current_idx == offset {
            return m;
        }
        current_idx += 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_procedural() {
        assert!(is_procedural('\u{2500}')); // Box horizontal
        assert!(is_procedural('\u{2502}')); // Box vertical
        assert!(is_procedural('\u{2588}')); // Full block
        assert!(is_procedural('\u{28FF}')); // Braille all dots
        assert!(is_procedural('\u{E0B0}')); // Powerline
        assert!(is_procedural('\u{1FB00}')); // Sextant
        assert!(!is_procedural('A'));
        assert!(!is_procedural(' '));
        assert!(!is_procedural('0'));
    }

    #[test]
    fn test_full_block_fixture() {
        let w = 8;
        let h = 16;
        let mut buf = vec![0u8; (w * h) as usize];
        assert!(rasterize_procedural('\u{2588}', w, h, &mut buf));
        // Every pixel must be 255
        assert!(buf.iter().all(|&p| p == 255));
    }

    #[test]
    fn test_half_block_upper_lower_fixtures() {
        let w = 8;
        let h = 16;
        let mut buf = vec![0u8; (w * h) as usize];

        // Upper half (U+2580)
        assert!(rasterize_procedural('\u{2580}', w, h, &mut buf));
        for y in 0..h {
            for x in 0..w {
                let p = buf[(y * w + x) as usize];
                if y < h / 2 {
                    assert_eq!(p, 255, "Upper half should be filled at y={}", y);
                } else {
                    assert_eq!(p, 0, "Lower half should be empty at y={}", y);
                }
            }
        }

        // Lower half (U+2584)
        buf.fill(0);
        assert!(rasterize_procedural('\u{2584}', w, h, &mut buf));
        for y in 0..h {
            for x in 0..w {
                let p = buf[(y * w + x) as usize];
                if y >= h / 2 {
                    assert_eq!(p, 255, "Lower half should be filled at y={}", y);
                } else {
                    assert_eq!(p, 0, "Upper half should be empty at y={}", y);
                }
            }
        }
    }

    #[test]
    fn test_box_drawing_connects_to_edges() {
        let w = 10;
        let h = 18;
        let mut buf = vec![0u8; (w * h) as usize];

        // Horizontal line (U+2500)
        assert!(rasterize_procedural('\u{2500}', w, h, &mut buf));
        let cy = h / 2;
        // Edge pixels at x=0 and x=w-1 must be filled
        assert_eq!(buf[(cy * w) as usize], 255);
        assert_eq!(buf[(cy * w + (w - 1)) as usize], 255);

        // Vertical line (U+2502)
        buf.fill(0);
        assert!(rasterize_procedural('\u{2502}', w, h, &mut buf));
        let cx = w / 2;
        // Edge pixels at y=0 and y=h-1 must be filled
        assert_eq!(buf[cx as usize], 255);
        assert_eq!(buf[((h - 1) * w + cx) as usize], 255);
    }

    #[test]
    fn test_shades_density_fixtures() {
        let w = 16;
        let h = 16;
        let total = (w * h) as usize;
        let mut buf = vec![0u8; total];

        // Light shade U+2591: ~25%
        assert!(rasterize_procedural('\u{2591}', w, h, &mut buf));
        let count_light = buf.iter().filter(|&&p| p > 0).count();
        let ratio_light = count_light as f32 / total as f32;
        assert!((ratio_light - 0.25).abs() < 0.05);

        // Medium shade U+2592: 50%
        buf.fill(0);
        assert!(rasterize_procedural('\u{2592}', w, h, &mut buf));
        let count_med = buf.iter().filter(|&&p| p > 0).count();
        let ratio_med = count_med as f32 / total as f32;
        assert!((ratio_med - 0.50).abs() < 0.02);

        // Dark shade U+2593: ~75%
        buf.fill(0);
        assert!(rasterize_procedural('\u{2593}', w, h, &mut buf));
        let count_dark = buf.iter().filter(|&&p| p > 0).count();
        let ratio_dark = count_dark as f32 / total as f32;
        assert!((ratio_dark - 0.75).abs() < 0.05);
    }

    #[test]
    fn test_braille_dots_fixture() {
        let w = 10;
        let h = 24;
        let mut buf = vec![0u8; (w * h) as usize];
        // U+2801 is dot 1 only
        assert!(rasterize_procedural('\u{2801}', w, h, &mut buf));
        let count = buf.iter().filter(|&&p| p > 0).count();
        assert!(count > 0);
        // Dot 1 is in the top-left region: y < h/2, x < w/2
        for y in (h / 2)..h {
            for x in 0..w {
                assert_eq!(buf[(y * w + x) as usize], 0);
            }
        }
    }

    #[test]
    fn test_powerline_fixture() {
        let w = 12;
        let h = 20;
        let mut buf = vec![0u8; (w * h) as usize];
        assert!(rasterize_procedural('\u{E0B0}', w, h, &mut buf));
        // Point is at (w-1, h/2), left edge x=0 is completely filled
        for y in 0..h {
            assert_eq!(buf[(y * w) as usize], 255);
        }
    }
}
