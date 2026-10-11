//! The dispatch traits, each object a wit resource. Single-threaded: a
//! wasm32-wasip2 module has one thread, and wgpu asks for no `Send` on
//! wasm, so `Rc` and `Cell` do.

use std::cell::Cell;
use std::fmt;
use std::future::ready;
use std::ops::Range;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use wgpu::custom::*;
use wgpu::{self as wgt, WriteOnly};

use crate::bindings::wasi::webgpu::webgpu as w;
use crate::conv;
use crate::counters;

/// A `wgpu::Instance` whose adapters are the host's.
pub fn instance() -> wgpu::Instance {
    wgpu::Instance::from_custom(Instance { gpu: w::get_gpu() })
}

macro_rules! named_debug {
    ($($t:ident),* $(,)?) => {$(
        impl fmt::Debug for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!("wasi:webgpu ", stringify!($t)))
            }
        }
    )*};
}

fn label(l: wgt::Label<'_>) -> Option<String> {
    l.map(|s| s.to_string())
}

macro_rules! custom {
    ($d:expr, $t:ty) => {
        $d.as_custom::<$t>()
            .unwrap_or_else(|| panic!("wasi:webgpu: {} is not this backend's", stringify!($t)))
    };
}

// ---------------------------------------------------------------- instance

pub struct Instance {
    gpu: w::Gpu,
}

impl InstanceInterface for Instance {
    fn new(_desc: wgt::InstanceDescriptor) -> Self {
        Instance { gpu: w::get_gpu() }
    }

    unsafe fn create_surface(
        &self,
        _target: wgt::SurfaceTargetUnsafe,
    ) -> Result<DispatchSurface, wgt::CreateSurfaceError> {
        todo!("wasi:webgpu: surfaces")
    }

    fn request_adapter(
        &self,
        options: &wgt::RequestAdapterOptions<'_, '_>,
    ) -> Pin<Box<dyn RequestAdapterFuture>> {
        let opts = w::GpuRequestAdapterOptions {
            feature_level: None,
            power_preference: Some(match options.power_preference {
                wgt::PowerPreference::LowPower => w::GpuPowerPreference::LowPower,
                _ => w::GpuPowerPreference::HighPerformance,
            }),
            force_fallback_adapter: Some(options.force_fallback_adapter),
            xr_compatible: None,
        };
        let adapter = self
            .gpu
            .request_adapter(Some(&opts))
            .expect("wasi:webgpu: the host has no adapter (was the module granted -gpu?)");
        Box::pin(ready(Ok(DispatchAdapter::custom(Adapter {
            adapter: Rc::new(adapter),
        }))))
    }

    fn poll_all_devices(&self, _force_wait: bool) -> bool {
        false
    }

    fn wgsl_language_features(&self) -> wgt::WgslLanguageFeatures {
        wgt::WgslLanguageFeatures::empty()
    }

    fn enumerate_adapters(&self, _backends: wgt::Backends) -> Pin<Box<dyn EnumerateAdapterFuture>> {
        Box::pin(ready(Vec::new()))
    }
}

// ----------------------------------------------------------------- adapter

pub struct Adapter {
    adapter: Rc<w::GpuAdapter>,
}

impl AdapterInterface for Adapter {
    fn request_device(&self, desc: &wgt::DeviceDescriptor<'_>) -> Pin<Box<dyn RequestDeviceFuture>> {
        let names: Vec<w::GpuFeatureName> = conv::FEATURES
            .iter()
            .filter(|(_, flag)| desc.required_features.contains(*flag))
            .filter_map(|(name, _)| conv::feature_name(name))
            .collect();
        let wit_desc = w::GpuDeviceDescriptor {
            required_features: Some(names),
            required_limits: Some(conv::required_limits(&desc.required_limits)),
            default_queue: None,
            label: label(desc.label),
        };
        let device = match self.adapter.request_device(Some(wit_desc)) {
            Ok(d) => d,
            // wgpu's RequestDeviceError cannot be built outside wgpu.
            Err(e) => panic!("wasi:webgpu: request-device: {:?}: {}", e.kind, e.message),
        };
        let queue = device.queue();
        let features = conv::features(&device.features());
        let limits = conv::limits(&device.limits());
        let info = conv::adapter_info(&device.adapter_info());
        let device = Device {
            device: Rc::new(device),
            features,
            limits,
            info,
        };
        let queue = Queue {
            queue,
            submissions: Cell::new(0),
        };
        Box::pin(ready(Ok((DispatchDevice::custom(device), DispatchQueue::custom(queue)))))
    }

    fn is_surface_supported(&self, _surface: &DispatchSurface) -> bool {
        false
    }

    fn features(&self) -> wgt::Features {
        conv::features(&self.adapter.features())
    }

    fn limits(&self) -> wgt::Limits {
        conv::limits(&self.adapter.limits())
    }

    fn downlevel_capabilities(&self) -> wgt::DownlevelCapabilities {
        wgt::DownlevelCapabilities::default()
    }

    fn get_info(&self) -> wgt::AdapterInfo {
        conv::adapter_info(&self.adapter.info())
    }

    fn get_texture_format_features(&self, _format: wgt::TextureFormat) -> wgt::TextureFormatFeatures {
        wgt::TextureFormatFeatures {
            allowed_usages: wgt::TextureUsages::all(),
            flags: wgt::TextureFormatFeatureFlags::empty(),
        }
    }

    fn get_presentation_timestamp(&self) -> wgt::PresentationTimestamp {
        wgt::PresentationTimestamp::INVALID_TIMESTAMP
    }

    fn cooperative_matrix_properties(&self) -> Vec<wgt::CooperativeMatrixProperties> {
        Vec::new()
    }
}

// ------------------------------------------------------------------ device

pub struct Device {
    device: Rc<w::GpuDevice>,
    features: wgt::Features,
    limits: wgt::Limits,
    info: wgt::AdapterInfo,
}

#[derive(Debug)]
struct Message(String);

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Message {}

impl DeviceInterface for Device {
    fn features(&self) -> wgt::Features {
        self.features
    }

    fn limits(&self) -> wgt::Limits {
        self.limits.clone()
    }

