//! The experiment's frame loop, shared by step A (native wgpu) and step C
//! (wgpu over wasi:webgpu): the generalised lbar document over N synthetic
//! inputs, painted by blitz into `anyrender_vello_hybrid` (vello_hybrid:
//! CPU preparation, GPU rasterisation), 600 frames at t = n / 60, one
//! [`Row`] a frame. The caller brings the `wgpu::Instance` and writes the
//! rows out.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyrender::PaintScene;
use anyrender_vello_hybrid::{ImageManager, VelloHybridScenePainter};
use blitz_bench_doc::{self as doc, GpuTiming, Meta, Row, Variant};
use compose_core::{Compositor, Fit, Geometry, RasterImageData};
use kurbo::{Affine, Rect};
use peniko::{Color, Fill};
use rustc_hash::FxHashMap;
use vello_hybrid::{RenderSettings, RenderSize, RenderTargetConfig, Renderer, Resources, TextureBindings};
use wgpu_context::DeviceHandle;

/// A frame's picture: the bytes and the blob the document is handed, kept
/// so that handing it again carries the same id.
struct Picture {
    w: u32,
    h: u32,
    base: Arc<Vec<u8>>,
    image: RasterImageData,
}

fn us(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e6
}

/// What a run is.
#[derive(Clone, Debug)]
pub struct Config {
    pub n: u32,
    pub variant: Variant,
    pub all_4k: bool,
    pub frames: u32,
    /// "A" native, "C" wasm.
    pub step: String,
}

pub struct Harness {
    config: Config,
    meta: Meta,
    device: wgpu::Device,
    queue: wgpu::Queue,
    device_handle: DeviceHandle,
    target: wgpu::Texture,
    target_view: wgpu::TextureView,
    renderer: Renderer,
    resources: Resources,
    scene: vello_hybrid::Scene,
    cache: FxHashMap<u64, vello_common::paint::ImageId>,
    texture_bindings: FxHashMap<anyrender::ResourceId, wgpu::TextureView>,
    query_set: Option<wgpu::QuerySet>,
    ts_resolve: wgpu::Buffer,
    ts_read: wgpu::Buffer,
    ts_period: f64,
    readback: wgpu::Buffer,
    comp: Compositor,
    pictures: Vec<Picture>,
    blob_bytes: HashMap<u64, u64>,
    to_destroy: Vec<vello_common::paint::ImageId>,
}

