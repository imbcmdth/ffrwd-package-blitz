//! The document rasterised on the GPU: blitz paints into
//! `anyrender_vello_hybrid` (vello_hybrid: the scene prepared on the CPU,
//! rasterised by the GPU) on whatever `wgpu::Instance` the caller hands
//! over, and the frame is a new texture each time, left on the device.
//!
//! A picture reaches the renderer one of three ways ([`Picture`]):
//!
//! - As bytes, uploaded into vello_hybrid's image atlas once per blob id: a
//!   picture handed again with the same id is not uploaded again, and an
//!   upload no frame drew is freed before the next frame's. A picture with
//!   no transparency in it, which decoded video always is, goes to the atlas
//!   as it is; anything else is premultiplied on the way by vello_hybrid.
//! - As an rgba texture of the device, drawn from where it is: vello_hybrid
//!   samples a texture the caller binds (`Scene::draw_texture_rects`, which
//!   anyrender_vello_hybrid reaches for a `Paint::Resource`), and a picture
//!   given as a texture is drawn that way, with nothing copied.
//! - As the three planes of a yuv420p picture, converted to rgba on the
//!   device by one pass of our own, once per blob id and input, into a
//!   texture kept per input, and drawn as an rgba texture is.

use std::any::Any;
use std::sync::Arc;
use std::time::Instant;

use anyrender::{
    Filter, Glyph, NormalizedCoord, Paint, PaintRef, PaintScene, RegisterResourceError,
    RenderContext, ResourceId,
};
use anyrender_vello_hybrid::{ImageManager, VelloHybridScenePainter};
use kurbo::{Affine, Rect, Shape, Stroke};
use peniko::{BlendMode, Color, Fill, FontData, ImageBrush, StyleRef};
use rustc_hash::{FxHashMap, FxHashSet};
use vello_common::TextureId;
use vello_common::paint::ImageId;
use vello_hybrid::{
    AtlasWriter, RenderSettings, RenderSize, RenderTargetConfig, Renderer, Resources,
    TextureBindings,
};
use wgpu::util::DeviceExt;

use crate::RasterImageData;
use wgpu_context::DeviceHandle;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// What a frame the renderer makes, or a picture it converts, can be used
/// for: drawn into, sampled, copied from and into (the host takes a frame
/// that is `TEXTURE_BINDING` and `COPY_SRC`).
const FRAME_USAGE: wgpu::TextureUsages = wgpu::TextureUsages::RENDER_ATTACHMENT
    .union(wgpu::TextureUsages::TEXTURE_BINDING)
    .union(wgpu::TextureUsages::COPY_SRC)
    .union(wgpu::TextureUsages::COPY_DST);

/// The atlas layer's side, px: vello_hybrid's default. A picture larger
/// than this on either side cannot be uploaded as bytes; a picture given as
/// a texture is drawn from where it is and has no such bound.
pub const MAX_PICTURE: u32 = 4096;

/// Atlas layers allowed: each 4K picture takes most of one.
const MAX_ATLASES: usize = 16;

/// The YUV matrix a picture was coded with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Matrix {
    Bt601,
    Bt709,
}

/// How a yuv picture's samples read as colour: its matrix, and whether
/// they span the whole of 0 to 255 (`pc`) or the limited range (`tv`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Colour {
    pub matrix: Matrix,
    pub full: bool,
}

impl Colour {
    /// A stream's colour from its space and range in ffmpeg's names:
    /// `bt709` is BT.709 and every other space BT.601 (as swscale reads a
    /// stream that does not say), `pc` is full range and anything else
    /// limited.
    pub fn of(space: &str, range: &str) -> Colour {
        Colour {
            matrix: if space == "bt709" {
                Matrix::Bt709
            } else {
                Matrix::Bt601
            },
            full: range == "pc",
        }
    }

