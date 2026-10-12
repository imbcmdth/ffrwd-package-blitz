//! The document rasterised on the GPU: blitz paints into
//! `anyrender_vello_hybrid` (vello_hybrid: the scene prepared on the CPU,
//! rasterised by the GPU) on whatever `wgpu::Instance` the caller hands
//! over, and the frame is read back into the same premultiplied RGBA8
//! bytes the CPU renderer writes.
//!
//! Pictures are uploaded into vello_hybrid's image atlas once per blob id:
//! a picture handed again with the same id is not uploaded again. An upload
//! no frame drew is freed before the next frame's. A video picture with no
//! transparency in it, which decoded video always is, goes to the atlas as
//! it is; anything else is premultiplied on the way by vello_hybrid.

use std::any::Any;
use std::sync::Arc;
use std::time::Instant;

use anyrender::{
    Filter, Glyph, NormalizedCoord, PaintRef, PaintScene, RegisterResourceError, RenderContext,
    ResourceId,
};
use anyrender_vello_hybrid::{ImageManager, VelloHybridScenePainter};
use kurbo::{Affine, Rect, Shape, Stroke};
use peniko::{BlendMode, Color, Fill, FontData, StyleRef};
use rustc_hash::{FxHashMap, FxHashSet};
use vello_common::paint::ImageId;
use vello_hybrid::{
    AtlasWriter, RenderSettings, RenderSize, RenderTargetConfig, Renderer, Resources,
    TextureBindings,
};

use crate::RasterImageData;
use wgpu_context::DeviceHandle;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The atlas layer's side, px: vello_hybrid's default. A picture larger
/// than this on either side cannot be uploaded.
pub const MAX_PICTURE: u32 = 4096;

/// Atlas layers allowed: each 4K picture takes most of one.
const MAX_ATLASES: usize = 16;

pub struct Gpu {
    adapter: String,
    device: wgpu::Device,
    queue: wgpu::Queue,
    handle: DeviceHandle,
    renderer: Renderer,
    resources: Resources,
    scene: vello_hybrid::Scene,
    /// Uploads by blob id.
    cache: FxHashMap<u64, ImageId>,
    bindings: FxHashMap<ResourceId, wgpu::TextureView>,
    /// Blob ids the frame being painted draws.
    used: FxHashSet<u64>,
    /// Uploads the last frame did not draw, freed before the next one's.
    stale: Vec<ImageId>,
    /// Pictures uploaded since the renderer was built.
    uploads: u64,
    target: wgpu::Texture,
    view: wgpu::TextureView,
    readback: wgpu::Buffer,
    width: u32,
    height: u32,
    /// A row of the readback buffer, bytes: the frame's row padded to
    /// wgpu's copy alignment.
    row: u32,
}