    fn adapter_info(&self) -> wgt::AdapterInfo {
        self.info.clone()
    }

    fn create_shader_module(
        &self,
        desc: wgt::ShaderModuleDescriptor<'_>,
        _shader_bound_checks: wgt::ShaderRuntimeChecks,
    ) -> DispatchShaderModule {
        let code = match desc.source {
            wgt::ShaderSource::Wgsl(code) => code.into_owned(),
            _ => todo!("wasi:webgpu: shader sources other than WGSL"),
        };
        let module = self.device.create_shader_module(&w::GpuShaderModuleDescriptor {
            code,
            compilation_hints: None,
            label: label(desc.label),
        });
        DispatchShaderModule::custom(ShaderModule { module })
    }

    unsafe fn create_shader_module_passthrough(
        &self,
        _desc: &wgt::ShaderModuleDescriptorPassthrough<'_>,
    ) -> DispatchShaderModule {
        todo!("wasi:webgpu: passthrough shaders")
    }

    fn create_bind_group_layout(&self, desc: &wgt::BindGroupLayoutDescriptor<'_>) -> DispatchBindGroupLayout {
        let entries: Vec<w::GpuBindGroupLayoutEntry> = desc.entries.iter().map(conv::layout_entry).collect();
        let layout = self
            .device
            .create_bind_group_layout(&w::GpuBindGroupLayoutDescriptor {
                entries,
                label: label(desc.label),
            });
        DispatchBindGroupLayout::custom(BindGroupLayout { layout })
    }

    fn create_bind_group(&self, desc: &wgt::BindGroupDescriptor<'_>) -> DispatchBindGroup {
        let layout: &BindGroupLayout = desc.layout.as_custom().expect("bind group layout");
        let entries: Vec<w::GpuBindGroupEntry<'_>> = desc
            .entries
            .iter()
            .map(|e| {
                let resource = match &e.resource {
                    wgt::BindingResource::Buffer(b) => {
                        let buffer: &Buffer = b.buffer.as_custom().expect("buffer");
                        w::GpuBindingResource::GpuBufferBinding(w::GpuBufferBinding {
                            buffer: &buffer.buffer,
                            offset: Some(b.offset),
                            size: b.size.map(|s| s.get()),
                        })
                    }
                    wgt::BindingResource::Sampler(s) => {
                        let sampler: &Sampler = s.as_custom().expect("sampler");
                        w::GpuBindingResource::GpuSampler(&sampler.sampler)
                    }
                    wgt::BindingResource::TextureView(v) => {
                        let view: &TextureView = v.as_custom().expect("texture view");
                        w::GpuBindingResource::GpuTextureView(&view.view)
                    }
                    other => todo!("wasi:webgpu: binding resource {other:?}"),
                };
                w::GpuBindGroupEntry {
                    binding: e.binding,
                    resource,
                }
            })
            .collect();
        let group = self.device.create_bind_group(&w::GpuBindGroupDescriptor {
            layout: &layout.layout,
            entries,
            label: label(desc.label),
        });
        DispatchBindGroup::custom(BindGroup { group })
    }

    fn create_pipeline_layout(&self, desc: &wgt::PipelineLayoutDescriptor<'_>) -> DispatchPipelineLayout {
        let layouts: Vec<Option<&w::GpuBindGroupLayout>> = desc
            .bind_group_layouts
            .iter()
            .map(|l| l.map(|l| &l.as_custom::<BindGroupLayout>().expect("bind group layout").layout))
            .collect();
        let layout = self.device.create_pipeline_layout(&w::GpuPipelineLayoutDescriptor {
            bind_group_layouts: layouts,
            label: label(desc.label),
        });
        DispatchPipelineLayout::custom(PipelineLayout { layout })
    }

    fn create_render_pipeline(&self, desc: &wgt::RenderPipelineDescriptor<'_>) -> DispatchRenderPipeline {
        let layout = match desc.layout {
            Some(l) => w::GpuLayoutMode::Specific(&l.as_custom::<PipelineLayout>().expect("pipeline layout").layout),
            None => w::GpuLayoutMode::Auto,
        };
        let vertex_module: &ShaderModule = desc.vertex.module.as_custom().expect("shader module");
        let vertex = w::GpuVertexState {
            buffers: Some(
                desc.vertex
                    .buffers
                    .iter()
                    .map(|b| Some(conv::vertex_buffer_layout(b)))
                    .collect(),
            ),
            module: &vertex_module.module,
            entry_point: desc.vertex.entry_point.map(|s| s.to_string()),
            constants: None,
        };
        let fragment = desc.fragment.as_ref().map(|f| {
            let module: &ShaderModule = f.module.as_custom().expect("shader module");
            w::GpuFragmentState {
                targets: f.targets.iter().map(|t| t.as_ref().map(conv::color_target)).collect(),
                module: &module.module,
                entry_point: f.entry_point.map(|s| s.to_string()),
                constants: None,
            }
        });
        let p = desc.primitive;
        let primitive = w::GpuPrimitiveState {
            topology: Some(conv::topology(p.topology)),
            strip_index_format: p.strip_index_format.map(conv::index_format),
            front_face: Some(conv::front_face(p.front_face)),
            cull_mode: Some(conv::cull_mode(p.cull_mode)),
            unclipped_depth: Some(p.unclipped_depth),
        };
        let multisample = w::GpuMultisampleState {
            count: Some(desc.multisample.count),
            mask: Some(desc.multisample.mask as u32),
            alpha_to_coverage_enabled: Some(desc.multisample.alpha_to_coverage_enabled),
        };
        let pipeline = self.device.create_render_pipeline(w::GpuRenderPipelineDescriptor {
            vertex,
            primitive: Some(primitive),
            depth_stencil: desc.depth_stencil.as_ref().map(conv::depth_stencil),
            multisample: Some(multisample),
            fragment,
            layout,
            label: label(desc.label),
        });
        DispatchRenderPipeline::custom(RenderPipeline { pipeline })
    }

    fn create_mesh_pipeline(&self, _desc: &wgt::MeshPipelineDescriptor<'_>) -> DispatchRenderPipeline {
        todo!("wasi:webgpu: mesh pipelines")
    }

