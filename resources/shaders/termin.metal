// Termin Metal Shaders
// See docs/specs/text-and-rendering.md §3.2
#include <metal_stdlib>
using namespace metal;

struct QuadInst {
    float4 rect;
    uint color;
    uint border_color;
    float radius;
    float border;
    float shadow;
    float _pad;
};

struct GlyphInst {
    float2 pos;
    ushort2 size;
    ushort4 uv;
    uint color;
};

// Shaders will be fully implemented in Phase 1E