impl Gpu {
    /// Asks `instance` for an adapter and builds a renderer for frames of
    /// `width` x `height`. The error says why there is none: no adapter, a
    /// device refused, or a frame larger than the device takes.
    pub fn new(instance: &wgpu::Instance, (width, height): (u32, u32)) -> Result<Gpu, String> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .map_err(|e| format!("no GPU adapter: {e}"))?;
        let info = adapter.get_info();
        let limits = wgpu::Limits::default().using_resolution(adapter.limits());
        if width > limits.max_texture_dimension_2d || height > limits.max_texture_dimension_2d {
            return Err(format!(
                "a {width}x{height} frame is larger than the GPU's textures ({} px a side)",
                limits.max_texture_dimension_2d
            ));
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("compose"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            ..Default::default()
        }))
        .map_err(|e| format!("the GPU refused a device: {e}"))?;
        let handle = DeviceHandle {
            instance: instance.clone(),
            adapter: adapter.clone(),
            device: device.clone(),
            queue: queue.clone(),
        };

        let mut settings = RenderSettings::default();
        settings.memory_settings.image_atlas_config.atlas_size = (MAX_PICTURE, MAX_PICTURE);
        settings.memory_settings.image_atlas_config.max_atlases = MAX_ATLASES;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let renderer = Renderer::new_with(
            &device,
            &RenderTargetConfig {
                format: FORMAT,
                width,
                height,
            },
            settings,
        );
        let resources = Resources::new_with_config(settings.memory_settings.image_atlas_config);
        let scene = vello_hybrid::Scene::new_with(width as u16, height as u16, settings);
        let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(row) * u64::from(height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Gpu {
            adapter: info.name,
            device,
            queue,
            handle,
            renderer,
            resources,
            scene,
            cache: FxHashMap::default(),
            bindings: FxHashMap::default(),
            used: FxHashSet::default(),
            stale: Vec::new(),
            uploads: 0,
            target,
            view,
            readback,
            width,
            height,
            row,
        })
    }

    /// The adapter's name.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// The frame size this renderer was built for.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// How many pictures have been uploaded since the renderer was built.
    pub fn uploads(&self) -> u64 {
        self.uploads
    }

    /// Whether a picture of `w` x `h` fits in the image atlas.
    pub fn takes_picture(w: u32, h: u32) -> bool {
        w <= MAX_PICTURE && h <= MAX_PICTURE
    }

    /// Paints with `paint` and reads the frame back into `out`, which it
    /// fills (RGBA8, premultiplied, `width * height * 4` bytes). `pictures` are the video
    /// pictures the document was handed for this frame. Returns
    /// (paint_cmds_us, raster_us): building the scene, uploads included,
    /// then the rest, readback included.
    pub fn render(
        &mut self,
        out: &mut Vec<u8>,
        pictures: &[RasterImageData],
        paint: impl FnOnce(&mut Recorded<'_, VelloHybridScenePainter<'_>>),
    ) -> (f64, f64) {
        let start = Instant::now();
        // Freed in a submission of their own: `write_texture` is staged
        // ahead of the command buffers of the submit it lands in, so a clear
        // recorded beside this frame's uploads could wipe one of them.
        if !self.stale.is_empty() {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("free pictures"),
                });
            for id in self.stale.drain(..) {
                self.renderer
                    .destroy_image(&mut self.resources, &mut encoder, id);
            }
            self.queue.submit([encoder.finish()]);
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        self.scene.reset();
        self.used.clear();
        let cached = self.cache.len();
        for picture in pictures {
            let blob = picture.data.id();
            if self.cache.contains_key(&blob) {
                continue;
            }
            let bytes = picture.data.data();
            if bytes.len() != (picture.width as usize) * (picture.height as usize) * 4
                || !opaque(bytes)
            {
                continue;
            }
            let id = self.renderer.upload_image(
                &mut self.resources,
                &self.device,
                &self.queue,
                &mut encoder,
                &Opaque {
                    width: picture.width,
                    height: picture.height,
                    bytes,
                },
            );
            self.cache.insert(blob, id);
        }
        {
            let images = ImageManager::new(
                &mut self.renderer,
                &mut self.resources,
                &self.device,
                &self.queue,
                &mut encoder,
                &mut self.cache,
            );
            let mut painter = VelloHybridScenePainter::new(
                &mut self.scene,
                images,
                &mut self.bindings,
                &self.handle,
            );
            paint(&mut Recorded {
                inner: &mut painter,
                used: &mut self.used,
            });
        }
        self.uploads += (self.cache.len() - cached) as u64;
        let cmds = us(start);

        // The target starts each frame transparent, as the CPU renderer's
        // buffer does.
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        }));
        self.renderer
            .render(
                &self.scene,
                &mut self.resources,
                &self.device,
                &self.queue,
                &mut encoder,
                &RenderSize {
                    width: self.width,
                    height: self.height,
                },
                &self.view,
                &TextureBindings::new(),
            )
            .expect("vello_hybrid render");
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);

        let slice = self.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for the GPU");
        {
            let mapped = slice.get_mapped_range();
            let line = (self.width * 4) as usize;
            out.clear();
            out.reserve(line * self.height as usize);
            if line == self.row as usize {
                out.extend_from_slice(&mapped[..line * self.height as usize]);
            } else {
                for row in mapped.chunks_exact(self.row as usize) {
                    out.extend_from_slice(&row[..line]);
                }
            }
        }
        self.readback.unmap();

        // What this frame did not draw is freed before the next frame.
        let used = &self.used;
        let stale = &mut self.stale;
        self.cache.retain(|blob, id| {
            let keep = used.contains(blob);
            if !keep {
                stale.push(*id);
            }
            keep
        });
        let all = us(start);
        (cmds, all - cmds)
    }
}

