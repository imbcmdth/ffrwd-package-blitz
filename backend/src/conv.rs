//! wgpu's types to the wit's and back. The flags (buffer and texture usage,
//! shader stages, colour writes, map modes) carry WebGPU's bit values on
//! both sides, so they pass through as bits.

use crate::bindings::wasi::webgpu::webgpu as w;
use wgpu::{self as wgt};

pub fn texture_format(f: wgt::TextureFormat) -> w::GpuTextureFormat {
    use w::GpuTextureFormat as W;
    use wgt::TextureFormat as F;
    match f {
        F::R8Unorm => W::R8unorm,
        F::R8Snorm => W::R8snorm,
        F::R8Uint => W::R8uint,
        F::R8Sint => W::R8sint,
        F::R16Uint => W::R16uint,
        F::R16Sint => W::R16sint,
        F::R16Float => W::R16float,
        F::Rg8Unorm => W::Rg8unorm,
        F::Rg8Snorm => W::Rg8snorm,
        F::Rg8Uint => W::Rg8uint,
        F::Rg8Sint => W::Rg8sint,
        F::R32Uint => W::R32uint,
        F::R32Sint => W::R32sint,
        F::R32Float => W::R32float,
        F::Rg16Uint => W::Rg16uint,
        F::Rg16Sint => W::Rg16sint,
        F::Rg16Float => W::Rg16float,
        F::Rgba8Unorm => W::Rgba8unorm,
        F::Rgba8UnormSrgb => W::Rgba8unormSrgb,
        F::Rgba8Snorm => W::Rgba8snorm,
        F::Rgba8Uint => W::Rgba8uint,
        F::Rgba8Sint => W::Rgba8sint,
        F::Bgra8Unorm => W::Bgra8unorm,
        F::Bgra8UnormSrgb => W::Bgra8unormSrgb,
        F::Rgb9e5Ufloat => W::Rgb9e5ufloat,
        F::Rgb10a2Uint => W::Rgb10a2uint,
        F::Rgb10a2Unorm => W::Rgb10a2unorm,
        F::Rg11b10Ufloat => W::Rg11b10ufloat,
        F::Rg32Uint => W::Rg32uint,
        F::Rg32Sint => W::Rg32sint,
        F::Rg32Float => W::Rg32float,
        F::Rgba16Uint => W::Rgba16uint,
        F::Rgba16Sint => W::Rgba16sint,
        F::Rgba16Float => W::Rgba16float,
        F::Rgba32Uint => W::Rgba32uint,
        F::Rgba32Sint => W::Rgba32sint,
        F::Rgba32Float => W::Rgba32float,
        F::Stencil8 => W::Stencil8,
        F::Depth16Unorm => W::Depth16unorm,
        F::Depth24Plus => W::Depth24plus,
        F::Depth24PlusStencil8 => W::Depth24plusStencil8,
        F::Depth32Float => W::Depth32float,
        F::Depth32FloatStencil8 => W::Depth32floatStencil8,
        other => todo!("wasi:webgpu: texture format {other:?}"),
    }
}

pub fn texture_dimension(d: wgt::TextureDimension) -> w::GpuTextureDimension {
    match d {
        wgt::TextureDimension::D1 => w::GpuTextureDimension::D1,
        wgt::TextureDimension::D2 => w::GpuTextureDimension::D2,
        wgt::TextureDimension::D3 => w::GpuTextureDimension::D3,
    }
}

pub fn view_dimension(d: wgt::TextureViewDimension) -> w::GpuTextureViewDimension {
    use wgt::TextureViewDimension as V;
    match d {
        V::D1 => w::GpuTextureViewDimension::D1,
        V::D2 => w::GpuTextureViewDimension::D2,
        V::D2Array => w::GpuTextureViewDimension::D2Array,
        V::Cube => w::GpuTextureViewDimension::Cube,
        V::CubeArray => w::GpuTextureViewDimension::CubeArray,
        V::D3 => w::GpuTextureViewDimension::D3,
    }
}

pub fn aspect(a: wgt::TextureAspect) -> w::GpuTextureAspect {
    match a {
        wgt::TextureAspect::All => w::GpuTextureAspect::All,
        wgt::TextureAspect::StencilOnly => w::GpuTextureAspect::StencilOnly,
        wgt::TextureAspect::DepthOnly => w::GpuTextureAspect::DepthOnly,
        other => todo!("wasi:webgpu: texture aspect {other:?}"),
    }
}

