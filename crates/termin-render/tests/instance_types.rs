use termin_render::{GlyphInst, Pod, QuadInst};

#[test]
fn test_quad_inst_size_and_alignment() {
    assert_eq!(std::mem::size_of::<QuadInst>(), 40);
    assert_eq!(std::mem::align_of::<QuadInst>(), 4);
}

#[test]
fn test_glyph_inst_size_and_alignment() {
    assert_eq!(std::mem::size_of::<GlyphInst>(), 24);
    assert_eq!(std::mem::align_of::<GlyphInst>(), 4);
}

#[test]
fn test_pod_safety_and_byte_transmute() {
    fn assert_pod<T: Pod>() {}
    assert_pod::<QuadInst>();
    assert_pod::<GlyphInst>();

    let quad = QuadInst {
        rect: [10.0, 20.0, 100.0, 50.0],
        color: 0xFF00AAFF,
        border_color: 0x00FF00FF,
        radius: 4.0,
        border: 1.5,
        shadow: 8.0,
        _pad: 0.0,
    };

    // SAFETY: QuadInst implements Pod and is guaranteed 40 bytes #[repr(C)] with no uninitialized bytes.
    let bytes: [u8; 40] = unsafe { std::mem::transmute(quad) };
    assert_eq!(bytes.len(), 40);

    // SAFETY: Round-trip transmute of valid Pod bytes.
    let restored: QuadInst = unsafe { std::mem::transmute(bytes) };
    assert_eq!(quad, restored);

    let glyph = GlyphInst {
        pos: [15.0, 25.0],
        size: [8, 16],
        uv: [0, 0, 8, 16],
        color: 0xFFFFFFFF,
    };

    // SAFETY: GlyphInst implements Pod and is guaranteed 24 bytes #[repr(C)] with no uninitialized bytes.
    let glyph_bytes: [u8; 24] = unsafe { std::mem::transmute(glyph) };
    assert_eq!(glyph_bytes.len(), 24);

    // SAFETY: Round-trip transmute of valid Pod bytes.
    let glyph_restored: GlyphInst = unsafe { std::mem::transmute(glyph_bytes) };
    assert_eq!(glyph, glyph_restored);
}