    /// The rows taking (Y, U, V, 1), each as an `r8unorm` texture holds it
    /// (0 to 1), to red, green and blue.
    pub fn rows(&self) -> [[f32; 4]; 3] {
        let (kr, kb) = match self.matrix {
            Matrix::Bt601 => (0.299, 0.114),
            Matrix::Bt709 => (0.2126, 0.0722),
        };
        let kg = 1.0 - kr - kb;
        // Y' = y_scale * (Y - y_offset), and a chroma sample c_scale * (C - 128/255).
        let (y_scale, y_offset, c_scale): (f64, f64, f64) = if self.full {
            (1.0, 0.0, 1.0)
        } else {
            (255.0 / 219.0, 16.0 / 255.0, 255.0 / 224.0)
        };
        let c_offset = 128.0 / 255.0;
        let row = |cb: f64, cr: f64| {
            let (u, v) = (cb * c_scale, cr * c_scale);
            [
                y_scale as f32,
                u as f32,
                v as f32,
                (-y_scale * y_offset - (u + v) * c_offset) as f32,
            ]
        };
        [
            row(0.0, 2.0 * (1.0 - kr)),
            row(-2.0 * kb * (1.0 - kb) / kg, -2.0 * kr * (1.0 - kr) / kg),
            row(2.0 * (1.0 - kb), 0.0),
        ]
    }
}

/// A picture handed to the renderer for one input.
#[derive(Clone, Debug)]
pub enum Picture {
    /// Straight RGBA8 bytes, uploaded to the atlas once per blob id.
    Bytes(RasterImageData),
    /// One `rgba8unorm` texture of the device, drawn as it is. It is read
    /// as premultiplied, which a picture with no transparency is.
    Rgba {
        image: RasterImageData,
        texture: wgpu::Texture,
    },
    /// The Y, U and V planes of a yuv420p picture, `r8unorm` textures of
    /// the device (U and V at half the size, rounded up), and how they read
    /// as colour; converted to rgba once per blob id.
    Yuv420p {
        image: RasterImageData,
        planes: [wgpu::Texture; 3],
        colour: Colour,
    },
}

impl Picture {
    /// What the document holds for it: its size, and the blob id the
    /// renderer keys it by.
    pub fn image(&self) -> &RasterImageData {
        match self {
            Picture::Bytes(image)
            | Picture::Rgba { image, .. }
            | Picture::Yuv420p { image, .. } => image,
        }
    }

    /// Whether it is on the device rather than in bytes.
    pub fn on_gpu(&self) -> bool {
        !matches!(self, Picture::Bytes(_))
    }
}

impl From<RasterImageData> for Picture {
    fn from(image: RasterImageData) -> Picture {
        Picture::Bytes(image)
    }
}

/// The pass that turns yuv420p planes into an rgba texture.
struct Converter {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// One uniform buffer of rows per colour seen.
    colours: FxHashMap<Colour, wgpu::Buffer>,
}

const CONVERT: &str = r#"
struct Colour { r: vec4<f32>, g: vec4<f32>, b: vec4<f32> }

@group(0) @binding(0) var y_plane: texture_2d<f32>;
@group(0) @binding(1) var u_plane: texture_2d<f32>;
@group(0) @binding(2) var v_plane: texture_2d<f32>;
@group(0) @binding(3) var<uniform> colour: Colour;

// One triangle over the whole target.
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