pub fn extent(e: wgt::Extent3d) -> w::GpuExtent3D {
    w::GpuExtent3D {
        width: e.width,
        height: Some(e.height),
        depth_or_array_layers: Some(e.depth_or_array_layers),
    }
}

pub fn origin(o: wgt::Origin3d) -> w::GpuOrigin3D {
    w::GpuOrigin3D {
        x: Some(o.x),
        y: Some(o.y),
        z: Some(o.z),
    }
}

pub fn color(c: wgt::Color) -> w::GpuColor {
    w::GpuColor {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
}

pub fn address_mode(m: wgt::AddressMode) -> w::GpuAddressMode {
    match m {
        wgt::AddressMode::ClampToEdge => w::GpuAddressMode::ClampToEdge,
        wgt::AddressMode::Repeat => w::GpuAddressMode::Repeat,
        wgt::AddressMode::MirrorRepeat => w::GpuAddressMode::MirrorRepeat,
        other => todo!("wasi:webgpu: address mode {other:?}"),
    }
}

pub fn filter(m: wgt::FilterMode) -> w::GpuFilterMode {
    match m {
        wgt::FilterMode::Nearest => w::GpuFilterMode::Nearest,
        wgt::FilterMode::Linear => w::GpuFilterMode::Linear,
    }
}

pub fn mipmap_filter(m: wgt::MipmapFilterMode) -> w::GpuMipmapFilterMode {
    match m {
        wgt::MipmapFilterMode::Nearest => w::GpuMipmapFilterMode::Nearest,
        wgt::MipmapFilterMode::Linear => w::GpuMipmapFilterMode::Linear,
    }
}

pub fn compare(c: wgt::CompareFunction) -> w::GpuCompareFunction {
    use wgt::CompareFunction as C;
    match c {
        C::Never => w::GpuCompareFunction::Never,
        C::Less => w::GpuCompareFunction::Less,
        C::Equal => w::GpuCompareFunction::Equal,
        C::LessEqual => w::GpuCompareFunction::LessEqual,
        C::Greater => w::GpuCompareFunction::Greater,
        C::NotEqual => w::GpuCompareFunction::NotEqual,
        C::GreaterEqual => w::GpuCompareFunction::GreaterEqual,
        C::Always => w::GpuCompareFunction::Always,
    }
}

pub fn buffer_binding_type(t: wgt::BufferBindingType) -> w::GpuBufferBindingType {
    match t {
        wgt::BufferBindingType::Uniform => w::GpuBufferBindingType::Uniform,
        wgt::BufferBindingType::Storage { read_only: false } => w::GpuBufferBindingType::Storage,
        wgt::BufferBindingType::Storage { read_only: true } => w::GpuBufferBindingType::ReadOnlyStorage,
    }
}

pub fn sampler_binding_type(t: wgt::SamplerBindingType) -> w::GpuSamplerBindingType {
    match t {
        wgt::SamplerBindingType::Filtering => w::GpuSamplerBindingType::Filtering,
        wgt::SamplerBindingType::NonFiltering => w::GpuSamplerBindingType::NonFiltering,
        wgt::SamplerBindingType::Comparison => w::GpuSamplerBindingType::Comparison,
    }
}

pub fn sample_type(t: wgt::TextureSampleType) -> w::GpuTextureSampleType {
    match t {
        wgt::TextureSampleType::Float { filterable: true } => w::GpuTextureSampleType::Float,
        wgt::TextureSampleType::Float { filterable: false } => w::GpuTextureSampleType::UnfilterableFloat,
        wgt::TextureSampleType::Depth => w::GpuTextureSampleType::Depth,
        wgt::TextureSampleType::Sint => w::GpuTextureSampleType::Sint,
        wgt::TextureSampleType::Uint => w::GpuTextureSampleType::Uint,
    }
}

pub fn storage_access(a: wgt::StorageTextureAccess) -> w::GpuStorageTextureAccess {
    match a {
        wgt::StorageTextureAccess::WriteOnly => w::GpuStorageTextureAccess::WriteOnly,
        wgt::StorageTextureAccess::ReadOnly => w::GpuStorageTextureAccess::ReadOnly,
        wgt::StorageTextureAccess::ReadWrite => w::GpuStorageTextureAccess::ReadWrite,
        other => todo!("wasi:webgpu: storage texture access {other:?}"),
    }
}

pub fn layout_entry(e: &wgt::BindGroupLayoutEntry) -> w::GpuBindGroupLayoutEntry {
    if e.count.is_some() {
        todo!("wasi:webgpu: binding arrays");
    }
    let mut entry = w::GpuBindGroupLayoutEntry {
        binding: e.binding,
        visibility: e.visibility.bits(),
        buffer: None,
        sampler: None,
        texture: None,
        storage_texture: None,
    };
    match e.ty {
        wgt::BindingType::Buffer {
            ty,
            has_dynamic_offset,
            min_binding_size,
        } => {
            entry.buffer = Some(w::GpuBufferBindingLayout {
                type_: Some(buffer_binding_type(ty)),
                has_dynamic_offset: Some(has_dynamic_offset),
                min_binding_size: min_binding_size.map(|s| s.get()),
            });
        }
        wgt::BindingType::Sampler(t) => {
            entry.sampler = Some(w::GpuSamplerBindingLayout {
                type_: Some(sampler_binding_type(t)),
            });
        }
        wgt::BindingType::Texture {
            sample_type: st,
            view_dimension: vd,
            multisampled,
        } => {
            entry.texture = Some(w::GpuTextureBindingLayout {
                sample_type: Some(sample_type(st)),
                view_dimension: Some(view_dimension(vd)),
                multisampled: Some(multisampled),
            });
        }
        wgt::BindingType::StorageTexture {
            access,
            format,
            view_dimension: vd,
        } => {
            entry.storage_texture = Some(w::GpuStorageTextureBindingLayout {
                access: Some(storage_access(access)),
                format: texture_format(format),
                view_dimension: Some(view_dimension(vd)),
            });
        }
        other => todo!("wasi:webgpu: binding type {other:?}"),
    }
    entry
}

pub fn topology(t: wgt::PrimitiveTopology) -> w::GpuPrimitiveTopology {
    use wgt::PrimitiveTopology as T;
    match t {
        T::PointList => w::GpuPrimitiveTopology::PointList,
        T::LineList => w::GpuPrimitiveTopology::LineList,
        T::LineStrip => w::GpuPrimitiveTopology::LineStrip,
        T::TriangleList => w::GpuPrimitiveTopology::TriangleList,
        T::TriangleStrip => w::GpuPrimitiveTopology::TriangleStrip,
    }
}

pub fn index_format(f: wgt::IndexFormat) -> w::GpuIndexFormat {
    match f {
        wgt::IndexFormat::Uint16 => w::GpuIndexFormat::Uint16,
        wgt::IndexFormat::Uint32 => w::GpuIndexFormat::Uint32,
    }
}

pub fn front_face(f: wgt::FrontFace) -> w::GpuFrontFace {
    match f {
        wgt::FrontFace::Ccw => w::GpuFrontFace::Ccw,
        wgt::FrontFace::Cw => w::GpuFrontFace::Cw,
    }
}

pub fn cull_mode(f: Option<wgt::Face>) -> w::GpuCullMode {
    match f {
        None => w::GpuCullMode::None,
        Some(wgt::Face::Front) => w::GpuCullMode::Front,
        Some(wgt::Face::Back) => w::GpuCullMode::Back,
    }
}

pub fn blend_factor(f: wgt::BlendFactor) -> w::GpuBlendFactor {
    use wgt::BlendFactor as B;
    match f {
        B::Zero => w::GpuBlendFactor::Zero,
        B::One => w::GpuBlendFactor::One,
        B::Src => w::GpuBlendFactor::Src,
        B::OneMinusSrc => w::GpuBlendFactor::OneMinusSrc,
        B::SrcAlpha => w::GpuBlendFactor::SrcAlpha,
        B::OneMinusSrcAlpha => w::GpuBlendFactor::OneMinusSrcAlpha,
        B::Dst => w::GpuBlendFactor::Dst,
        B::OneMinusDst => w::GpuBlendFactor::OneMinusDst,
        B::DstAlpha => w::GpuBlendFactor::DstAlpha,
        B::OneMinusDstAlpha => w::GpuBlendFactor::OneMinusDstAlpha,
        B::SrcAlphaSaturated => w::GpuBlendFactor::SrcAlphaSaturated,
        B::Constant => w::GpuBlendFactor::Constant,
        B::OneMinusConstant => w::GpuBlendFactor::OneMinusConstant,
        B::Src1 => w::GpuBlendFactor::Src1,
        B::OneMinusSrc1 => w::GpuBlendFactor::OneMinusSrc1,
        B::Src1Alpha => w::GpuBlendFactor::Src1Alpha,
        B::OneMinusSrc1Alpha => w::GpuBlendFactor::OneMinusSrc1Alpha,
    }
}

pub fn blend_operation(o: wgt::BlendOperation) -> w::GpuBlendOperation {
    use wgt::BlendOperation as O;
    match o {
        O::Add => w::GpuBlendOperation::Add,
        O::Subtract => w::GpuBlendOperation::Subtract,
        O::ReverseSubtract => w::GpuBlendOperation::ReverseSubtract,
        O::Min => w::GpuBlendOperation::Min,
        O::Max => w::GpuBlendOperation::Max,
    }
}

pub fn blend_component(c: wgt::BlendComponent) -> w::GpuBlendComponent {
    w::GpuBlendComponent {
        operation: Some(blend_operation(c.operation)),
        src_factor: Some(blend_factor(c.src_factor)),
        dst_factor: Some(blend_factor(c.dst_factor)),
    }
}

pub fn color_target(t: &wgt::ColorTargetState) -> w::GpuColorTargetState {
    w::GpuColorTargetState {
        format: texture_format(t.format),
        blend: t.blend.map(|b| w::GpuBlendState {
            color: blend_component(b.color),
            alpha: blend_component(b.alpha),
        }),
        write_mask: Some(t.write_mask.bits()),
    }
}

pub fn stencil_operation(o: wgt::StencilOperation) -> w::GpuStencilOperation {
    use wgt::StencilOperation as S;
    match o {
        S::Keep => w::GpuStencilOperation::Keep,
        S::Zero => w::GpuStencilOperation::Zero,
        S::Replace => w::GpuStencilOperation::Replace,
        S::Invert => w::GpuStencilOperation::Invert,
        S::IncrementClamp => w::GpuStencilOperation::IncrementClamp,
        S::DecrementClamp => w::GpuStencilOperation::DecrementClamp,
        S::IncrementWrap => w::GpuStencilOperation::IncrementWrap,
        S::DecrementWrap => w::GpuStencilOperation::DecrementWrap,
    }
}

pub fn stencil_face(s: wgt::StencilFaceState) -> w::GpuStencilFaceState {
    w::GpuStencilFaceState {
        compare: Some(compare(s.compare)),
        fail_op: Some(stencil_operation(s.fail_op)),
        depth_fail_op: Some(stencil_operation(s.depth_fail_op)),
        pass_op: Some(stencil_operation(s.pass_op)),
    }
}

pub fn depth_stencil(d: &wgt::DepthStencilState) -> w::GpuDepthStencilState {
    w::GpuDepthStencilState {
        format: texture_format(d.format),
        depth_write_enabled: d.depth_write_enabled,
        depth_compare: d.depth_compare.map(compare),
        stencil_front: Some(stencil_face(d.stencil.front)),
        stencil_back: Some(stencil_face(d.stencil.back)),
        stencil_read_mask: Some(d.stencil.read_mask),
        stencil_write_mask: Some(d.stencil.write_mask),
        depth_bias: Some(d.bias.constant),
        depth_bias_slope_scale: Some(d.bias.slope_scale),
        depth_bias_clamp: Some(d.bias.clamp),
    }
}

pub fn vertex_format(f: wgt::VertexFormat) -> w::GpuVertexFormat {
    use w::GpuVertexFormat as W;
    use wgt::VertexFormat as V;
    match f {
        V::Uint8 => W::Uint8,
        V::Uint8x2 => W::Uint8x2,
        V::Uint8x4 => W::Uint8x4,
        V::Sint8 => W::Sint8,
        V::Sint8x2 => W::Sint8x2,
        V::Sint8x4 => W::Sint8x4,
        V::Unorm8 => W::Unorm8,
        V::Unorm8x2 => W::Unorm8x2,
        V::Unorm8x4 => W::Unorm8x4,
        V::Snorm8 => W::Snorm8,
        V::Snorm8x2 => W::Snorm8x2,
        V::Snorm8x4 => W::Snorm8x4,
        V::Uint16 => W::Uint16,
        V::Uint16x2 => W::Uint16x2,
        V::Uint16x4 => W::Uint16x4,
        V::Sint16 => W::Sint16,
        V::Sint16x2 => W::Sint16x2,
        V::Sint16x4 => W::Sint16x4,
        V::Unorm16 => W::Unorm16,
        V::Unorm16x2 => W::Unorm16x2,
        V::Unorm16x4 => W::Unorm16x4,
        V::Snorm16 => W::Snorm16,
        V::Snorm16x2 => W::Snorm16x2,
        V::Snorm16x4 => W::Snorm16x4,
        V::Float16 => W::Float16,
        V::Float16x2 => W::Float16x2,
        V::Float16x4 => W::Float16x4,
        V::Float32 => W::Float32,
        V::Float32x2 => W::Float32x2,
        V::Float32x3 => W::Float32x3,
        V::Float32x4 => W::Float32x4,
        V::Uint32 => W::Uint32,
        V::Uint32x2 => W::Uint32x2,
        V::Uint32x3 => W::Uint32x3,
        V::Uint32x4 => W::Uint32x4,
        V::Sint32 => W::Sint32,
        V::Sint32x2 => W::Sint32x2,
        V::Sint32x3 => W::Sint32x3,
        V::Sint32x4 => W::Sint32x4,
        V::Unorm10_10_10_2 => W::Unorm1010102,
        V::Unorm8x4Bgra => W::Unorm8x4Bgra,
        other => todo!("wasi:webgpu: vertex format {other:?}"),
    }
}

pub fn step_mode(m: wgt::VertexStepMode) -> w::GpuVertexStepMode {
    match m {
        wgt::VertexStepMode::Vertex => w::GpuVertexStepMode::Vertex,
        wgt::VertexStepMode::Instance => w::GpuVertexStepMode::Instance,
    }
}

pub fn vertex_buffer_layout(l: &wgt::VertexBufferLayout<'_>) -> w::GpuVertexBufferLayout {
    w::GpuVertexBufferLayout {
        array_stride: l.array_stride,
        step_mode: Some(step_mode(l.step_mode)),
        attributes: l
            .attributes
            .iter()
            .map(|a| w::GpuVertexAttribute {
                format: vertex_format(a.format),
                offset: a.offset,
                shader_location: a.shader_location,
            })
            .collect(),
    }
}

pub fn load_op<V>(op: &wgt::LoadOp<V>) -> w::GpuLoadOp {
    match op {
        wgt::LoadOp::Clear(_) => w::GpuLoadOp::Clear,
        _ => w::GpuLoadOp::Load,
    }
}

pub fn store_op(op: wgt::StoreOp) -> w::GpuStoreOp {
    match op {
        wgt::StoreOp::Store => w::GpuStoreOp::Store,
        wgt::StoreOp::Discard => w::GpuStoreOp::Discard,
    }
}

pub fn query_type(t: wgt::QueryType) -> w::GpuQueryType {
    match t {
        wgt::QueryType::Occlusion => w::GpuQueryType::Occlusion,
        wgt::QueryType::Timestamp => w::GpuQueryType::Timestamp,
        other => todo!("wasi:webgpu: query type {other:?}"),
    }
}

pub fn buffer_layout(l: wgt::TexelCopyBufferLayout) -> w::GpuTexelCopyBufferLayout {
    w::GpuTexelCopyBufferLayout {
        offset: Some(l.offset),
        bytes_per_row: l.bytes_per_row,
        rows_per_image: l.rows_per_image,
    }
}

/// The features the wit can name, by wgpu's flag.
pub const FEATURES: &[(&str, wgt::Features)] = &[
    ("depth-clip-control", wgt::Features::DEPTH_CLIP_CONTROL),
    ("depth32float-stencil8", wgt::Features::DEPTH32FLOAT_STENCIL8),
    ("texture-compression-bc", wgt::Features::TEXTURE_COMPRESSION_BC),
    ("texture-compression-bc-sliced3d", wgt::Features::TEXTURE_COMPRESSION_BC_SLICED_3D),
    ("texture-compression-etc2", wgt::Features::TEXTURE_COMPRESSION_ETC2),
    ("texture-compression-astc", wgt::Features::TEXTURE_COMPRESSION_ASTC),
    ("timestamp-query", wgt::Features::TIMESTAMP_QUERY),
    ("indirect-first-instance", wgt::Features::INDIRECT_FIRST_INSTANCE),
    ("shader-f16", wgt::Features::SHADER_F16),
    ("rg11b10ufloat-renderable", wgt::Features::RG11B10UFLOAT_RENDERABLE),
    ("bgra8unorm-storage", wgt::Features::BGRA8UNORM_STORAGE),
    ("float32-filterable", wgt::Features::FLOAT32_FILTERABLE),
    ("float32-blendable", wgt::Features::FLOAT32_BLENDABLE),
    ("clip-distances", wgt::Features::CLIP_DISTANCES),
    ("dual-source-blending", wgt::Features::DUAL_SOURCE_BLENDING),
    ("subgroups", wgt::Features::SUBGROUP),
];

pub fn feature_name(name: &str) -> Option<w::GpuFeatureName> {
    use w::GpuFeatureName as N;
    Some(match name {
        "depth-clip-control" => N::DepthClipControl,
        "depth32float-stencil8" => N::Depth32floatStencil8,
        "texture-compression-bc" => N::TextureCompressionBc,
        "texture-compression-bc-sliced3d" => N::TextureCompressionBcSliced3d,
        "texture-compression-etc2" => N::TextureCompressionEtc2,
        "texture-compression-astc" => N::TextureCompressionAstc,
        "timestamp-query" => N::TimestampQuery,
        "indirect-first-instance" => N::IndirectFirstInstance,
        "shader-f16" => N::ShaderF16,
        "rg11b10ufloat-renderable" => N::Rg11b10ufloatRenderable,
        "bgra8unorm-storage" => N::Bgra8unormStorage,
        "float32-filterable" => N::Float32Filterable,
        "float32-blendable" => N::Float32Blendable,
        "clip-distances" => N::ClipDistances,
        "dual-source-blending" => N::DualSourceBlending,
        "subgroups" => N::Subgroups,
        _ => return None,
    })
}

/// wgpu's features from the host's: the wit's names, plus encoder
/// timestamps where timestamp queries are, which the host grants together.
pub fn features(f: &w::GpuSupportedFeatures) -> wgt::Features {
    let mut out = wgt::Features::empty();
    for (name, flag) in FEATURES {
        if f.has(name) {
            out |= *flag;
        }
    }
    if out.contains(wgt::Features::TIMESTAMP_QUERY) {
        out |= wgt::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    }
    out
}

pub fn limits(l: &w::GpuSupportedLimits) -> wgt::Limits {
    wgt::Limits {
        max_texture_dimension_1d: l.max_texture_dimension1_d(),
        max_texture_dimension_2d: l.max_texture_dimension2_d(),
        max_texture_dimension_3d: l.max_texture_dimension3_d(),
        max_texture_array_layers: l.max_texture_array_layers(),
        max_bind_groups: l.max_bind_groups(),
        max_bindings_per_bind_group: l.max_bindings_per_bind_group(),
        max_dynamic_uniform_buffers_per_pipeline_layout: l.max_dynamic_uniform_buffers_per_pipeline_layout(),
        max_dynamic_storage_buffers_per_pipeline_layout: l.max_dynamic_storage_buffers_per_pipeline_layout(),
        max_sampled_textures_per_shader_stage: l.max_sampled_textures_per_shader_stage(),
        max_samplers_per_shader_stage: l.max_samplers_per_shader_stage(),
        max_storage_buffers_per_shader_stage: l.max_storage_buffers_per_shader_stage(),
        max_storage_textures_per_shader_stage: l.max_storage_textures_per_shader_stage(),
        max_uniform_buffers_per_shader_stage: l.max_uniform_buffers_per_shader_stage(),
        max_uniform_buffer_binding_size: l.max_uniform_buffer_binding_size() as u64,
        max_storage_buffer_binding_size: l.max_storage_buffer_binding_size() as u64,
        min_uniform_buffer_offset_alignment: l.min_uniform_buffer_offset_alignment(),
        min_storage_buffer_offset_alignment: l.min_storage_buffer_offset_alignment(),
        max_vertex_buffers: l.max_vertex_buffers(),
        max_buffer_size: l.max_buffer_size(),
        max_vertex_attributes: l.max_vertex_attributes(),
        max_vertex_buffer_array_stride: l.max_vertex_buffer_array_stride(),
        max_inter_stage_shader_variables: l.max_inter_stage_shader_variables(),
        max_color_attachments: l.max_color_attachments(),
        max_color_attachment_bytes_per_sample: l.max_color_attachment_bytes_per_sample(),
        max_compute_workgroup_storage_size: l.max_compute_workgroup_storage_size(),
        max_compute_invocations_per_workgroup: l.max_compute_invocations_per_workgroup(),
        max_compute_workgroup_size_x: l.max_compute_workgroup_size_x(),
        max_compute_workgroup_size_y: l.max_compute_workgroup_size_y(),
        max_compute_workgroup_size_z: l.max_compute_workgroup_size_z(),
        max_compute_workgroups_per_dimension: l.max_compute_workgroups_per_dimension(),
        ..wgt::Limits::default()
    }
}

/// The limits a device is asked for, as the wit's record: the WebGPU names
/// the host knows, with the values wgpu wants.
pub fn required_limits(l: &wgt::Limits) -> w::RecordOptionGpuSize64 {
    let r = w::RecordOptionGpuSize64::new();
    let pairs: [(&str, u64); 28] = [
        ("maxTextureDimension1D", l.max_texture_dimension_1d as u64),
        ("maxTextureDimension2D", l.max_texture_dimension_2d as u64),
        ("maxTextureDimension3D", l.max_texture_dimension_3d as u64),
        ("maxTextureArrayLayers", l.max_texture_array_layers as u64),
        ("maxBindGroups", l.max_bind_groups as u64),
        ("maxBindingsPerBindGroup", l.max_bindings_per_bind_group as u64),
        ("maxDynamicUniformBuffersPerPipelineLayout", l.max_dynamic_uniform_buffers_per_pipeline_layout as u64),
        ("maxDynamicStorageBuffersPerPipelineLayout", l.max_dynamic_storage_buffers_per_pipeline_layout as u64),
        ("maxSampledTexturesPerShaderStage", l.max_sampled_textures_per_shader_stage as u64),
        ("maxSamplersPerShaderStage", l.max_samplers_per_shader_stage as u64),
        ("maxStorageBuffersPerShaderStage", l.max_storage_buffers_per_shader_stage as u64),
        ("maxStorageTexturesPerShaderStage", l.max_storage_textures_per_shader_stage as u64),
        ("maxUniformBuffersPerShaderStage", l.max_uniform_buffers_per_shader_stage as u64),
        ("maxUniformBufferBindingSize", l.max_uniform_buffer_binding_size),
        ("maxStorageBufferBindingSize", l.max_storage_buffer_binding_size),
        ("minUniformBufferOffsetAlignment", l.min_uniform_buffer_offset_alignment as u64),
        ("minStorageBufferOffsetAlignment", l.min_storage_buffer_offset_alignment as u64),
        ("maxVertexBuffers", l.max_vertex_buffers as u64),
        ("maxBufferSize", l.max_buffer_size),
        ("maxVertexAttributes", l.max_vertex_attributes as u64),
        ("maxVertexBufferArrayStride", l.max_vertex_buffer_array_stride as u64),
        ("maxInterStageShaderVariables", l.max_inter_stage_shader_variables as u64),
        ("maxColorAttachments", l.max_color_attachments as u64),
        ("maxColorAttachmentBytesPerSample", l.max_color_attachment_bytes_per_sample as u64),
        ("maxComputeWorkgroupStorageSize", l.max_compute_workgroup_storage_size as u64),
        ("maxComputeInvocationsPerWorkgroup", l.max_compute_invocations_per_workgroup as u64),
        ("maxComputeWorkgroupSizeX", l.max_compute_workgroup_size_x as u64),
        ("maxComputeWorkgroupsPerDimension", l.max_compute_workgroups_per_dimension as u64),
    ];
    for (k, v) in pairs {
        r.add(k, Some(v));
    }
    r
}

pub fn adapter_info(i: &w::GpuAdapterInfo) -> wgt::AdapterInfo {
    wgt::AdapterInfo {
        name: i.device(),
        vendor: 0,
        device: 0,
        device_type: wgt::DeviceType::Other,
        device_pci_bus_id: String::new(),
        driver: i.vendor(),
        driver_info: i.description(),
        // The closest wgpu has to "WebGPU through a host": the host's own
        // backend is not in the wit.
        backend: wgt::Backend::BrowserWebGpu,
        subgroup_min_size: i.subgroup_min_size(),
        subgroup_max_size: i.subgroup_max_size(),
        transient_saves_memory: false,
    }
}