    fn create_compute_pipeline(&self, desc: &wgt::ComputePipelineDescriptor<'_>) -> DispatchComputePipeline {
        let layout = match desc.layout {
            Some(l) => w::GpuLayoutMode::Specific(&l.as_custom::<PipelineLayout>().expect("pipeline layout").layout),
            None => w::GpuLayoutMode::Auto,
        };
        let module: &ShaderModule = desc.module.as_custom().expect("shader module");
        let pipeline = self.device.create_compute_pipeline(w::GpuComputePipelineDescriptor {
            compute: w::GpuProgrammableStage {
                module: &module.module,
                entry_point: desc.entry_point.map(|s| s.to_string()),
                constants: None,
            },
            layout,
            label: label(desc.label),
        });
        DispatchComputePipeline::custom(ComputePipeline { pipeline })
    }

    unsafe fn create_pipeline_cache(&self, _desc: &wgt::PipelineCacheDescriptor<'_>) -> DispatchPipelineCache {
        todo!("wasi:webgpu: pipeline caches")
    }

    fn create_buffer(&self, desc: &wgt::BufferDescriptor<'_>) -> DispatchBuffer {
        let buffer = self.device.create_buffer(&w::GpuBufferDescriptor {
            size: desc.size,
            usage: desc.usage.bits(),
            mapped_at_creation: Some(desc.mapped_at_creation),
            label: label(desc.label),
        });
        DispatchBuffer::custom(Buffer {
            buffer: Rc::new(buffer),
            size: desc.size,
            mapped_for_write: Cell::new(desc.mapped_at_creation),
        })
    }

    fn create_texture(&self, desc: &wgt::TextureDescriptor<'_>) -> DispatchTexture {
        let texture = self.device.create_texture(&w::GpuTextureDescriptor {
            size: conv::extent(desc.size),
            mip_level_count: Some(desc.mip_level_count),
            sample_count: Some(desc.sample_count),
            dimension: Some(conv::texture_dimension(desc.dimension)),
            format: conv::texture_format(desc.format),
            usage: desc.usage.bits(),
            view_formats: Some(desc.view_formats.iter().map(|f| conv::texture_format(*f)).collect()),
            label: label(desc.label),
        });
        DispatchTexture::custom(Texture {
            texture: Rc::new(texture),
        })
    }

    fn create_external_texture(
        &self,
        _desc: &wgt::ExternalTextureDescriptor<'_>,
        _planes: &[&wgt::TextureView],
    ) -> DispatchExternalTexture {
        todo!("wasi:webgpu: external textures")
    }

    fn create_blas(
        &self,
        _desc: &wgt::CreateBlasDescriptor<'_>,
        _sizes: wgt::BlasGeometrySizeDescriptors,
    ) -> (Option<u64>, DispatchBlas) {
        todo!("wasi:webgpu: acceleration structures")
    }

    fn create_tlas(&self, _desc: &wgt::CreateTlasDescriptor<'_>) -> DispatchTlas {
        todo!("wasi:webgpu: acceleration structures")
    }

    fn create_sampler(&self, desc: &wgt::SamplerDescriptor<'_>) -> DispatchSampler {
        let sampler = self.device.create_sampler(Some(&w::GpuSamplerDescriptor {
            address_mode_u: Some(conv::address_mode(desc.address_mode_u)),
            address_mode_v: Some(conv::address_mode(desc.address_mode_v)),
            address_mode_w: Some(conv::address_mode(desc.address_mode_w)),
            mag_filter: Some(conv::filter(desc.mag_filter)),
            min_filter: Some(conv::filter(desc.min_filter)),
            mipmap_filter: Some(conv::mipmap_filter(desc.mipmap_filter)),
            lod_min_clamp: Some(desc.lod_min_clamp),
            lod_max_clamp: Some(desc.lod_max_clamp),
            compare: desc.compare.map(conv::compare),
            max_anisotropy: Some(desc.anisotropy_clamp),
            label: label(desc.label),
        }));
        DispatchSampler::custom(Sampler { sampler })
    }

    fn create_query_set(&self, desc: &wgt::QuerySetDescriptor<'_>) -> DispatchQuerySet {
        let set = self
            .device
            .create_query_set(&w::GpuQuerySetDescriptor {
                type_: conv::query_type(desc.ty),
                count: desc.count,
                label: label(desc.label),
            })
            .unwrap_or_else(|e| panic!("wasi:webgpu: create-query-set: {}", e.message));
        DispatchQuerySet::custom(QuerySet { set })
    }

    fn create_command_encoder(&self, desc: &wgt::CommandEncoderDescriptor<'_>) -> DispatchCommandEncoder {
        let encoder = self
            .device
            .create_command_encoder(Some(&w::GpuCommandEncoderDescriptor {
                label: label(desc.label),
            }));
        DispatchCommandEncoder::custom(CommandEncoder { encoder })
    }

    fn create_render_bundle_encoder(
        &self,
        _desc: &wgt::RenderBundleEncoderDescriptor<'_>,
    ) -> DispatchRenderBundleEncoder {
        todo!("wasi:webgpu: render bundles")
    }

    fn set_device_lost_callback(&self, _device_lost_callback: BoxDeviceLostCallback) {}

    fn on_uncaptured_error(&self, _handler: Arc<dyn wgt::UncapturedErrorHandler>) {
        // The 0.0.1 wit signals an uncaptured error through a pollable and
        // the host prints it; nothing to hand the handler.
    }

    fn push_error_scope(&self, filter: wgt::ErrorFilter) -> u32 {
        self.device.push_error_scope(match filter {
            wgt::ErrorFilter::Validation => w::GpuErrorFilter::Validation,
            wgt::ErrorFilter::OutOfMemory => w::GpuErrorFilter::OutOfMemory,
            wgt::ErrorFilter::Internal => w::GpuErrorFilter::Internal,
        });
        0
    }

