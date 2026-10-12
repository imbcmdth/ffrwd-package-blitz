//! `compose` and `page`: one HTML document rendered by Blitz each tick over
//! the video inputs it shows, as a node.
//!
//! The clock is `v` when the call binds it (`compose`), and otherwise a rate
//! of `fps` (`page`, a source). `inputs` are held, each showing its newest
//! frame at the tick and nothing while its feed is down; one given by a
//! port is whatever connects there. `v` is `ffrwd:0` in the document and
//! the held inputs `ffrwd:1` on, in the order the call names them. Rows on
//! `changes` are folded as state and presence rows come from the host's
//! record of each feed, so a call is pure and a host spreads it over
//! workers, each folding the rows of the ticks the others rendered.
//!
//! The document is rasterised on the GPU through wgpu over wasi:webgpu when
//! the host has one (`renderer` `auto`, the default, or `gpu`), and on the
//! CPU otherwise or with `renderer` `cpu`. On the GPU the video ports are
//! held there too (`ffrwd:gpu`): each input picture is drawn from the
//! host's texture, a yuv420p one converted on the device first, and the
//! frame is handed back as a texture, so no picture's bytes cross into the
//! module or out of it.
//! On the CPU a yuv420p picture is fetched as it is and converted to rgba
//! here, in its stream's matrix and range.

use std::collections::HashMap;
use std::time::Instant;

use std::sync::Arc;

use compose_core::gpu::{MAX_PICTURE, Matrix};
use compose_core::params::{Log, Params as Document, Renderer};
use compose_core::presence::{self, Clock, Entry, Presence, Seen};
use compose_core::{
    Colour, Gpu, MAX_INPUTS, Output as Made, Picture, RasterImageData, Session, bit,
};
use ffrwd_frame::yuv;
use ffrwd_node::{
    Anchor, Bound, BoundStream, Frame, Init, Input, Node, Out, Output, Rational, Result, Shape,
    StateRow, Tick,
};
use serde::Deserialize;
use serde_json::{Map, Value};

#[cfg(target_arch = "wasm32")]
mod frames;

/// On any target but wasm32 there is no host to hold frames on the GPU.
#[cfg(not(target_arch = "wasm32"))]
mod frames {
    pub fn shaped(_clocked: bool) {}

    pub fn planes(
        _id: u32,
        _index: u32,
        _sizes: &[(u32, u32, wgpu::TextureFormat)],
    ) -> Result<Vec<wgpu::Texture>, String> {
        Err("frames are held on the GPU only by a wasm32 module".to_owned())
    }

    pub fn emit(
        _port: &str,
        _pts: i64,
        _duration: Option<i64>,
        _texture: wgpu::Texture,
    ) -> Result<(), String> {
        Err("frames are held on the GPU only by a wasm32 module".to_owned())
    }
}

const PARAMS_SCHEMA: &str = r#"{"type":"object","properties":{"html":{"type":"string","default":""},"rows":{"type":"string","default":""},"width":{"type":"number","minimum":1},"height":{"type":"number","minimum":1},"css_width":{"type":"number","minimum":1},"css_height":{"type":"number","minimum":1},"fit":{"type":"string","enum":["contain","stretch"],"default":"contain"},"bypass":{"type":"boolean","default":true},"fps":{"type":"number","exclusiveMinimum":0,"maximum":240,"default":30},"port":{"type":["array","integer"],"items":{"type":"integer","minimum":1,"maximum":65535},"minimum":1,"maximum":65535},"lead":{"type":"number","minimum":0,"maximum":60,"default":0.3},"linger":{"type":"number","minimum":0,"maximum":60,"default":0},"timeout":{"type":"number","minimum":0,"maximum":60,"default":1},"latency":{"type":"number","minimum":0},"presence":{"type":"string","default":""},"renderer":{"type":"string","enum":["auto","gpu","cpu"],"default":"auto"},"log":{"type":"string","enum":["off","summary","frame"],"default":"off"}},"additionalProperties":false}"#;

/// What `changes` reads: rows with a selector. The rest of a row is read
/// when it is applied, and a row that does not read is logged.
const CHANGES_SCHEMA: &str =
    r#"{"type":"object","properties":{"select":{"type":"string"}},"required":["select"]}"#;

/// The tag a source puts on its stream to say its timestamps are the
/// clock's, as a timed feeder's are.
const TIMED_TAG: &str = "smart_timed";

const PIXEL_FORMAT: &str = "rgba";

const YUV420P: &str = "yuv420p";