impl Harness {
    /// Asks `instance` for its best adapter and builds everything a run
    /// needs. `backend_name`, when given, is what the meta line calls the
    /// backend instead of wgpu's own word for it.
    pub fn new(instance: &wgpu::Instance, config: Config, backend_name: Option<&str>) -> Harness {
        let (w, h) = doc::OUT;
        let n = config.n;
        let variant = config.variant;
        let mut meta = Meta::new(&config.step, variant, n, config.all_4k, config.frames);

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .expect("no wgpu adapter");
        let info = adapter.get_info();
        meta.adapter = info.name.clone();
        meta.backend = backend_name
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{:?}", info.backend));
        meta.driver = info.driver.clone();
        meta.driver_info = info.driver_info.clone();
        meta.device_type = format!("{:?}", info.device_type);
        let have = adapter.features();
        let timestamps =
            have.contains(wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);
        let mut features = wgpu::Features::empty();
        for f in [wgpu::Features::CLEAR_TEXTURE, wgpu::Features::PIPELINE_CACHE] {
            if have.contains(f) {
                features |= f;
            }
        }
        if timestamps {
            features |= wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
            meta.gpu_timing = GpuTiming::Timestamp;
        } else {
            meta.gpu_timing = GpuTiming::Wall;
            meta.notes.push(
                "adapter has no TIMESTAMP_QUERY_INSIDE_ENCODERS: gpu_us is wall clock around submit and the wait"
                    .into(),
            );
        }
        let limits = wgpu::Limits::default().using_resolution(adapter.limits());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("bench"),
            required_features: features,
            required_limits: limits,
            ..Default::default()
        }))
        .expect("no wgpu device");
        device.on_uncaptured_error(Arc::new(|e| {
            eprintln!("bench: wgpu error: {e}");
            std::process::exit(3);
        }));
        let device_handle = DeviceHandle {
            instance: instance.clone(),
            adapter: adapter.clone(),
            device: device.clone(),
            queue: queue.clone(),
        };

        // The renderer: a 4K RGBA8 target, the atlas allowed sixteen
        // 4096x4096 layers (each 4K picture takes one; the default of eight
        // leaves nothing for glyphs with eight 4K inputs).
        let mut settings = RenderSettings::default();
        settings.memory_settings.image_atlas_config.max_atlases = 16;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let renderer = Renderer::new_with(
            &device,
            &RenderTargetConfig {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: w,
                height: h,
            },
            settings,
        );
        let resources = Resources::new_with_config(settings.memory_settings.image_atlas_config);
        let scene = vello_hybrid::Scene::new_with(w as u16, h as u16, settings);

        let query_set = timestamps.then(|| {
            device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("frame timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: 2,
            })
        });
        let ts_resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamps resolve"),
            size: 16,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let ts_read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamps read"),
            size: 16,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ts_period = queue.get_timestamp_period() as f64;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (w as u64) * (h as u64) * 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // The document and its inputs.
        let html = doc::document(n, variant.animated());
        let sizes: Vec<(u32, u32)> = (0..n).map(|i| doc::input_size(i, config.all_4k)).collect();
        let geometry = Geometry::new(
            (w, h),
            sizes[0],
            (None, None),
            (Some(doc::CSS_WIDTH as f64), None),
            Fit::Contain,
        );
        let t_build = Instant::now();
        let comp = Compositor::with_sizes(&html, geometry, sizes.iter().map(|s| Some(*s)).collect());
        meta.notes
            .push(format!("document built in {:.1} ms", t_build.elapsed().as_secs_f64() * 1e3));
        let pictures: Vec<Picture> = sizes
            .iter()
            .enumerate()
            .map(|(i, &(pw, ph))| {
                let mut v = doc::gradient(pw, ph, i as u32);
                doc::stamp(&mut v, pw, ph, 0);
                let base = Arc::new(v);
                Picture {
                    w: pw,
                    h: ph,
                    image: RasterImageData::new(pw, ph, base.clone()),
                    base,
                }
            })
            .collect();
        let mut blob_bytes = HashMap::new();
        for p in &pictures {
            blob_bytes.insert(p.image.data.id(), (p.w as u64) * (p.h as u64) * 4);
        }

        Harness {
            config,
            meta,
            device,
            queue,
            device_handle,
            target,
            target_view,
            renderer,
            resources,
            scene,
            cache: FxHashMap::default(),
            texture_bindings: FxHashMap::default(),
            query_set,
            ts_resolve,
            ts_read,
            ts_period,
            readback,
            comp,
            pictures,
            blob_bytes,
            to_destroy: Vec::new(),
        }
    }

    pub fn meta(&self) -> &Meta {
        &self.meta
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Renders frame `frame` (t = frame / 60) and returns its row. The 4K
    /// frame's pixels come back when the variant reads back or `want_pixels`
    /// asks for them (a readback either way, counted in the row).
    pub fn frame(&mut self, frame: u32, want_pixels: bool) -> (Row, Option<Vec<u8>>) {
        let (w, h) = doc::OUT;
        let n = self.config.n;
        let variant = self.config.variant;
        let tick = Instant::now();
        let t = frame as f64 / 60.0;
        let mut row = Row {
            frame,
            t,
            ..Default::default()
        };

        // Inputs: variant 2 makes a new blob a frame, the rest hand the same one.
        if variant.uploads_every_frame() {
            let s = Instant::now();
            for p in self.pictures.iter_mut() {
                let mut v: Vec<u8> = (*p.base).clone();
                doc::stamp(&mut v, p.w, p.h, frame);
                p.image = RasterImageData::new(p.w, p.h, Arc::new(v));
                self.blob_bytes
                    .insert(p.image.data.id(), (p.w as u64) * (p.h as u64) * 4);
            }
            row.synth_us = us(s);
        }

        let s = Instant::now();
        self.comp.resolve(t);
        row.resolve_us = us(s);
        let s = Instant::now();
        let plan = self.comp.plan(0);
        row.plan_us = us(s);
        if frame == 0 {
            let all = if n == 64 { u64::MAX } else { (1u64 << n) - 1 };
            if plan.drawn != all {
                eprintln!(
                    "bench: warning: frame 0 draws inputs {:#b}, expected all {n}",
                    plan.drawn
                );
            }
        }
        for (i, p) in self.pictures.iter().enumerate() {
            self.comp.set_frame_image(i as u32, &p.image);
        }

        // Variant 2: free last frame's atlas regions in a submission of their
        // own, before this frame's uploads: `write_texture` is staged ahead
        // of the command buffers of the submit it lands in, so a clear
        // recorded in this frame's encoder would wipe an upload into the
        // same region.
        if !self.to_destroy.is_empty() {
            let mut clears = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("free atlas"),
                });
            for id in self.to_destroy.drain(..) {
                self.renderer.destroy_image(&mut self.resources, &mut clears, id);
            }
            self.queue.submit([clears.finish()]);
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        if let Some(qs) = &self.query_set {
            encoder.write_timestamp(qs, 0);
        }
        let known: Vec<u64> = self.cache.keys().copied().collect();
        self.scene.reset();
        {
            let images = ImageManager::new(
                &mut self.renderer,
                &mut self.resources,
                &self.device,
                &self.queue,
                &mut encoder,
                &mut self.cache,
            );
            let mut painter =
                VelloHybridScenePainter::new(&mut self.scene, images, &mut self.texture_bindings, &self.device_handle);
            let s = Instant::now();
            painter.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                Color::from_rgba8(0, 0, 0, 255),
                None,
                &Rect::new(0.0, 0.0, w as f64, h as f64),
            );
            self.comp.paint_into(&mut painter);
            row.paint_cmds_us = us(s);
        }
        let gained: Vec<u64> = self.cache.keys().copied().filter(|k| !known.contains(k)).collect();
        for k in &gained {
            row.upload_bytes += self.blob_bytes.get(k).copied().unwrap_or(0);
        }

        let s = Instant::now();
        self.renderer
            .render(
                &self.scene,
                &mut self.resources,
                &self.device,
                &self.queue,
                &mut encoder,
                &RenderSize { width: w, height: h },
                &self.target_view,
                &TextureBindings::new(),
            )
            .expect("render failed");
        row.encode_us = us(s);
        if let Some(qs) = &self.query_set {
            encoder.write_timestamp(qs, 1);
            encoder.resolve_query_set(qs, 0..2, &self.ts_resolve, 0);
            encoder.copy_buffer_to_buffer(&self.ts_resolve, 0, &self.ts_read, 0, 16);
        }
        let read_now = variant.reads_back() || want_pixels;
        if read_now {
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
                        bytes_per_row: Some(w * 4),
                        rows_per_image: Some(h),
                    },
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        }

        let s = Instant::now();
        self.queue.submit([encoder.finish()]);
        row.submit_us = us(s);
        let s = Instant::now();
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        row.wait_us = us(s);
        if self.query_set.is_some() {
            // Mapping blocks until the GPU is done where `poll` does not
            // (the wasi host), so the wait is counted here too.
            let s = Instant::now();
            self.ts_read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            self.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
            let stamps: Vec<u64> = {
                let m = self.ts_read.slice(..).get_mapped_range();
                m.chunks_exact(8)
                    .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
                    .collect()
            };
            self.ts_read.unmap();
            row.wait_us += us(s);
            row.gpu_us = (stamps[1].wrapping_sub(stamps[0])) as f64 * self.ts_period / 1e3;
        } else {
            row.gpu_us = row.submit_us + row.wait_us;
        }
        let mut pixels = None;
        if read_now {
            // Today's output path: the frame's bytes into a Vec of the guest's.
            let s = Instant::now();
            self.readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            self.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
            let bytes: Vec<u8> = self.readback.slice(..).get_mapped_range().to_vec();
            self.readback.unmap();
            row.readback_us = us(s);
            pixels = Some(bytes);
        }
        if variant.uploads_every_frame() {
            for k in &gained {
                if let Some(id) = self.cache.remove(k) {
                    self.to_destroy.push(id);
                }
                self.blob_bytes.remove(k);
            }
        }
        row.frame_us = us(tick);
        if frame == doc::HASHED_FRAME {
            if let Some(p) = &pixels {
                row.pixel_hash = Some(doc::fnv1a(p));
            }
        }
        (row, pixels)
    }
}