    fn pop_error_scope(&self, _index: u32) -> Pin<Box<dyn PopErrorScopeFuture>> {
        let error = match self.device.pop_error_scope() {
            Ok(Some(e)) => {
                let message = e.message();
                let source = Box::new(Message(message.clone()));
                Some(match e.kind() {
                    w::GpuErrorKind::OutOfMemoryError => wgt::Error::OutOfMemory { source },
                    w::GpuErrorKind::ValidationError => wgt::Error::Validation {
                        source,
                        description: message,
                    },
                    w::GpuErrorKind::InternalError => wgt::Error::Internal {
                        source,
                        description: message,
                    },
                })
            }
            _ => None,
        };
        Box::pin(ready(error))
    }

    unsafe fn start_graphics_debugger_capture(&self) {}

    unsafe fn stop_graphics_debugger_capture(&self) {}

    fn poll(&self, _poll_type: wgpu::wgt::PollType<u64>) -> Result<wgt::PollStatus, wgt::PollError> {
        // map-async blocks on this host, and submit returns when the host
        // has queued the work; there is nothing to drive.
        Ok(wgt::PollStatus::QueueEmpty)
    }

    fn get_internal_counters(&self) -> wgt::InternalCounters {
        wgt::InternalCounters::default()
    }

    fn generate_allocator_report(&self) -> Option<wgt::AllocatorReport> {
        None
    }

    fn destroy(&self) {
        self.device.destroy();
    }
}

// ------------------------------------------------------------------- queue

pub struct Queue {
    queue: w::GpuQueue,
    submissions: Cell<u64>,
}

impl QueueInterface for Queue {
    fn write_buffer(&self, buffer: &DispatchBuffer, offset: wgt::BufferAddress, data: &[u8]) {
        let buffer: &Buffer = custom!(buffer, Buffer);
        counters::up(data.len());
        self.queue
            .write_buffer_with_copy(&buffer.buffer, offset, data, None, None)
            .unwrap_or_else(|e| panic!("wasi:webgpu: write-buffer: {}", e.message));
    }

    fn create_staging_buffer(&self, size: wgt::BufferSize) -> Option<DispatchQueueWriteBuffer> {
        Some(DispatchQueueWriteBuffer::custom(StagingBuffer {
            data: vec![0u8; size.get() as usize],
        }))
    }

    fn validate_write_buffer(
        &self,
        _buffer: &DispatchBuffer,
        _offset: wgt::BufferAddress,
        _size: wgt::BufferSize,
    ) -> Option<()> {
        Some(())
    }

    fn write_staging_buffer(
        &self,
        buffer: &DispatchBuffer,
        offset: wgt::BufferAddress,
        staging_buffer: &DispatchQueueWriteBuffer,
    ) {
        let buffer: &Buffer = custom!(buffer, Buffer);
        let staging: &StagingBuffer = custom!(staging_buffer, StagingBuffer);
        counters::up(staging.data.len());
        self.queue
            .write_buffer_with_copy(&buffer.buffer, offset, &staging.data, None, None)
            .unwrap_or_else(|e| panic!("wasi:webgpu: write-buffer: {}", e.message));
    }

    fn write_texture(
        &self,
        texture: wgt::TexelCopyTextureInfo<'_>,
        data: &[u8],
        data_layout: wgt::TexelCopyBufferLayout,
        size: wgt::Extent3d,
    ) {
        let t: &Texture = texture.texture.as_custom().expect("texture");
        counters::up(data.len());
        self.queue.write_texture_with_copy(
            &w::GpuTexelCopyTextureInfo {
                texture: &t.texture,
                mip_level: Some(texture.mip_level),
                origin: Some(conv::origin(texture.origin)),
                aspect: Some(conv::aspect(texture.aspect)),
            },
            data,
            conv::buffer_layout(data_layout),
            conv::extent(size),
        );
    }

    fn submit(&self, command_buffers: &mut dyn Iterator<Item = DispatchCommandBuffer>) -> u64 {
        let buffers: Vec<DispatchCommandBuffer> = command_buffers.collect();
        let refs: Vec<&w::GpuCommandBuffer> = buffers
            .iter()
            .map(|b| &custom!(b, CommandBuffer).buffer)
            .collect();
        self.queue.submit(&refs);
        let n = self.submissions.get() + 1;
        self.submissions.set(n);
        n
    }

    fn get_timestamp_period(&self) -> f32 {
        self.queue.get_timestamp_period()
    }

    fn on_submitted_work_done(&self, callback: BoxSubmittedWorkDoneCallback) {
        self.queue.on_submitted_work_done();
        callback();
    }

    fn compact_blas(&self, _blas: &DispatchBlas) -> (Option<u64>, DispatchBlas) {
        todo!("wasi:webgpu: acceleration structures")
    }
}

/// What `Queue::write_buffer_with` fills: a Vec the guest writes, handed
/// over whole when the queue is told to write it.
pub struct StagingBuffer {
    data: Vec<u8>,
}

impl QueueWriteBufferInterface for StagingBuffer {
    fn len(&self) -> usize {
        self.data.len()
    }

    unsafe fn write_slice(&mut self) -> WriteOnly<'_, [u8]> {
        // SAFETY: the pointer is to this buffer's own live Vec.
        unsafe { WriteOnly::new(std::ptr::NonNull::from(&mut self.data[..])) }
    }
}

// ----------------------------------------------------------------- buffers

pub struct Buffer {
    buffer: Rc<w::GpuBuffer>,
    size: u64,
    /// Mapped for writing (at creation or by `map-async` with WRITE): the
    /// mapped range is a copy the guest fills, handed over when it drops.
    mapped_for_write: Cell<bool>,
}

impl BufferInterface for Buffer {
    fn map_async(&self, mode: wgt::MapMode, range: Range<wgt::BufferAddress>, callback: BufferMapCallback) {
        let (flags, write) = match mode {
            wgt::MapMode::Read => (1u32, false),
            wgt::MapMode::Write => (2u32, true),
        };
        // The host maps synchronously: the callback runs before this returns.
        let result = self
            .buffer
            .map_async(flags, Some(range.start), Some(range.end - range.start));
        match result {
            Ok(()) => {
                self.mapped_for_write.set(write);
                callback(Ok(()));
            }
            Err(e) => {
                eprintln!("wasi:webgpu: map-async: {}", e.message);
                callback(Err(wgt::BufferAsyncError));
            }
        }
    }