/// What a video input is taken as: rgba, or yuv420p, which the module
/// converts (on the device when its frames are held there). The compiler
/// puts the first on the wire. On the CPU yuv420p is first: the module's
/// conversion spreads over the workers, where ffmpeg's conversion to rgba
/// upstream of the sidecar held a 4K run to 23 frames a second against 30.
/// On the GPU rgba is first, the decoder's pictures made rgba in one pass.
fn pixel_formats(renderer: Renderer) -> [&'static str; 2] {
    match renderer {
        Renderer::Cpu => [YUV420P, PIXEL_FORMAT],
        _ => [PIXEL_FORMAT, YUV420P],
    }
}

#[derive(Deserialize)]
pub struct Params {
    fps: f64,
    /// The loopback ports the held inputs are given on, one each in
    /// `inputs` order, or one port alone; the host reads them.
    port: Option<Value>,
    lead: f64,
    linger: f64,
    timeout: f64,
    latency: Option<f64>,
    presence: String,
    #[serde(flatten)]
    document: Map<String, Value>,
}

impl Params {
    fn document(&self) -> Result<Document, String> {
        Document::parse(&Value::Object(self.document.clone()).to_string())
    }
}

/// The output's size, when the call gives both sides of the canvas.
fn given_size(doc: &Document) -> Option<(u32, u32)> {
    let (w, h) = (doc.width?, doc.height?);
    Some((w.round() as u32, h.round() as u32))
}

fn seconds_of(pts: i64, base: Rational) -> f64 {
    pts as f64 * (base.num as f64 / base.den as f64)
}

fn positive(seconds: f64) -> Option<f64> {
    (seconds > 0.0).then_some(seconds)
}

/// The GPU the host offers: wgpu over wasi:webgpu. On any other target
/// there is none, so the native tests render on the CPU.
fn instance() -> wgpu::Instance {
    #[cfg(target_arch = "wasm32")]
    {
        wgpu_wasi_webgpu::instance()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::empty(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        })
    }
}

/// The GPU renderer `renderer` asks for, for frames of `out` over pictures
/// of `sizes` handed as bytes: none for `cpu`, and for `auto` when there is
/// no adapter or a picture is too large for it; `gpu` refuses rather than
/// fall back.
fn choose(
    renderer: Renderer,
    out: (u32, u32),
    sizes: &[Option<(u32, u32)>],
    summary: bool,
) -> Result<Option<Gpu>, String> {
    if renderer == Renderer::Cpu {
        return Ok(None);
    }
    let made = match sizes
        .iter()
        .flatten()
        .find(|(w, h)| !Gpu::takes_picture(*w, *h))
    {
        Some((w, h)) => Err(format!(
            "a {w}x{h} picture is larger than the GPU path takes ({MAX_PICTURE} px a side)"
        )),
        None => Gpu::new(&instance(), out),
    };
    match made {
        Ok(gpu) => Ok(Some(gpu)),
        Err(e) if renderer == Renderer::Gpu => Err(format!("renderer => 'gpu': {e}")),
        Err(e) => {
            if summary {
                eprintln!("compose: rendering on the CPU: {e}");
            }
            Ok(None)
        }
    }
}

/// Which frame a picture is: its stream, its pts, and the feed it came on.
type FrameKey = (u32, i64, Option<(i64, i64)>);

/// How a video input's frames are laid out.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Layout {
    Rgba,
    /// Three planes, and how they read as colour.
    Yuv420p(Colour),
}

impl Layout {
    /// Each plane's width, height and texture format, for a picture of
    /// `w` x `h`.
    fn planes(self, (w, h): (u32, u32)) -> Vec<(u32, u32, wgpu::TextureFormat)> {
        match self {
            Layout::Rgba => vec![(w, h, wgpu::TextureFormat::Rgba8Unorm)],
            Layout::Yuv420p(_) => {
                let r8 = wgpu::TextureFormat::R8Unorm;
                let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
                vec![(w, h, r8), (cw, ch, r8), (cw, ch, r8)]
            }
        }
    }
}