// Each chroma sample covers the two by two pixels it is coded for. Alpha is
// one, so the colour is its own premultiplied value.
@fragment
fn fs(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(at.xy);
    let c = p / 2;
    let yuv = vec4<f32>(
        textureLoad(y_plane, p, 0).r,
        textureLoad(u_plane, c, 0).r,
        textureLoad(v_plane, c, 0).r,
        1.0,
    );
    let rgb = vec3<f32>(dot(colour.r, yuv), dot(colour.g, yuv), dot(colour.b, yuv));
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
"#;

impl Converter {
    fn new(device: &wgpu::Device) -> Converter {
        let plane = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yuv420p"),
            entries: &[
                plane(0),
                plane(1),
                plane(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("yuv420p"),
            source: wgpu::ShaderSource::Wgsl(CONVERT.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yuv420p"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yuv420p"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Converter {
            pipeline,
            layout,
            colours: FxHashMap::default(),
        }
    }

    /// Records the conversion of `planes` into `target` on `encoder`.
    fn convert(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        planes: &[wgpu::Texture; 3],
        colour: Colour,
        target: &wgpu::Texture,
    ) {
        let rows = self.colours.entry(colour).or_insert_with(|| {
            let bytes: Vec<u8> = colour
                .rows()
                .iter()
                .flatten()
                .flat_map(|v| v.to_le_bytes())
                .collect();
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("yuv420p colour"),
                contents: &bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            })
        });
        let views: Vec<wgpu::TextureView> = planes
            .iter()
            .map(|p| p.create_view(&wgpu::TextureViewDescriptor::default()))
            .collect();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yuv420p"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: rows.as_entire_binding(),
                },
            ],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("yuv420p"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

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
    /// The pictures the frame being painted draws from textures, by blob
    /// id: the resource each is bound as, and its size.
    textures: FxHashMap<u64, (ResourceId, u32, u32)>,
    /// Blob ids the frame being painted draws.
    used: FxHashSet<u64>,
    /// Uploads the last frame did not draw, freed before the next one's.
    stale: Vec<ImageId>,
    /// Pictures uploaded since the renderer was built.
    uploads: u64,
    converter: Option<Converter>,
    /// Per input, the yuv picture last converted: its blob id and the rgba
    /// texture it was converted into, which the input's next picture is
    /// converted into in turn.
    converted: Vec<Option<(u64, wgpu::Texture)>>,
    /// Pictures converted since the renderer was built.
    conversions: u64,
    /// For `read_back`, made the first time it is asked.
    readback: Option<wgpu::Buffer>,
    width: u32,
    height: u32,
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
            textures: FxHashMap::default(),
            used: FxHashSet::default(),
            stale: Vec::new(),
            uploads: 0,
            converter: None,
            converted: Vec::new(),
            conversions: 0,
            readback: None,
            width,
            height,
        })
    }

    /// The adapter's name.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// The device a picture given as a texture is on.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// The frame size this renderer was built for.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// How many pictures have been uploaded since the renderer was built.
    pub fn uploads(&self) -> u64 {
        self.uploads
    }

    /// How many yuv pictures have been converted since the renderer was
    /// built.
    pub fn conversions(&self) -> u64 {
        self.conversions
    }

    /// Whether a picture of `w` x `h` fits in the image atlas.
    pub fn takes_picture(w: u32, h: u32) -> bool {
        w <= MAX_PICTURE && h <= MAX_PICTURE
    }

    /// A new `width` x `height` rgba texture, for a frame or a conversion.
    fn texture(&self, label: &str, (width, height): (u32, u32)) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: FRAME_USAGE,
            view_formats: &[],
        })
    }

    /// The rgba texture `picture` of `input` is drawn from: an rgba
    /// texture as it is, and a yuv picture converted (recorded on
    /// `encoder`) unless the input's last conversion was of the same blob
    /// id. None for bytes.
    fn source(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        input: u32,
        picture: &Picture,
    ) -> Option<wgpu::Texture> {
        let (image, planes, colour) = match picture {
            Picture::Bytes(_) => return None,
            Picture::Rgba { texture, .. } => return Some(texture.clone()),
            Picture::Yuv420p {
                image,
                planes,
                colour,
            } => (image, planes, *colour),
        };
        let blob = image.data.id();
        let slot = input as usize;
        if self.converted.len() <= slot {
            self.converted.resize(slot + 1, None);
        }
        if let Some((kept, texture)) = &self.converted[slot]
            && *kept == blob
        {
            return Some(texture.clone());
        }
        let size = (image.width, image.height);
        let target = match self.converted[slot].take() {
            Some((_, texture)) if (texture.width(), texture.height()) == size => texture,
            _ => self.texture("converted", size),
        };
        let device = &self.device;
        self.converter
            .get_or_insert_with(|| Converter::new(device))
            .convert(device, encoder, planes, colour, &target);
        self.conversions += 1;
        self.converted[slot] = Some((blob, target.clone()));
        Some(target)
    }

    /// `picture` of `input` alone, as a new rgba texture of its own size:
    /// what a frame that is nothing but the picture hands on when the
    /// picture cannot leave as it is (a yuv picture, converted as it is for
    /// drawing). None for bytes.
    pub fn frame_of(&mut self, input: u32, picture: &Picture) -> Option<wgpu::Texture> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame of a picture"),
            });
        let source = self.source(&mut encoder, input, picture)?;
        let image = picture.image();
        let frame = self.texture("frame", (image.width, image.height));
        encoder.copy_texture_to_texture(
            source.as_image_copy(),
            frame.as_image_copy(),
            wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        Some(frame)
    }

    /// Paints with `paint` into a new texture and returns it (RGBA8,
    /// premultiplied, the renderer's size; drawn into, sampled, copied from
    /// and into). `pictures` are the video pictures the document was handed
    /// for this frame, by input. Returns it with (paint_cmds_us,
    /// raster_us): building the scene, uploads and conversions included,
    /// then the rest, which records and submits the GPU's work without
    /// waiting for it.
    pub fn render(
        &mut self,
        pictures: &[(u32, Picture)],
        paint: impl FnOnce(&mut Recorded<'_, VelloHybridScenePainter<'_>>),
    ) -> (wgpu::Texture, f64, f64) {
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
        self.textures.clear();
        self.bindings.clear();
        let cached = self.cache.len();
        for (input, picture) in pictures {
            let image = picture.image();
            let blob = image.data.id();
            if picture.on_gpu() {
                if self.textures.contains_key(&blob) {
                    continue;
                }
                let Some(texture) = self.source(&mut encoder, *input, picture) else {
                    continue;
                };
                let id = ResourceId::new();
                self.bindings.insert(
                    id,
                    texture.create_view(&wgpu::TextureViewDescriptor::default()),
                );
                self.textures
                    .insert(blob, (id, texture.width(), texture.height()));
                continue;
            }
            if self.cache.contains_key(&blob) {
                continue;
            }
            let bytes = image.data.data();
            if bytes.len() != (image.width as usize) * (image.height as usize) * 4 || !opaque(bytes)
            {
                continue;
            }
            let id = self.renderer.upload_image(
                &mut self.resources,
                &self.device,
                &self.queue,
                &mut encoder,
                &Opaque {
                    width: image.width,
                    height: image.height,
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
                textures: &self.textures,
            });
        }
        self.uploads += (self.cache.len() - cached) as u64;
        let cmds = us(start);

        let target = self.texture("frame", (self.width, self.height));
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        // The target starts each frame transparent, as the CPU renderer's
        // buffer does.
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        }));
        let mut bound = TextureBindings::new();
        for (id, view) in &self.bindings {
            bound.insert(TextureId(id.into_ffi()), view.clone());
        }
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
                &view,
                &bound,
            )
            .expect("vello_hybrid render");
        self.queue.submit([encoder.finish()]);
        self.bindings.clear();
        self.textures.clear();

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
        (target, cmds, all - cmds)
    }

    /// Waits for `texture`, an rgba8 texture of this device, and reads it
    /// into `out`, which it fills tightly packed: the frame as bytes.
    pub fn read_back(&mut self, texture: &wgpu::Texture, out: &mut Vec<u8>) {
        let (width, height) = (texture.width(), texture.height());
        let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let size = u64::from(row) * u64::from(height);
        if self.readback.as_ref().is_none_or(|b| b.size() < size) {
            self.readback = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("readback"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let readback = self.readback.as_ref().expect("made above");
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("read back"),
            });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let slice = readback.slice(..size);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for the GPU");
        {
            let mapped = slice.get_mapped_range();
            let line = (width * 4) as usize;
            out.clear();
            out.reserve(line * height as usize);
            if line == row as usize {
                out.extend_from_slice(&mapped[..line * height as usize]);
            } else {
                for r in mapped.chunks_exact(row as usize) {
                    out.extend_from_slice(&r[..line]);
                }
            }
        }
        readback.unmap();
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

/// A paint target that passes everything to the one underneath, notes the
/// blob id of every picture drawn, and draws a picture given as a texture
/// from that texture.
pub struct Recorded<'a, S: PaintScene> {
    inner: &'a mut S,
    used: &'a mut FxHashSet<u64>,
    textures: &'a FxHashMap<u64, (ResourceId, u32, u32)>,
}