/// Whether every pixel of an RGBA8 picture has alpha 255, read two pixels
/// at a time.
fn opaque(bytes: &[u8]) -> bool {
    const ALPHAS: u64 = 0xff00_0000_ff00_0000;
    let (pairs, rest) = bytes.as_chunks::<8>();
    pairs.chunks(1 << 13).all(|block| {
        block
            .iter()
            .fold(ALPHAS, |all, pair| all & u64::from_le_bytes(*pair))
            & ALPHAS
            == ALPHAS
    }) && rest.as_chunks::<4>().0.iter().all(|px| px[3] == 0xff)
}

/// An opaque picture's bytes, written into the atlas as they are: with
/// alpha 255 everywhere, straight and premultiplied are the same bytes.
struct Opaque<'a> {
    width: u32,
    height: u32,
    bytes: &'a [u8],
}

impl AtlasWriter for Opaque<'_> {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn write_to_atlas_layer(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _encoder: &mut wgpu::CommandEncoder,
        atlas_texture: &wgpu::Texture,
        layer: u32,
        offset: [u32; 2],
        width: u32,
        height: u32,
    ) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: atlas_texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: offset[0],
                    y: offset[1],
                    z: layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            self.bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
}

fn us(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e6
}

/// A paint target that passes everything to the one underneath and notes
/// the blob id of every picture drawn.
pub struct Recorded<'a, S: PaintScene> {
    inner: &'a mut S,
    used: &'a mut FxHashSet<u64>,
}

impl<S: PaintScene> Recorded<'_, S> {
    fn note(&mut self, brush: &PaintRef<'_>) {
        if let PaintRef::Image(image) = brush {
            self.used.insert(image.image.data.id());
        }
    }
}

impl<S: PaintScene> RenderContext for Recorded<'_, S> {
    fn try_register_custom_resource(
        &mut self,
        resource: Box<dyn Any>,
    ) -> Result<ResourceId, RegisterResourceError> {
        self.inner.try_register_custom_resource(resource)
    }

    fn unregister_resource(&mut self, resource_id: ResourceId) {
        self.inner.unregister_resource(resource_id)
    }

    fn renderer_specific_context(&self) -> Option<Box<dyn Any>> {
        self.inner.renderer_specific_context()
    }
}

impl<S: PaintScene> PaintScene for Recorded<'_, S> {
    fn reset(&mut self) {
        self.inner.reset()
    }

    fn push_layer(
        &mut self,
        blend: impl Into<BlendMode>,
        alpha: f32,
        transform: Affine,
        clip: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
    ) {
        self.inner
            .push_layer(blend, alpha, transform, clip, filter, backdrop_filter)
    }

    fn push_clip_layer(&mut self, transform: Affine, clip: &impl Shape) {
        self.inner.push_clip_layer(transform, clip)
    }

    fn pop_layer(&mut self) {
        self.inner.pop_layer()
    }

    fn stroke<'b>(
        &mut self,
        style: &Stroke,
        transform: Affine,
        brush: impl Into<PaintRef<'b>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        let brush = brush.into();
        self.note(&brush);
        self.inner
            .stroke(style, transform, brush, brush_transform, shape)
    }

    fn fill<'b>(
        &mut self,
        style: Fill,
        transform: Affine,
        brush: impl Into<PaintRef<'b>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        let brush = brush.into();
        self.note(&brush);
        self.inner
            .fill(style, transform, brush, brush_transform, shape)
    }

    fn draw_glyphs<'b, 's: 'b>(
        &'s mut self,
        font: &'b FontData,
        font_size: f32,
        hint: bool,
        normalized_coords: &'b [NormalizedCoord],
        embolden: kurbo::Vec2,
        style: impl Into<StyleRef<'b>>,
        brush: impl Into<PaintRef<'b>>,
        brush_alpha: f32,
        transform: Affine,
        glyph_transform: Option<Affine>,
        glyphs: impl Iterator<Item = Glyph> + Clone,
    ) {
        let brush = brush.into();
        self.note(&brush);
        self.inner.draw_glyphs(
            font,
            font_size,
            hint,
            normalized_coords,
            embolden,
            style,
            brush,
            brush_alpha,
            transform,
            glyph_transform,
            glyphs,
        )
    }

    fn draw_box_shadow(
        &mut self,
        transform: Affine,
        rect: Rect,
        brush: Color,
        radius: f64,
        std_dev: f64,
    ) {
        self.inner
            .draw_box_shadow(transform, rect, brush, radius, std_dev)
    }
}
