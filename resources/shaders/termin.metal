// Termin Metal Shaders
// See docs/specs/text-and-rendering.md §3.2
#include <metal_stdlib>
using namespace metal;

struct VertexIn {
    float2 position [[attribute(0)]];
};

struct Uniforms {
    float2 viewport_size;       // viewport in physical pixels
    float scale_factor;         // logical to physical scale factor
    float contrast_boost;       // font.thicken coverage boost
    float srgb_to_linear[256];  // 256-entry piecewise sRGB linear decode table
};

// 40 bytes, align 4 - matching Rust #[repr(C)] QuadInst exactly
struct QuadInst {
    float rect[4];       // [x, y, w, h] in logical points
    uint color;          // 0xRRGGBBAA fill
    uint border_color;   // 0xRRGGBBAA border
    float radius;        // corner radius in logical points
    float border;        // border width in logical points
    float shadow;        // soft shadow blur in logical points
    float _pad;
};

// 24 bytes, align 4 - matching Rust #[repr(C)] GlyphInst exactly
struct GlyphInst {
    float pos[2];        // [x, y] in logical points
    ushort size[2];      // [w, h] in physical pixels
    ushort uv[4];        // [u0, v0, u1, v1] in atlas physical pixels
    uint color;          // 0xRRGGBBAA
};

// Varyings for Quad pipeline
struct QuadVaryings {
    float4 position [[position]];
    float2 local_pos;
    float2 half_size;
    float radius;
    float border;
    float shadow;
    uint fill_color;
    uint border_color;
};

// Varyings for Glyph/Icon pipelines
struct GlyphVaryings {
    float4 position [[position]];
    float2 uv_pixels;
    uint color;
};

inline float3 decode_srgb_rgb(uint color, constant Uniforms& uniforms) {
    uint r = (color >> 24) & 0xFF;
    uint g = (color >> 16) & 0xFF;
    uint b = (color >> 8) & 0xFF;
    return float3(
        uniforms.srgb_to_linear[r],
        uniforms.srgb_to_linear[g],
        uniforms.srgb_to_linear[b]
    );
}

inline float decode_alpha(uint color) {
    return float(color & 0xFF) / 255.0f;
}

// -----------------------------------------------------------------------------
// Quad Pipeline
// -----------------------------------------------------------------------------

vertex QuadVaryings quad_vertex(
    const device float2* vertices [[buffer(0)]],
    const device QuadInst* instances [[buffer(1)]],
    constant Uniforms& uniforms [[buffer(2)]],
    uint vertex_id [[vertex_id]],
    uint instance_id [[instance_id]]
) {
    float2 unit = vertices[vertex_id];
    QuadInst inst = instances[instance_id];

    float2 rect_size = float2(inst.rect[2], inst.rect[3]);
    float2 half_size = rect_size * 0.5f;
    float2 center = float2(inst.rect[0], inst.rect[1]) + half_size;

    // Expand bounding geometry for soft shadow and AA padding
    float extent = (inst.shadow > 0.0f)
        ? max(inst.shadow * 2.5f, 2.0f / uniforms.scale_factor)
        : (2.0f / uniforms.scale_factor);

    float2 local_pos = -half_size - extent + unit * (rect_size + 2.0f * extent);
    float2 phys_pos = (center + local_pos) * uniforms.scale_factor;

    QuadVaryings out;
    out.position.x = (phys_pos.x / (uniforms.viewport_size.x * 0.5f)) - 1.0f;
    out.position.y = 1.0f - (phys_pos.y / (uniforms.viewport_size.y * 0.5f));
    out.position.z = 0.0f;
    out.position.w = 1.0f;

    out.local_pos = local_pos;
    out.half_size = half_size;
    out.radius = inst.radius;
    out.border = inst.border;
    out.shadow = inst.shadow;
    out.fill_color = inst.color;
    out.border_color = inst.border_color;
    return out;
}