pub struct Compose {
    v: Option<u32>,
    /// The streams of `inputs`, input 1 on.
    held: Vec<u32>,
    bases: HashMap<u32, Rational>,
    session: Session,
    presence: Presence,
    /// The stream each presence entry watches.
    watched: Vec<u32>,
    /// One frame of the clock, in its pts, where a frame does not say.
    step: Option<i64>,
    /// Whether the video ports are held on the GPU: `renderer` was `gpu` or
    /// `auto` when the instance opened, which is what `ffrwd:gpu/ports`
    /// said for it.
    on_gpu: bool,
    /// Each input's layout, input 0 on; none for an input not read.
    layouts: Vec<Option<Layout>>,
    /// The picture each input last handed over, kept so that a frame shown
    /// again (a held input repeating while its source is late) is the same
    /// image: not fetched, uploaded or converted again. On the GPU it holds
    /// no bytes, only the size and the blob id the renderer keys it by.
    pictures: Vec<Option<(FrameKey, RasterImageData)>>,
}

fn watched(entries: &[Entry], held: &[u32]) -> Result<Vec<u32>, String> {
    entries
        .iter()
        .map(|entry| {
            let k = entry.input as usize;
            (k >= 1)
                .then(|| held.get(k - 1).copied())
                .flatten()
                .ok_or_else(|| match held.len() {
                    0 => format!("presence names input {k}, and the call holds no input"),
                    n => format!(
                        "presence names input {k}, and the inputs held are ffrwd:1 to ffrwd:{n}"
                    ),
                })
        })
        .collect()
}

/// What a tick shows: the clock's frame and each held input's, and how to
/// reach each one's picture.
struct Shown<'a> {
    tick: &'a Tick<'a>,
    clocked: Option<(u32, Frame)>,
    held: &'a [u32],
    frames: Vec<Option<Frame>>,
    layouts: &'a [Option<Layout>],
    /// Whether pictures are taken as textures rather than fetched.
    textures: bool,
}

impl Shown<'_> {
    /// Input `input`'s stream and frame this tick, if it shows one.
    fn frame(&self, input: u32) -> Option<(u32, Frame)> {
        match input {
            0 => self.clocked.clone(),
            k => self
                .held
                .get(k as usize - 1)
                .copied()
                .zip(self.frames.get(k as usize - 1).cloned().flatten()),
        }
    }

    /// Input `input`'s layout; an error for an input not read.
    fn layout(&self, input: u32) -> Result<Layout, String> {
        self.layouts
            .get(input as usize)
            .copied()
            .flatten()
            .ok_or_else(|| format!("input {input} is not read"))
    }

    /// Frame `index` of stream `id`, input `input`'s picture of `size`,
    /// fetched and made straight rgba. None, said on stderr, when its bytes
    /// are not a picture of that size.
    fn rgba(
        &self,
        input: u32,
        id: u32,
        index: u32,
        size: (u32, u32),
    ) -> Result<Option<Vec<u8>>, String> {
        let layout = self.layout(input)?;
        match rgba_of(self.tick.fetch(id, index), layout, size) {
            Ok(rgba) => Ok(Some(rgba)),
            Err(e) => {
                eprintln!("compose: input {input}: {e}");
                Ok(None)
            }
        }
    }

    /// Input `input`'s picture this tick, `w` x `h`: the one kept in
    /// `pictures` when it is the same frame, fetched (a yuv420p one
    /// converted to rgba) or taken as textures otherwise. None when the
    /// input shows nothing or its bytes are not a picture of that size.
    fn picture(
        &self,
        pictures: &mut [Option<(FrameKey, RasterImageData)>],
        input: u32,
        (w, h): (u32, u32),
    ) -> Result<Option<Picture>, String> {
        let Some((id, frame)) = self.frame(input) else {
            return Ok(None);
        };
        let Some(slot) = pictures.get_mut(input as usize) else {
            return Ok(None);
        };
        let feed = self.tick.feed(id).map(|f| (f.start.at, f.start.first_pts));
        let key = (id, frame.pts, feed);
        let kept = slot
            .as_ref()
            .filter(|(kept, _)| *kept == key)
            .map(|(_, image)| image.clone());
        if !self.textures {
            if let Some(image) = kept {
                return Ok(Some(Picture::Bytes(image)));
            }
            let Some(rgba) = self.rgba(input, id, frame.index, (w, h))? else {
                return Ok(None);
            };
            let image = RasterImageData::new(w, h, Arc::new(rgba));
            *slot = Some((key, image.clone()));
            return Ok(Some(Picture::Bytes(image)));
        }
        let image = match kept {
            Some(image) => image,
            None => {
                let image = RasterImageData::new(w, h, Arc::new(Vec::new()));
                *slot = Some((key, image.clone()));
                image
            }
        };
        let layout = self.layout(input)?;
        let mut planes = frames::planes(id, frame.index, &layout.planes((w, h)))
            .map_err(|e| format!("input {input}: {e}"))?
            .into_iter();
        let mut next = || planes.next().expect("as many planes as the layout has");
        Ok(Some(match layout {
            Layout::Rgba => Picture::Rgba {
                image,
                texture: next(),
            },
            Layout::Yuv420p(colour) => Picture::Yuv420p {
                image,
                planes: [next(), next(), next()],
                colour,
            },
        }))
    }
}