    fn get_mapped_range(&self, sub_range: Range<wgt::BufferAddress>) -> DispatchBufferMappedRange {
        let len = (sub_range.end - sub_range.start) as usize;
        let range = if self.mapped_for_write.get() {
            MappedRange {
                data: vec![0u8; len],
                write_back: Some((Rc::clone(&self.buffer), sub_range.start)),
            }
        } else {
            let data = self
                .buffer
                .get_mapped_range_get_with_copy(Some(sub_range.start), Some(len as u64))
                .unwrap_or_else(|e| panic!("wasi:webgpu: get-mapped-range: {}", e.message));
            counters::down(data.len());
            MappedRange {
                data,
                write_back: None,
            }
        };
        DispatchBufferMappedRange::custom(range)
    }

    fn unmap(&self) {
        self.mapped_for_write.set(false);
        if let Err(e) = self.buffer.unmap() {
            panic!("wasi:webgpu: unmap: {}", e.message);
        }
    }

    fn destroy(&self) {
        self.buffer.destroy();
    }
}

pub struct MappedRange {
    data: Vec<u8>,
    write_back: Option<(Rc<w::GpuBuffer>, u64)>,
}

impl Drop for MappedRange {
    fn drop(&mut self) {
        if let Some((buffer, offset)) = self.write_back.take() {
            counters::up(self.data.len());
            buffer
                .get_mapped_range_set_with_copy(&self.data, Some(offset), Some(self.data.len() as u64))
                .unwrap_or_else(|e| panic!("wasi:webgpu: get-mapped-range (write): {}", e.message));
        }
    }
}

impl BufferMappedRangeInterface for MappedRange {
    fn len(&self) -> usize {
        self.data.len()
    }

    unsafe fn read_slice(&self) -> &[u8] {
        &self.data
    }

    unsafe fn write_slice(&mut self) -> WriteOnly<'_, [u8]> {
        // SAFETY: the pointer is to this range's own live Vec.
        unsafe { WriteOnly::new(std::ptr::NonNull::from(&mut self.data[..])) }
    }
}

// --------------------------------------------------- textures and the rest

pub struct Texture {
    texture: Rc<w::GpuTexture>,
}

impl TextureInterface for Texture {
    fn create_view(&self, desc: &wgt::TextureViewDescriptor<'_>) -> DispatchTextureView {
        let view = self.texture.create_view(Some(&w::GpuTextureViewDescriptor {
            format: desc.format.map(conv::texture_format),
            dimension: desc.dimension.map(conv::view_dimension),
            usage: desc.usage.map(|u| u.bits()),
            aspect: Some(conv::aspect(desc.aspect)),
            base_mip_level: Some(desc.base_mip_level),
            mip_level_count: desc.mip_level_count,
            base_array_layer: Some(desc.base_array_layer),
            array_layer_count: desc.array_layer_count,
            label: label(desc.label),
        }));
        DispatchTextureView::custom(TextureView { view })
    }

    fn destroy(&self) {
        self.texture.destroy();
    }
}

pub struct TextureView {
    view: w::GpuTextureView,
}
impl TextureViewInterface for TextureView {}

pub struct Sampler {
    sampler: w::GpuSampler,
}
impl SamplerInterface for Sampler {}

pub struct BindGroupLayout {
    layout: w::GpuBindGroupLayout,
}
impl BindGroupLayoutInterface for BindGroupLayout {}

pub struct BindGroup {
    group: w::GpuBindGroup,
}
impl BindGroupInterface for BindGroup {}

pub struct PipelineLayout {
    layout: w::GpuPipelineLayout,
}
impl PipelineLayoutInterface for PipelineLayout {}

pub struct ShaderModule {
    module: w::GpuShaderModule,
}

impl ShaderModuleInterface for ShaderModule {
    fn get_compilation_info(&self) -> Pin<Box<dyn ShaderCompilationInfoFuture>> {
        let messages = self
            .module
            .get_compilation_info()
            .messages()
            .iter()
            .map(|m| wgt::CompilationMessage {
                message: m.message(),
                message_type: match m.type_() {
                    w::GpuCompilationMessageType::Error => wgt::CompilationMessageType::Error,
                    w::GpuCompilationMessageType::Warning => wgt::CompilationMessageType::Warning,
                    w::GpuCompilationMessageType::Info => wgt::CompilationMessageType::Info,
                },
                location: Some(wgt::SourceLocation {
                    line_number: m.line_num() as u32,
                    line_position: m.line_pos() as u32,
                    offset: m.offset() as u32,
                    length: m.length() as u32,
                }),
            })
            .collect();
        Box::pin(ready(wgt::CompilationInfo { messages }))
    }
}

pub struct RenderPipeline {
    pipeline: w::GpuRenderPipeline,
}

impl RenderPipelineInterface for RenderPipeline {
    fn get_bind_group_layout(&self, index: u32) -> DispatchBindGroupLayout {
        DispatchBindGroupLayout::custom(BindGroupLayout {
            layout: self.pipeline.get_bind_group_layout(index),
        })
    }
}

pub struct ComputePipeline {
    pipeline: w::GpuComputePipeline,
}

impl ComputePipelineInterface for ComputePipeline {
    fn get_bind_group_layout(&self, index: u32) -> DispatchBindGroupLayout {
        DispatchBindGroupLayout::custom(BindGroupLayout {
            layout: self.pipeline.get_bind_group_layout(index),
        })
    }
}

pub struct QuerySet {
    set: w::GpuQuerySet,
}
impl QuerySetInterface for QuerySet {}

pub struct CommandBuffer {
    buffer: w::GpuCommandBuffer,
}
impl CommandBufferInterface for CommandBuffer {}

// ---------------------------------------------------------------- encoders

pub struct CommandEncoder {
    encoder: w::GpuCommandEncoder,
}

fn texture_info<'a>(t: &'a wgt::TexelCopyTextureInfo<'_>) -> w::GpuTexelCopyTextureInfo<'a> {
    let texture: &Texture = t.texture.as_custom().expect("texture");
    w::GpuTexelCopyTextureInfo {
        texture: &texture.texture,
        mip_level: Some(t.mip_level),
        origin: Some(conv::origin(t.origin)),
        aspect: Some(conv::aspect(t.aspect)),
    }
}