impl<S: PaintScene> Recorded<'_, S> {
    fn note(&mut self, brush: &PaintRef<'_>) {
        if let PaintRef::Image(image) = brush {
            self.used.insert(image.image.data.id());
        }
    }

    /// The resource a brush's picture is bound as, with its size, when the
    /// picture was given as a texture.
    fn texture(&self, brush: &PaintRef<'_>) -> Option<(ResourceId, u32, u32)> {
        match brush {
            PaintRef::Image(image) => self.textures.get(&image.image.data.id()).copied(),
            _ => None,
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
        // A picture given as a texture is drawn by a fill alone.
        if self.texture(&brush).is_some() {
            return;
        }
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
        if let (PaintRef::Image(image), Some((id, w, h))) = (&brush, self.texture(&brush)) {
            // vello_hybrid draws a bound texture as a rectangle of the
            // texture's size placed by the transform. A fill of exactly that
            // rectangle, which is how an `<img>` is drawn, is that; any other
            // shape or brush transform clips the rectangle to the shape.
            let resource = Paint::Resource(ImageBrush {
                image: id,
                sampler: image.sampler,
            });
            let whole = Rect::new(0.0, 0.0, f64::from(w), f64::from(h));
            if brush_transform.is_none() && shape.as_rect() == Some(whole) {
                self.inner.fill(style, transform, resource, None, &whole);
            } else {
                self.inner.push_clip_layer(transform, shape);
                self.inner.fill(
                    Fill::NonZero,
                    transform * brush_transform.unwrap_or_default(),
                    resource,
                    None,
                    &whole,
                );
                self.inner.pop_layer();
            }
            return;
        }
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
        if self.texture(&brush).is_some() {
            return;
        }
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

#[cfg(test)]
mod tests {
    use super::{Colour, Matrix};

    /// (Y, U, V) as 8-bit samples to (R, G, B) as 8-bit values, through the
    /// rows the conversion pass uses.
    fn rgb(colour: Colour, yuv: [u8; 3]) -> [u8; 3] {
        let v = [
            f32::from(yuv[0]) / 255.0,
            f32::from(yuv[1]) / 255.0,
            f32::from(yuv[2]) / 255.0,
            1.0,
        ];
        colour.rows().map(|row| {
            let x: f32 = row.iter().zip(v).map(|(a, b)| a * b).sum();
            (x.clamp(0.0, 1.0) * 255.0).round() as u8
        })
    }

    #[test]
    fn limited_range_white_and_black_are_235_and_16() {
        for matrix in [Matrix::Bt601, Matrix::Bt709] {
            let c = Colour {
                matrix,
                full: false,
            };
            assert_eq!(rgb(c, [235, 128, 128]), [255, 255, 255]);
            assert_eq!(rgb(c, [16, 128, 128]), [0, 0, 0]);
            assert_eq!(rgb(c, [126, 128, 128]), [128, 128, 128]);
        }
    }

    #[test]
    fn full_range_grey_is_its_own_level() {
        let c = Colour::of("bt709", "pc");
        assert_eq!(c.matrix, Matrix::Bt709);
        assert!(c.full);
        for y in [0, 77, 128, 255] {
            assert_eq!(rgb(c, [y, 128, 128]), [y, y, y]);
        }
    }

    #[test]
    fn the_primaries_come_back() {
        // BT.601 and BT.709 limited-range red, green and blue.
        let bt601 = Colour::of("unknown", "tv");
        let bt709 = Colour::of("bt709", "tv");
        let near = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 1);
        assert!(near(rgb(bt601, [81, 90, 240]), [255, 0, 0]));
        assert!(near(rgb(bt601, [145, 54, 34]), [0, 255, 0]));
        assert!(near(rgb(bt601, [41, 240, 110]), [0, 0, 255]));
        assert!(near(rgb(bt709, [63, 102, 240]), [255, 0, 0]));
        assert!(near(rgb(bt709, [173, 42, 26]), [0, 255, 0]));
        assert!(near(rgb(bt709, [32, 240, 118]), [0, 0, 255]));
    }

    #[test]
    fn an_unsettled_space_is_bt601_limited() {
        let c = Colour::of("unknown", "unknown");
        assert_eq!(
            c,
            Colour {
                matrix: Matrix::Bt601,
                full: false
            }
        );
    }
}