impl Node for Compose {
    const NAME: &'static str = "compose";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const PARAMS_SCHEMA: &'static str = PARAMS_SCHEMA;
    type Params = Params;

    fn shape(params: &Params, bound: &Bound) -> Result<Shape> {
        let doc = params.document()?;
        Entry::parse_list(&params.presence)?;
        let clocked = bound.has("v");
        frames::shaped(clocked);
        let mut held = Input::video("inputs")
            .optional()
            .many()
            .anchor(Anchor::Tagged(TIMED_TAG.to_owned()))
            .lead(params.lead)
            .pixel_formats(&pixel_formats(doc.renderer));
        if let Some(linger) = positive(params.linger) {
            held = held.linger(linger);
        }
        if let Some(timeout) = positive(params.timeout) {
            held = held.timeout(timeout);
        }
        if clocked && (params.port.is_some() || bound.has("inputs")) {
            held = held.port_param("port");
        }
        let mut changes = Input::rows("changes")
            .optional()
            .many()
            .interval()
            .state()
            .schema_json(CHANGES_SCHEMA);
        if let Some(latency) = params.latency {
            changes = changes.latency(latency);
        }
        let size = given_size(&doc);
        let mut shape = Shape::new();
        let video = match size {
            Some((w, h)) => Output::video("video").size(w, h).pixel_format(PIXEL_FORMAT),
            None if clocked => Output::video("video")
                .following("v")
                .pixel_format(PIXEL_FORMAT),
            None => {
                return Err(
                    "a page has no picture to take its size from: give it width and height".into(),
                );
            }
        };
        if clocked {
            shape = shape
                .input(
                    Input::video("v")
                        .clock()
                        .pixel_formats(&pixel_formats(doc.renderer)),
                )
                .output(video)
                .one_to_one();
        } else {
            let (w, h) = size.unwrap_or_default();
            shape = shape
                .rate(Rational::approximate(params.fps, 1001))
                .output(video.row(0))
                .relation_row(&format!(r#"{{"width":{w},"height":{h}}}"#))
                .bounded(false);
        }
        shape = shape.input(held).input(changes);
        Ok(shape.pure())
    }

    fn init(params: Params, init: &Init) -> Result<Compose> {
        let doc = params.document()?;
        let entries = Entry::parse_list(&params.presence)?;
        let v = init.optional("v");
        let held: Vec<&BoundStream> = init.streams("inputs");
        if held.len() as u32 + 1 > MAX_INPUTS {
            return Err(format!(
                "compose reads at most {MAX_INPUTS} pictures, and the call gives {}",
                held.len() + 1
            )
            .into());
        }
        let on_gpu = doc.renderer != Renderer::Cpu;
        let mut sizes = Vec::new();
        let mut layouts = Vec::new();
        for stream in v.iter().chain(held.iter()) {
            let video = stream
                .video_format()
                .ok_or_else(|| format!("`{}` is a video input", stream.port))?;
            let layout = match video.pix_fmt.as_str() {
                PIXEL_FORMAT => Layout::Rgba,
                YUV420P => {
                    let color = video.color.clone().unwrap_or_default();
                    Layout::Yuv420p(Colour::of(&color.space, &color.range))
                }
                other => {
                    return Err(format!(
                        "compose takes rgba or yuv420p pictures, and `{}` is {other}",
                        stream.port
                    )
                    .into());
                }
            };
            sizes.push(Some((video.width, video.height)));
            layouts.push(Some(layout));
        }
        if v.is_none() {
            sizes.insert(0, None);
            layouts.insert(0, None);
        }
        let out = given_size(&doc)
            .or(sizes[0])
            .ok_or("compose has no picture to take its size from: give it width and height")?;
        let held: Vec<u32> = held.iter().map(|s| s.id).collect();
        let watched = watched(&entries, &held)?;
        let bases = init
            .all()
            .iter()
            .map(|s| (s.id, s.info.time_base))
            .collect();
        let step = match v {
            Some(v) => v
                .hint
                .rate
                .map(|rate| v.info.time_base.pts(rate.duration(1))),
            None => Some(1),
        }
        .filter(|step| *step > 0);
        let summary = doc.log != Log::Off;
        let started = Instant::now();
        let renderer = doc.renderer;
        let pictures = vec![None; sizes.len()];
        let mut session = Session::with_sizes(doc, out, sizes.clone());
        // Pictures held on the GPU are drawn from their textures, not the
        // atlas, so their size is no bound.
        let atlas: &[Option<(u32, u32)>] = if on_gpu { &[] } else { &sizes };
        session.set_gpu(choose(renderer, out, atlas, summary)?);
        for e in session.take_errors() {
            eprintln!("compose: row error: {e}");
        }
        if summary {
            let g = *session.compositor().geometry();
            eprintln!(
                "compose: {}x{} rgba, {} picture(s), canvas {}x{}, design {}x{} CSS px, {} parameter rows, {} presence entr{}, {}; document built in {:.1} ms",
                out.0,
                out.1,
                sizes.iter().flatten().count(),
                g.canvas_w,
                g.canvas_h,
                g.css_w,
                g.css_h,
                session.params.rows.len(),
                entries.len(),
                if entries.len() == 1 { "y" } else { "ies" },
                match (session.gpu(), on_gpu) {
                    (Some(gpu), true) =>
                        format!("on the GPU ({}), the video ports held there", gpu.adapter()),
                    (Some(gpu), false) => format!("on the GPU ({})", gpu.adapter()),
                    (None, _) => "on the CPU".to_owned(),
                },
                started.elapsed().as_secs_f64() * 1e3
            );
        }
        Ok(Compose {
            v: v.map(|s| s.id),
            held,
            bases,
            session,
            presence: Presence::new(entries),
            watched,
            step,
            on_gpu,
            layouts,
            pictures,
        })
    }

    fn set_params(&mut self, params: Params) -> Result<()> {
        let doc = params.document()?;
        let entries = Entry::parse_list(&params.presence)?;
        self.watched = watched(&entries, &self.held)?;
        self.presence.set(entries);
        let renderer = doc.renderer;
        if renderer != self.session.params.renderer {
            let summary = doc.log != Log::Off;
            let gpu = match (renderer, self.session.compositor().take_gpu()) {
                (Renderer::Cpu, _) => None,
                (_, Some(gpu)) => Some(gpu),
                (_, None) => {
                    let sizes = self.session.sizes().to_vec();
                    let atlas: &[Option<(u32, u32)>] = if self.on_gpu { &[] } else { &sizes };
                    choose(renderer, self.session.size(), atlas, summary)?
                }
            };
            self.session.set_gpu(gpu);
        }
        self.session.set_params(doc);
        Ok(())
    }

    fn fold(&mut self, row: StateRow) -> Result<()> {
        let base = self.bases.get(&row.id).copied().unwrap_or(Rational::MICROS);
        self.session
            .fold(seconds_of(row.pts, base), &[row.json.to_owned()]);
        Ok(())
    }

    fn process(&mut self, tick: &Tick, out: &mut Out) -> Result<()> {
        let clock = tick.time_base();
        let clocked = match self.v {
            Some(v) => match tick.frame(v) {
                Some(frame) => Some((v, frame)),
                None => return Ok(()),
            },
            None => None,
        };
        let (pts, duration) = match &clocked {
            Some((_, frame)) => (frame.pts, frame.duration),
            None => (tick.pts(), Some(1)),
        };
        let t = seconds_of(pts, clock);
        if !self.watched.is_empty() {
            let step = duration.filter(|d| *d > 0).or(self.step);
            let record = |f: ffrwd_node::Feed| presence::Feed {
                known: f.start.known,
                at: f.start.at,
                first_pts: f.start.first_pts,
                ends: f.ends,
            };
            let seen: Vec<Seen> = self
                .watched
                .iter()
                .map(|id| Seen {
                    current: tick.feed(*id).map(record),
                    ended: tick.ended_feeds(*id).into_iter().map(record).collect(),
                })
                .collect();
            let clock = Clock {
                num: clock.num,
                den: clock.den,
            };
            let rows = self.presence.tick(pts, step, clock, &seen);
            self.session.schedule(rows);
        }

        let showing: Vec<_> = self.held.iter().map(|id| tick.frame(*id)).collect();
        let mut absent = if clocked.is_some() { 0 } else { bit(0) };
        for (k, frame) in showing.iter().enumerate() {
            if frame.is_none() {
                absent |= bit(k as u32 + 1);
            }
        }
        let shown = Shown {
            tick,
            clocked: clocked.clone(),
            held: &self.held,
            frames: showing,
            layouts: &self.layouts,
            textures: self.on_gpu && self.session.gpu().is_some(),
        };
        let pictures = &mut self.pictures;
        let mut failed = None;
        let made = self.session.frame_with(t, absent, &mut |input, size| {
            shown.picture(pictures, input, size).unwrap_or_else(|e| {
                failed.get_or_insert(e);
                None
            })
        });
        if let Some(e) = failed {
            return Err(e.into());
        }
        match made {
            Made::Same(k) if shown.textures && self.layouts[k as usize] != Some(Layout::Rgba) => {
                // A yuv picture cannot leave on the rgba output as it is: it
                // leaves converted, in a texture of its own.
                let size = self.session.sizes()[k as usize].ok_or("a bypassed input is read")?;
                let picture = shown
                    .picture(&mut self.pictures, k, size)?
                    .ok_or("a bypassed input shows a picture")?;
                let frame = self
                    .session
                    .gpu_mut()
                    .and_then(|gpu| gpu.frame_of(k, &picture))
                    .ok_or("a picture held on the GPU is a texture")?;
                frames::emit("video", pts, duration, frame)?;
            }
            Made::Same(k) if self.layouts[k as usize] != Some(Layout::Rgba) => {
                // A yuv picture cannot leave on the rgba output as it is: it
                // leaves converted, and the time is the frame's.
                let started = Instant::now();
                let size = self.session.sizes()[k as usize].ok_or("a bypassed input is read")?;
                let (id, frame) = shown.frame(k).ok_or("a bypassed input shows a picture")?;
                if let Some(rgba) = shown.rgba(k, id, frame.index, size)? {
                    out.frame("video", pts, duration, rgba)?;
                }
                self.session.add_time(started.elapsed().as_secs_f64() * 1e6);
            }
            Made::Same(0) => {
                if let Some((v, frame)) = &clocked {
                    out.pass("video", *v, frame)?;
                }
            }
            Made::Same(k) => {
                let k = k as usize - 1;
                if let (Some(id), Some(Some(frame))) = (self.held.get(k), shown.frames.get(k)) {
                    out.same("video", pts, duration, *id, frame.index)?;
                }
            }
            Made::New(bytes) => out.frame("video", pts, duration, bytes)?,
            Made::Rendered(frame) if self.on_gpu => frames::emit("video", pts, duration, frame)?,
            Made::Rendered(frame) => {
                // The GPU renderer was asked for after the instance opened,
                // with its ports in host memory.
                let mut bytes = Vec::new();
                if let Some(gpu) = self.session.gpu_mut() {
                    gpu.read_back(&frame, &mut bytes);
                }
                out.frame("video", pts, duration, bytes)?;
            }
        }
        for e in self.session.take_errors() {
            eprintln!("compose: row error: {e}");
        }
        if let Some(line) = self.session.frame_line(t) {
            eprintln!("compose {line}");
        }
        if tick.last()
            && let Some(line) = self.session.summary_line()
        {
            eprintln!("compose {line}");
        }
        Ok(())
    }
}

/// ffrwd-frame's name for a colour: the same matrix and range.
fn frame_colour(colour: Colour) -> yuv::Colour {
    yuv::Colour {
        range: if colour.full {
            yuv::Range::Pc
        } else {
            yuv::Range::Tv
        },
        matrix: match colour.matrix {
            Matrix::Bt601 => yuv::Matrix::Bt601,
            Matrix::Bt709 => yuv::Matrix::Bt709,
        },
    }
}

/// `bytes`, a picture of `w` x `h` laid out as `layout` says, as straight
/// rgba: rgba as it is, yuv420p converted. An error when the bytes are not
/// a picture of that size.
fn rgba_of(bytes: Vec<u8>, layout: Layout, (w, h): (u32, u32)) -> Result<Vec<u8>, String> {
    let (w, h) = (w as usize, h as usize);
    match layout {
        Layout::Rgba => {
            ffrwd_frame::Rgba::new(&bytes, w, h)?;
            Ok(bytes)
        }
        Layout::Yuv420p(colour) => {
            let frame = yuv::Yuv420p::new(&bytes, w, h)?;
            let mut rgba = vec![0u8; w * h * 4];
            yuv::to_rgba(&frame, frame_colour(colour), &mut rgba)?;
            Ok(rgba)
        }
    }
}

ffrwd_node::export!(Compose);

#[cfg(test)]
mod tests;