fn buffer_info<'a>(b: &'a wgt::TexelCopyBufferInfo<'_>) -> w::GpuTexelCopyBufferInfo<'a> {
    let buffer: &Buffer = b.buffer.as_custom().expect("buffer");
    w::GpuTexelCopyBufferInfo {
        buffer: &buffer.buffer,
        offset: Some(b.layout.offset),
        bytes_per_row: b.layout.bytes_per_row,
        rows_per_image: b.layout.rows_per_image,
    }
}

impl CommandEncoderInterface for CommandEncoder {
    fn copy_buffer_to_buffer(
        &self,
        source: &DispatchBuffer,
        source_offset: wgt::BufferAddress,
        destination: &DispatchBuffer,
        destination_offset: wgt::BufferAddress,
        copy_size: Option<wgt::BufferAddress>,
    ) {
        let src: &Buffer = custom!(source, Buffer);
        let dst: &Buffer = custom!(destination, Buffer);
        let size = copy_size.unwrap_or(src.size - source_offset);
        self.encoder
            .copy_buffer_to_buffer(&src.buffer, source_offset, &dst.buffer, destination_offset, size);
    }

    fn copy_buffer_to_texture(
        &self,
        source: wgt::TexelCopyBufferInfo<'_>,
        destination: wgt::TexelCopyTextureInfo<'_>,
        copy_size: wgt::Extent3d,
    ) {
        self.encoder.copy_buffer_to_texture(
            &buffer_info(&source),
            &texture_info(&destination),
            conv::extent(copy_size),
        );
    }

    fn copy_texture_to_buffer(
        &self,
        source: wgt::TexelCopyTextureInfo<'_>,
        destination: wgt::TexelCopyBufferInfo<'_>,
        copy_size: wgt::Extent3d,
    ) {
        self.encoder.copy_texture_to_buffer(
            &texture_info(&source),
            &buffer_info(&destination),
            conv::extent(copy_size),
        );
    }

    fn copy_texture_to_texture(
        &self,
        source: wgt::TexelCopyTextureInfo<'_>,
        destination: wgt::TexelCopyTextureInfo<'_>,
        copy_size: wgt::Extent3d,
    ) {
        self.encoder.copy_texture_to_texture(
            &texture_info(&source),
            &texture_info(&destination),
            conv::extent(copy_size),
        );
    }

    fn begin_compute_pass(&self, desc: &wgt::ComputePassDescriptor<'_>) -> DispatchComputePass {
        let timestamp_writes = desc.timestamp_writes.as_ref().map(|t| w::GpuComputePassTimestampWrites {
            query_set: &t.query_set.as_custom::<QuerySet>().expect("query set").set,
            beginning_of_pass_write_index: t.beginning_of_pass_write_index,
            end_of_pass_write_index: t.end_of_pass_write_index,
        });
        let pass = self.encoder.begin_compute_pass(Some(&w::GpuComputePassDescriptor {
            timestamp_writes,
            label: label(desc.label),
        }));
        DispatchComputePass::custom(ComputePass { pass, ended: false })
    }

    fn begin_render_pass(&self, desc: &wgt::RenderPassDescriptor<'_>) -> DispatchRenderPass {
        let color_attachments: Vec<Option<w::GpuRenderPassColorAttachment<'_>>> = desc
            .color_attachments
            .iter()
            .map(|a| {
                a.as_ref().map(|a| w::GpuRenderPassColorAttachment {
                    view: &a.view.as_custom::<TextureView>().expect("texture view").view,
                    depth_slice: a.depth_slice,
                    resolve_target: a
                        .resolve_target
                        .map(|v| &v.as_custom::<TextureView>().expect("texture view").view),
                    clear_value: match a.ops.load {
                        wgt::LoadOp::Clear(c) => Some(conv::color(c)),
                        _ => None,
                    },
                    load_op: conv::load_op(&a.ops.load),
                    store_op: conv::store_op(a.ops.store),
                })
            })
            .collect();
        let depth_stencil_attachment = desc.depth_stencil_attachment.as_ref().map(|d| {
            w::GpuRenderPassDepthStencilAttachment {
                view: &d.view.as_custom::<TextureView>().expect("texture view").view,
                depth_clear_value: d.depth_ops.as_ref().and_then(|o| match o.load {
                    wgt::LoadOp::Clear(v) => Some(v),
                    _ => None,
                }),
                depth_load_op: d.depth_ops.as_ref().map(|o| conv::load_op(&o.load)),
                depth_store_op: d.depth_ops.as_ref().map(|o| conv::store_op(o.store)),
                depth_read_only: Some(d.depth_ops.is_none()),
                stencil_clear_value: d.stencil_ops.as_ref().and_then(|o| match o.load {
                    wgt::LoadOp::Clear(v) => Some(v),
                    _ => None,
                }),
                stencil_load_op: d.stencil_ops.as_ref().map(|o| conv::load_op(&o.load)),
                stencil_store_op: d.stencil_ops.as_ref().map(|o| conv::store_op(o.store)),
                stencil_read_only: Some(d.stencil_ops.is_none()),
            }
        });
        let timestamp_writes = desc.timestamp_writes.as_ref().map(|t| w::GpuRenderPassTimestampWrites {
            query_set: &t.query_set.as_custom::<QuerySet>().expect("query set").set,
            beginning_of_pass_write_index: t.beginning_of_pass_write_index,
            end_of_pass_write_index: t.end_of_pass_write_index,
        });
        let occlusion_query_set = desc
            .occlusion_query_set
            .map(|q| &q.as_custom::<QuerySet>().expect("query set").set);
        let pass = self.encoder.begin_render_pass(&w::GpuRenderPassDescriptor {
            color_attachments,
            depth_stencil_attachment,
            occlusion_query_set,
            timestamp_writes,
            max_draw_count: None,
            label: label(desc.label),
        });
        DispatchRenderPass::custom(RenderPass { pass, ended: false })
    }