fragment float4 quad_fragment(
    QuadVaryings in [[stage_in]],
    constant Uniforms& uniforms [[buffer(0)]]
) {
    // 2D rounded box SDF
    float r = min(in.radius, min(in.half_size.x, in.half_size.y));
    float2 q = abs(in.local_pos) - in.half_size + r;
    float dist = min(max(q.x, q.y), 0.0f) + length(max(q, 0.0f)) - r;

    float dist_px = dist * uniforms.scale_factor;
    float outer_coverage = clamp(0.5f - dist_px, 0.0f, 1.0f);

    float inner_dist_px = (dist + in.border) * uniforms.scale_factor;
    float inner_coverage = clamp(0.5f - inner_dist_px, 0.0f, 1.0f);

    float border_cov = (in.border > 0.0f) ? (outer_coverage - inner_coverage) : 0.0f;
    float fill_cov = (in.border > 0.0f) ? inner_coverage : outer_coverage;

    float3 fill_rgb = decode_srgb_rgb(in.fill_color, uniforms);
    float fill_a = decode_alpha(in.fill_color) * fill_cov;

    float3 border_rgb = decode_srgb_rgb(in.border_color, uniforms);
    float border_a = decode_alpha(in.border_color) * border_cov;

    // Composite border over fill in linear premultiplied space
    float3 rect_rgb = border_rgb * border_a + fill_rgb * fill_a * (1.0f - border_a);
    float rect_a = border_a + fill_a * (1.0f - border_a);

    // Soft drop shadow (approximated single-pass SDF blur)
    float shadow_alpha = 0.0f;
    if (in.shadow > 0.0f && dist > 0.0f) {
        float sigma = in.shadow * 0.5f;
        float s = exp(-0.5f * (dist / sigma) * (dist / sigma));
        // design-system.md: shadow token is #000 @ 45%
        shadow_alpha = s * 0.45f * (1.0f - outer_coverage);
    }

    float3 out_rgb = rect_rgb;
    float out_a = rect_a + shadow_alpha * (1.0f - rect_a);

    if (out_a <= 0.001f) {
        discard_fragment();
    }

    return float4(out_rgb, out_a);
}

// -----------------------------------------------------------------------------
// Glyph & Icon Pipelines
// -----------------------------------------------------------------------------

vertex GlyphVaryings glyph_vertex(
    const device float2* vertices [[buffer(0)]],
    const device GlyphInst* instances [[buffer(1)]],
    constant Uniforms& uniforms [[buffer(2)]],
    uint vertex_id [[vertex_id]],
    uint instance_id [[instance_id]]
) {
    float2 unit = vertices[vertex_id];
    GlyphInst inst = instances[instance_id];

    float2 inst_pos = float2(inst.pos[0], inst.pos[1]);
    float2 pos_phys = round(inst_pos * uniforms.scale_factor);
    float2 size_phys = float2(inst.size[0], inst.size[1]);
    float2 phys_pos = pos_phys + unit * size_phys;

    GlyphVaryings out;
    out.position.x = (phys_pos.x / (uniforms.viewport_size.x * 0.5f)) - 1.0f;
    out.position.y = 1.0f - (phys_pos.y / (uniforms.viewport_size.y * 0.5f));
    out.position.z = 0.0f;
    out.position.w = 1.0f;

    out.uv_pixels = mix(float2(inst.uv[0], inst.uv[1]), float2(inst.uv[2], inst.uv[3]), unit);
    out.color = inst.color;
    return out;
}

fragment float4 glyph_mask_fragment(
    GlyphVaryings in [[stage_in]],
    texture2d<float> mask_atlas [[texture(0)]],
    sampler s [[sampler(0)]],
    constant Uniforms& uniforms [[buffer(0)]]
) {
    float2 tex_size = float2(mask_atlas.get_width(), mask_atlas.get_height());
    float2 norm_uv = in.uv_pixels / tex_size;
    float coverage = mask_atlas.sample(s, norm_uv).r;

    // Contrast boost (font.thicken) for crisp text on dark backgrounds
    if (uniforms.contrast_boost > 0.0f) {
        coverage = saturate(coverage + uniforms.contrast_boost * coverage * (1.0f - coverage));
    }

    if (coverage <= 0.001f) {
        discard_fragment();
    }

    float3 linear_rgb = decode_srgb_rgb(in.color, uniforms);
    float alpha = decode_alpha(in.color) * coverage;
    return float4(linear_rgb * alpha, alpha);
}

fragment float4 glyph_color_fragment(
    GlyphVaryings in [[stage_in]],
    texture2d<float> color_atlas [[texture(0)]],
    sampler s [[sampler(0)]],
    constant Uniforms& uniforms [[buffer(0)]]
) {
    float2 tex_size = float2(color_atlas.get_width(), color_atlas.get_height());
    float2 norm_uv = in.uv_pixels / tex_size;
    float4 tex_color = color_atlas.sample(s, norm_uv);

    float opacity = decode_alpha(in.color);
    float4 out_color = tex_color * opacity;

    if (out_color.a <= 0.001f) {
        discard_fragment();
    }

    return out_color;
}

fragment float4 icon_fragment(
    GlyphVaryings in [[stage_in]],
    texture2d<float> mask_atlas [[texture(0)]],
    sampler s [[sampler(0)]],
    constant Uniforms& uniforms [[buffer(0)]]
) {
    float2 tex_size = float2(mask_atlas.get_width(), mask_atlas.get_height());
    float2 norm_uv = in.uv_pixels / tex_size;
    float coverage = mask_atlas.sample(s, norm_uv).r;

    if (coverage <= 0.001f) {
        discard_fragment();
    }

    float3 linear_rgb = decode_srgb_rgb(in.color, uniforms);
    float alpha = decode_alpha(in.color) * coverage;
    return float4(linear_rgb * alpha, alpha);
}
