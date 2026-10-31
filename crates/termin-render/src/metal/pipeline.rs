//! Metal pipeline state compilation and sampler management.
//!
//! See `docs/specs/text-and-rendering.md §3.2`.

use std::path::Path;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSString, NSURL};
use objc2_metal::{
    MTLBlendFactor, MTLBlendOperation, MTLDevice, MTLLibrary, MTLPixelFormat,
    MTLRenderPipelineDescriptor, MTLRenderPipelineState, MTLSamplerAddressMode,
    MTLSamplerDescriptor, MTLSamplerMinMagFilter, MTLSamplerState,
};

use super::RenderError;

const METALLIB_PATH: &str = concat!(env!("OUT_DIR"), "/termin.metallib");
const SHADER_SOURCE: &str = include_str!("../../../../resources/shaders/termin.metal");

/// Container for all four compiled render pipelines and texture sampler.
pub struct Pipelines {
    pub quad: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    pub glyph_mask: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    pub glyph_color: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    pub icon: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    pub sampler: Retained<ProtocolObject<dyn MTLSamplerState>>,
}

impl Pipelines {
    /// Create and compile all pipelines using embedded .metallib or runtime source fallback.
    pub fn new(device: &ProtocolObject<dyn MTLDevice>) -> Result<Self, RenderError> {
        let library = load_library(device)?;

        let quad = create_pipeline(device, &library, "quad_vertex", "quad_fragment")?;
        let glyph_mask =
            create_pipeline(device, &library, "glyph_vertex", "glyph_mask_fragment")?;
        let glyph_color =
            create_pipeline(device, &library, "glyph_vertex", "glyph_color_fragment")?;
        let icon = create_pipeline(device, &library, "glyph_vertex", "icon_fragment")?;
        let sampler = create_sampler(device)?;

        Ok(Self {
            quad,
            glyph_mask,
            glyph_color,
            icon,
            sampler,
        })
    }
}

/// Load the shader library from precompiled metallib file if available, or fall back to runtime compilation.
fn load_library(
    device: &ProtocolObject<dyn MTLDevice>,
) -> Result<Retained<ProtocolObject<dyn MTLLibrary>>, RenderError> {
    if Path::new(METALLIB_PATH).exists() {
        if let Ok(metadata) = std::fs::metadata(METALLIB_PATH) {
            if metadata.len() > 0 {
                let ns_path = NSString::from_str(METALLIB_PATH);
                // SAFETY: Creating file URL and calling device newLibraryWithURL_error with valid path.
                let url = unsafe { NSURL::fileURLWithPath(&ns_path) };
                let lib_res = unsafe { device.newLibraryWithURL_error(&url) };
                if let Ok(lib) = lib_res {
                    return Ok(lib);
                }
            }
        }
    }

    log::warn!("Metal toolchain unavailable; compiled shaders from source at runtime");
    let ns_source = NSString::from_str(SHADER_SOURCE);
    device
        .newLibraryWithSource_options_error(&ns_source, None)
        .map_err(|e| RenderError::ShaderCompilationFailed(e.to_string()))
}

/// Helper function to create a render pipeline state with premultiplied alpha blending into BGRA8Unorm_sRGB.
fn create_pipeline(
    device: &ProtocolObject<dyn MTLDevice>,
    library: &ProtocolObject<dyn MTLLibrary>,
    vertex_name: &str,
    fragment_name: &str,
) -> Result<Retained<ProtocolObject<dyn MTLRenderPipelineState>>, RenderError> {
    let ns_vert = NSString::from_str(vertex_name);
    let ns_frag = NSString::from_str(fragment_name);

    let vert_func = library
        .newFunctionWithName(&ns_vert)
        .ok_or_else(|| RenderError::PipelineCreationFailed(format!("vertex function not found: {vertex_name}")))?;

    let frag_func = library
        .newFunctionWithName(&ns_frag)
        .ok_or_else(|| RenderError::PipelineCreationFailed(format!("fragment function not found: {fragment_name}")))?;

    let desc = MTLRenderPipelineDescriptor::new();
    desc.setVertexFunction(Some(&vert_func));
    desc.setFragmentFunction(Some(&frag_func));

    // SAFETY: Index 0 is guaranteed valid for the main color attachment in MTLRenderPipelineColorAttachmentDescriptorArray.
    let color_att = unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) };
    color_att.setPixelFormat(MTLPixelFormat::BGRA8Unorm_sRGB);
    color_att.setBlendingEnabled(true);

    // Premultiplied alpha blending:
    // dst.rgb = src.rgb * 1 + dst.rgb * (1 - src.a)
    // dst.a   = src.a   * 1 + dst.a   * (1 - src.a)
    color_att.setSourceRGBBlendFactor(MTLBlendFactor::One);
    color_att.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
    color_att.setRgbBlendOperation(MTLBlendOperation::Add);
    color_att.setSourceAlphaBlendFactor(MTLBlendFactor::One);
    color_att.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
    color_att.setAlphaBlendOperation(MTLBlendOperation::Add);

    device
        .newRenderPipelineStateWithDescriptor_error(&desc)
        .map_err(|e| RenderError::PipelineCreationFailed(e.to_string()))
}

/// Create the linear sampler state with clamp-to-edge addressing.
fn create_sampler(
    device: &ProtocolObject<dyn MTLDevice>,
) -> Result<Retained<ProtocolObject<dyn MTLSamplerState>>, RenderError> {
    let desc = MTLSamplerDescriptor::new();
    desc.setMinFilter(MTLSamplerMinMagFilter::Linear);
    desc.setMagFilter(MTLSamplerMinMagFilter::Linear);
    desc.setSAddressMode(MTLSamplerAddressMode::ClampToEdge);
    desc.setTAddressMode(MTLSamplerAddressMode::ClampToEdge);

    device
        .newSamplerStateWithDescriptor(&desc)
        .ok_or_else(|| RenderError::PipelineCreationFailed("failed to create sampler state".into()))
}