    fn finish(&mut self) -> DispatchCommandBuffer {
        DispatchCommandBuffer::custom(CommandBuffer {
            buffer: self.encoder.finish(None),
        })
    }

    fn clear_texture(&self, _texture: &DispatchTexture, _subresource_range: &wgt::ImageSubresourceRange) {
        todo!("wasi:webgpu 0.0.1 has no clear-texture")
    }

    fn clear_buffer(&self, buffer: &DispatchBuffer, offset: wgt::BufferAddress, size: Option<wgt::BufferAddress>) {
        let b: &Buffer = custom!(buffer, Buffer);
        self.encoder.clear_buffer(&b.buffer, Some(offset), size);
    }

    fn insert_debug_marker(&self, label: &str) {
        self.encoder.insert_debug_marker(label);
    }

    fn push_debug_group(&self, label: &str) {
        self.encoder.push_debug_group(label);
    }

    fn pop_debug_group(&self) {
        self.encoder.pop_debug_group();
    }

    fn write_timestamp(&self, query_set: &DispatchQuerySet, query_index: u32) {
        let q: &QuerySet = custom!(query_set, QuerySet);
        self.encoder.write_timestamp(&q.set, query_index);
    }

    fn resolve_query_set(
        &self,
        query_set: &DispatchQuerySet,
        first_query: u32,
        query_count: u32,
        destination: &DispatchBuffer,
        destination_offset: wgt::BufferAddress,
    ) {
        let q: &QuerySet = custom!(query_set, QuerySet);
        let b: &Buffer = custom!(destination, Buffer);
        self.encoder
            .resolve_query_set(&q.set, first_query, query_count, &b.buffer, destination_offset);
    }

    fn mark_acceleration_structures_built<'a>(
        &self,
        _blas: &mut dyn Iterator<Item = &'a wgt::Blas>,
        _tlas: &mut dyn Iterator<Item = &'a wgt::Tlas>,
    ) {
        todo!("wasi:webgpu: acceleration structures")
    }

    fn build_acceleration_structures<'a>(
        &self,
        _blas: &mut dyn Iterator<Item = &'a wgt::BlasBuildEntry<'a>>,
        _tlas: &mut dyn Iterator<Item = &'a wgt::Tlas>,
    ) {
        todo!("wasi:webgpu: acceleration structures")
    }

    fn transition_resources<'a>(
        &mut self,
        _buffer_transitions: &mut dyn Iterator<Item = wgt::BufferTransition<&'a DispatchBuffer>>,
        _texture_transitions: &mut dyn Iterator<Item = wgt::TextureTransition<&'a DispatchTexture>>,
    ) {
        // The host tracks state itself.
    }
}

pub struct ComputePass {
    pass: w::GpuComputePassEncoder,
    ended: bool,
}

impl Drop for ComputePass {
    fn drop(&mut self) {
        if !self.ended {
            self.pass.end();
        }
    }
}

impl ComputePassInterface for ComputePass {
    fn set_pipeline(&mut self, pipeline: &DispatchComputePipeline) {
        let p: &ComputePipeline = custom!(pipeline, ComputePipeline);
        self.pass.set_pipeline(&p.pipeline);
    }

    fn set_bind_group(&mut self, index: u32, bind_group: Option<&DispatchBindGroup>, offsets: &[wgt::DynamicOffset]) {
        let group = bind_group.map(|g| &custom!(g, BindGroup).group);
        let offsets = (!offsets.is_empty()).then_some(offsets);
        self.pass
            .set_bind_group(index, group, offsets, None, None)
            .unwrap_or_else(|e| panic!("wasi:webgpu: set-bind-group: {}", e.message));
    }

    fn set_immediates(&mut self, _offset: u32, _data: &[u8]) {
        todo!("wasi:webgpu: immediates")
    }

    fn insert_debug_marker(&mut self, label: &str) {
        self.pass.insert_debug_marker(label);
    }

    fn push_debug_group(&mut self, group_label: &str) {
        self.pass.push_debug_group(group_label);
    }

    fn pop_debug_group(&mut self) {
        self.pass.pop_debug_group();
    }

    fn write_timestamp(&mut self, _query_set: &DispatchQuerySet, _query_index: u32) {
        todo!("wasi:webgpu: timestamps inside passes")
    }

    fn begin_pipeline_statistics_query(&mut self, _query_set: &DispatchQuerySet, _query_index: u32) {
        todo!("wasi:webgpu: pipeline statistics")
    }

    fn end_pipeline_statistics_query(&mut self) {
        todo!("wasi:webgpu: pipeline statistics")
    }

    fn dispatch_workgroups(&mut self, x: u32, y: u32, z: u32) {
        self.pass.dispatch_workgroups(x, Some(y), Some(z));
    }

    fn dispatch_workgroups_indirect(&mut self, indirect_buffer: &DispatchBuffer, indirect_offset: wgt::BufferAddress) {
        let b: &Buffer = custom!(indirect_buffer, Buffer);
        self.pass.dispatch_workgroups_indirect(&b.buffer, indirect_offset);
    }
}

pub struct RenderPass {
    pass: w::GpuRenderPassEncoder,
    ended: bool,
}

impl Drop for RenderPass {
    fn drop(&mut self) {
        if !self.ended {
            self.pass.end();
        }
    }
}

impl RenderPassInterface for RenderPass {
    fn set_pipeline(&mut self, pipeline: &DispatchRenderPipeline) {
        let p: &RenderPipeline = custom!(pipeline, RenderPipeline);
        self.pass.set_pipeline(&p.pipeline);
    }

    fn set_bind_group(&mut self, index: u32, bind_group: Option<&DispatchBindGroup>, offsets: &[wgt::DynamicOffset]) {
        let group = bind_group.map(|g| &custom!(g, BindGroup).group);
        let offsets = (!offsets.is_empty()).then_some(offsets);
        self.pass
            .set_bind_group(index, group, offsets, None, None)
            .unwrap_or_else(|e| panic!("wasi:webgpu: set-bind-group: {}", e.message));
    }

    fn set_index_buffer(
        &mut self,
        buffer: &DispatchBuffer,
        index_format: wgt::IndexFormat,
        offset: wgt::BufferAddress,
        size: Option<wgt::BufferSize>,
    ) {
        let b: &Buffer = custom!(buffer, Buffer);
        self.pass
            .set_index_buffer(&b.buffer, conv::index_format(index_format), Some(offset), size.map(|s| s.get()));
    }

    fn set_vertex_buffer(&mut self, slot: u32, buffer: &DispatchBuffer, offset: wgt::BufferAddress, size: Option<wgt::BufferSize>) {
        let b: &Buffer = custom!(buffer, Buffer);
        self.pass
            .set_vertex_buffer(slot, Some(&b.buffer), Some(offset), size.map(|s| s.get()));
    }

    fn set_immediates(&mut self, _offset: u32, _data: &[u8]) {
        todo!("wasi:webgpu: immediates")
    }

    fn set_blend_constant(&mut self, color: wgt::Color) {
        self.pass.set_blend_constant(conv::color(color));
    }

    fn set_scissor_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        self.pass.set_scissor_rect(x, y, width, height);
    }

    fn set_viewport(&mut self, x: f32, y: f32, width: f32, height: f32, min_depth: f32, max_depth: f32) {
        self.pass.set_viewport(x, y, width, height, min_depth, max_depth);
    }

    fn set_stencil_reference(&mut self, reference: u32) {
        self.pass.set_stencil_reference(reference);
    }

    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.pass.draw(
            vertices.end - vertices.start,
            Some(instances.end - instances.start),
            Some(vertices.start),
            Some(instances.start),
        );
    }

    fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        self.pass.draw_indexed(
            indices.end - indices.start,
            Some(instances.end - instances.start),
            Some(indices.start),
            Some(base_vertex),
            Some(instances.start),
        );
    }

    fn draw_mesh_tasks(&mut self, _x: u32, _y: u32, _z: u32) {
        todo!("wasi:webgpu: mesh shaders")
    }

    fn draw_indirect(&mut self, indirect_buffer: &DispatchBuffer, indirect_offset: wgt::BufferAddress) {
        let b: &Buffer = custom!(indirect_buffer, Buffer);
        self.pass.draw_indirect(&b.buffer, indirect_offset);
    }

    fn draw_indexed_indirect(&mut self, indirect_buffer: &DispatchBuffer, indirect_offset: wgt::BufferAddress) {
        let b: &Buffer = custom!(indirect_buffer, Buffer);
        self.pass.draw_indexed_indirect(&b.buffer, indirect_offset);
    }

    fn draw_mesh_tasks_indirect(&mut self, _indirect_buffer: &DispatchBuffer, _indirect_offset: wgt::BufferAddress) {
        todo!("wasi:webgpu: mesh shaders")
    }

    fn multi_draw_indirect(&mut self, _indirect_buffer: &DispatchBuffer, _indirect_offset: wgt::BufferAddress, _count: u32) {
        todo!("wasi:webgpu: multi-draw")
    }

    fn multi_draw_indexed_indirect(&mut self, _indirect_buffer: &DispatchBuffer, _indirect_offset: wgt::BufferAddress, _count: u32) {
        todo!("wasi:webgpu: multi-draw")
    }

    fn multi_draw_indirect_count(
        &mut self,
        _indirect_buffer: &DispatchBuffer,
        _indirect_offset: wgt::BufferAddress,
        _count_buffer: &DispatchBuffer,
        _count_buffer_offset: wgt::BufferAddress,
        _max_count: u32,
    ) {
        todo!("wasi:webgpu: multi-draw")
    }

    fn multi_draw_mesh_tasks_indirect(&mut self, _indirect_buffer: &DispatchBuffer, _indirect_offset: wgt::BufferAddress, _count: u32) {
        todo!("wasi:webgpu: mesh shaders")
    }

    fn multi_draw_indexed_indirect_count(
        &mut self,
        _indirect_buffer: &DispatchBuffer,
        _indirect_offset: wgt::BufferAddress,
        _count_buffer: &DispatchBuffer,
        _count_buffer_offset: wgt::BufferAddress,
        _max_count: u32,
    ) {
        todo!("wasi:webgpu: multi-draw")
    }

    fn multi_draw_mesh_tasks_indirect_count(
        &mut self,
        _indirect_buffer: &DispatchBuffer,
        _indirect_offset: wgt::BufferAddress,
        _count_buffer: &DispatchBuffer,
        _count_buffer_offset: wgt::BufferAddress,
        _max_count: u32,
    ) {
        todo!("wasi:webgpu: mesh shaders")
    }

    fn insert_debug_marker(&mut self, label: &str) {
        self.pass.insert_debug_marker(label);
    }

    fn push_debug_group(&mut self, group_label: &str) {
        self.pass.push_debug_group(group_label);
    }

    fn pop_debug_group(&mut self) {
        self.pass.pop_debug_group();
    }

    fn write_timestamp(&mut self, _query_set: &DispatchQuerySet, _query_index: u32) {
        todo!("wasi:webgpu: timestamps inside passes")
    }

    fn begin_occlusion_query(&mut self, query_index: u32) {
        self.pass.begin_occlusion_query(query_index);
    }

    fn end_occlusion_query(&mut self) {
        self.pass.end_occlusion_query();
    }

    fn begin_pipeline_statistics_query(&mut self, _query_set: &DispatchQuerySet, _query_index: u32) {
        todo!("wasi:webgpu: pipeline statistics")
    }

    fn end_pipeline_statistics_query(&mut self) {
        todo!("wasi:webgpu: pipeline statistics")
    }

    fn execute_bundles(&mut self, _render_bundles: &mut dyn Iterator<Item = &DispatchRenderBundle>) {
        todo!("wasi:webgpu: render bundles")
    }
}

named_debug!(
    Instance,
    Adapter,
    Device,
    Queue,
    Buffer,
    MappedRange,
    StagingBuffer,
    Texture,
    TextureView,
    Sampler,
    BindGroupLayout,
    BindGroup,
    PipelineLayout,
    ShaderModule,
    RenderPipeline,
    ComputePipeline,
    QuerySet,
    CommandBuffer,
    CommandEncoder,
    ComputePass,
    RenderPass,
);
