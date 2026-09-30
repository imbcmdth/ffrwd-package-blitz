//! An HTML/CSS document rendered by Blitz (blitz-dom with Stylo, Taffy and
//! Parley, painted by blitz-paint into anyrender_vello_cpu) over video
//! frames.
//!
//! The video inputs appear in the document as `<img src="ffrwd:N">` or as
//! `background-image: url(ffrwd:N)`, N the input's index from 0. Before each
//! frame is painted, every such element is handed that input's pixels. Time
//! comes from the caller (the frame's time in seconds) and is handed to
//! `resolve`, which is what Stylo's animation and transition engine samples.

pub mod changes;
pub mod geometry;
pub mod net;
pub mod params;
pub mod placed;
pub mod probe;
pub mod session;

use std::sync::Arc;
use std::time::Instant;

use anyrender::{ImageRenderer, PaintScene};
use anyrender_vello_cpu::{ImageCacheConfig, VelloCpuImageRenderer};
use blitz_dom::node::{ImageData, RasterImageData, SpecialElementData, Status};
use blitz_dom::{DocumentConfig, FontContext, NodeId, decode_font_bytes, local_name};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use kurbo::{Affine, Rect};
use parley::fontique::{Blob, Collection, CollectionOptions, GenericFamily, SourceCache};
use peniko::{Color, Fill};

pub use changes::{Row, Source, Timeline};
pub use geometry::{Fit, Geometry, Placement};
pub use params::Params;
pub use session::{Output, Session};

/// DejaVu Sans, the font every generic family resolves to: the sidecar has
/// no system fonts. Its license is in `assets/DejaVu-LICENSE`.
const DEJAVU_SANS: &[u8] = include_bytes!("../assets/DejaVuSans.woff2");

fn font_context() -> FontContext {
    let mut ctx = FontContext {
        source_cache: SourceCache::new_shared(),
        collection: Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        }),
    };
    let bytes = decode_font_bytes(DEJAVU_SANS).into_owned();
    let registered = ctx
        .collection
        .register_fonts(Blob::new(Arc::new(bytes) as _), None);
    let ids: Vec<_> = registered.iter().map(|(id, _)| *id).collect();
    for generic in [
        GenericFamily::SansSerif,
        GenericFamily::Serif,
        GenericFamily::Monospace,
        GenericFamily::Cursive,
        GenericFamily::Fantasy,
        GenericFamily::SystemUi,
        GenericFamily::UiSansSerif,
        GenericFamily::UiSerif,
        GenericFamily::UiMonospace,
        GenericFamily::UiRounded,
        GenericFamily::Emoji,
        GenericFamily::Math,
        GenericFamily::FangSong,
    ] {
        ctx.collection
            .append_generic_families(generic, ids.iter().copied());
    }
    ctx
}

/// The video input an `ffrwd:N` URL names.
pub fn input_of(url: &str) -> Option<u32> {
    let rest = url.trim().strip_prefix("ffrwd:")?;
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    let rest = rest.trim_end_matches('/');
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

/// Where a video input is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    /// `<img src="ffrwd:N">`.
    Img,
    /// The element's `background-image` layer at this index.
    Background(usize),
}

#[derive(Clone, Copy, Debug)]
struct VideoRef {
    node: NodeId,
    slot: Slot,
    input: u32,
}

/// What the frame the document would paint needs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Bit `i`: input `i` is drawn somewhere inside the frame.
    pub drawn: u64,
    /// The frame is input 0's picture, untouched.
    pub bypass: bool,
}

impl Plan {
    pub fn inputs(&self) -> impl Iterator<Item = u32> + '_ {
        (0..64u32).filter(|i| self.drawn & (1 << i) != 0)
    }
}

pub struct Compositor {
    doc: HtmlDocument,
    renderer: VelloCpuImageRenderer,
    geometry: Geometry,
    place: Placement,
    inputs: u32,
    /// One placeholder per input, all sharing one buffer of zeros: what a
    /// video element holds until the frame's pixels are fetched. Its blob id
    /// is how the probe tells which input a draw is.
    placeholders: Vec<RasterImageData>,
    placeholder_ids: Vec<(u64, u32)>,
    refs: Vec<VideoRef>,
    /// The rendered frame, RGBA8 (premultiplied, which is straight RGBA
    /// wherever the page is opaque).
    pub out: Vec<u8>,
}

impl Compositor {
    /// A document `html` placed in the output frame as `geometry` says,
    /// reading `inputs` video inputs.
    pub fn new(html: &str, geometry: Geometry, inputs: u32) -> Self {
        let place = geometry.placement();
        let (width, height) = (geometry.out_w, geometry.out_h);
        let renderer = VelloCpuImageRenderer::with_image_cache_config(
            width,
            height,
            // Every frame's image is new: evict the previous one each frame.
            ImageCacheConfig {
                max_age: 1,
                max_bytes: 64 * 1024 * 1024,
                prune_interval: 1,
            },
        );
        let config = DocumentConfig {
            viewport: Some(Viewport::new(
                place.view_w,
                place.view_h,
                place.scale as f32,
                ColorScheme::Light,
            )),
            font_ctx: Some(font_context()),
            // `html` change rows go through the mutator's `set_inner_html`,
            // which parses with this provider (the default one does nothing).
            html_parser_provider: Some(Arc::new(HtmlProvider)),
            // `data:` URIs, decoded synchronously; everything else refused.
            net_provider: Some(Arc::new(net::DataUriProvider)),
            ..Default::default()
        };
        let doc = HtmlDocument::from_html(html, config);
        let (iw, ih) = (geometry.in_w, geometry.in_h);
        let zeros = Arc::new(vec![0u8; (iw as usize) * (ih as usize) * 4]);
        let placeholders: Vec<RasterImageData> = (0..inputs)
            .map(|_| RasterImageData::new(iw, ih, zeros.clone()))
            .collect();
        let placeholder_ids = placeholders
            .iter()
            .enumerate()
            .map(|(i, p)| (p.data.id(), i as u32))
            .collect();
        let mut c = Compositor {
            doc,
            renderer,
            geometry,
            place,
            inputs,
            placeholders,
            placeholder_ids,
            refs: Vec::new(),
            out: vec![0; (width as usize) * (height as usize) * 4],
        };
        // Before the first layout, so an `<img>` with no CSS size is laid
        // out at the input's size.
        c.find_video();
        c.install_placeholders();
        c
    }

    /// The output frame's width, px.
    pub fn width(&self) -> u32 {
        self.geometry.out_w
    }

    /// The output frame's height, px.
    pub fn height(&self) -> u32 {
        self.geometry.out_h
    }

    pub fn geometry(&self) -> &Geometry {
        &self.geometry
    }

    pub fn placement(&self) -> &Placement {
        &self.place
    }

    /// Device pixels per CSS pixel.
    pub fn scale(&self) -> f64 {
        self.place.scale
    }

    pub fn doc(&mut self) -> &mut HtmlDocument {
        &mut self.doc
    }

    pub fn doc_ref(&self) -> &HtmlDocument {
        &self.doc
    }

    /// Applies one change row. Video elements a row inserted get their
    /// placeholder straight away, before the resolve that lays them out.
    pub fn apply(&mut self, row: &Row) -> Result<changes::Applied, String> {
        let r = changes::apply(&mut self.doc, row);
        self.find_video();
        self.install_placeholders();
        r
    }

    /// Restyle and relayout at `time` seconds.
    pub fn resolve(&mut self, time: f64) {
        self.doc.resolve(time);
        // A resolve builds background layers from style, so an element may
        // have gained or lost an `ffrwd:` background.
        self.find_video();
        self.install_placeholders();
    }

    /// Every element that shows a video input, found afresh.
    fn find_video(&mut self) {
        self.refs.clear();
        for (id, node) in self.doc.tree().iter() {
            let Some(el) = node.element_data() else {
                continue;
            };
            if el.name.local == local_name!("img")
                && let Some(input) = el.attr(local_name!("src")).and_then(input_of)
            {
                self.refs.push(VideoRef {
                    node: id,
                    slot: Slot::Img,
                    input,
                });
            }
            for (idx, layer) in el.background_images.iter().enumerate() {
                if let Some(layer) = layer
                    && layer.url.scheme() == "ffrwd"
                    && let Some(input) = input_of(layer.url.as_str())
                {
                    self.refs.push(VideoRef {
                        node: id,
                        slot: Slot::Background(idx),
                        input,
                    });
                }
            }
        }
    }

    fn set_image(&mut self, r: VideoRef, image: Option<&RasterImageData>) {
        let Some(node) = self.doc.get_node_mut(r.node) else {
            return;
        };
        let Some(el) = node.element_data_mut() else {
            return;
        };
        match r.slot {
            Slot::Img => {
                el.special_data = match image {
                    Some(i) => SpecialElementData::Image(Box::new(ImageData::Raster(i.clone()))),
                    None => SpecialElementData::None,
                };
            }
            Slot::Background(idx) => {
                if let Some(Some(layer)) = el.background_images.get_mut(idx) {
                    match image {
                        Some(i) => {
                            layer.status = Status::Ok;
                            layer.image = ImageData::Raster(i.clone());
                        }
                        None => {
                            layer.status = Status::Error;
                            layer.image = ImageData::None;
                        }
                    }
                }
            }
        }
    }

    /// Every video element gets its input's placeholder; one naming an input
    /// this instance does not read gets nothing, and draws nothing.
    fn install_placeholders(&mut self) {
        for i in 0..self.refs.len() {
            let r = self.refs[i];
            let p = self.placeholders.get(r.input as usize).cloned();
            self.set_image(r, p.as_ref());
        }
    }

    /// What the frame, as last resolved, needs: which inputs are drawn, and
    /// whether it is input 0 untouched. Paints the document into a probe
    /// that draws nothing; no pixels are needed.
    pub fn plan(&mut self) -> Plan {
        self.install_placeholders();
        let (w, h) = (self.geometry.out_w, self.geometry.out_h);
        let place = self.place;
        let mut probe = probe::Probe::new(&self.placeholder_ids, w, h);
        probe.reset();
        paint_placed(&mut probe, &mut self.doc, &place, w, h);
        Plan {
            drawn: probe.drawn,
            bypass: probe.bypass(),
        }
    }

    /// Hands every element showing input `input` a new picture (straight
    /// RGBA8 at the frame's size). The elements' boxes do not change, so no
    /// damage is needed: paint reads the image data every frame.
    pub fn set_frame(&mut self, input: u32, rgba: Arc<Vec<u8>>) {
        let image = RasterImageData::new(self.geometry.in_w, self.geometry.in_h, rgba);
        for i in 0..self.refs.len() {
            let r = self.refs[i];
            if r.input == input {
                self.set_image(r, Some(&image));
            }
        }
    }

    /// Paints the document into `self.out`, then puts the placeholders back
    /// so the frames' buffers are released. Returns (paint_cmds_us,
    /// raster_us).
    pub fn paint(&mut self) -> (f64, f64) {
        let (w, h) = (self.geometry.out_w, self.geometry.out_h);
        let place = self.place;
        let doc = &mut self.doc;
        let mut cmds = 0.0;
        let t_all = Instant::now();
        self.renderer.render(
            |scene| {
                let t = Instant::now();
                scene.reset();
                paint_placed(scene, doc, &place, w, h);
                cmds = t.elapsed().as_secs_f64() * 1e6;
            },
            &mut self.out,
        );
        let total = t_all.elapsed().as_secs_f64() * 1e6;
        self.install_placeholders();
        (cmds, total - cmds)
    }

    /// How many video inputs this instance reads.
    pub fn inputs(&self) -> u32 {
        self.inputs
    }

    /// The border box of the first element matching `selector`, in CSS px:
    /// (x, y, w, h).
    pub fn rect_of(&self, selector: &str) -> Option<(f32, f32, f32, f32)> {
        let id = self.doc.query_selector(selector).ok()??;
        let node = self.doc.get_node(id)?;
        let o = node.absolute_position(0.0, 0.0);
        let s = node.final_layout().size;
        Some((o.x, o.y, s.width, s.height))
    }

    /// The computed opacity of the first element matching `selector`.
    pub fn opacity_of(&self, selector: &str) -> Option<f32> {
        let id = self.doc.query_selector(selector).ok()??;
        let node = self.doc.get_node(id)?;
        let s = node.primary_styles()?;
        Some(s.get_effects().opacity)
    }
}

/// Paints the document where `place` puts it in a `w` x `h` frame: bars of
/// black outside the design's rectangle, and the document clipped to it.
fn paint_placed(
    scene: &mut impl PaintScene,
    doc: &mut HtmlDocument,
    place: &Placement,
    w: u32,
    h: u32,
) {
    let frame = Rect::new(0.0, 0.0, w as f64, h as f64);
    let whole = place.view_rect.contains_rect(frame);
    if !whole {
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::from_rgba8(0, 0, 0, 255),
            None,
            &frame,
        );
        scene.push_clip_layer(Affine::IDENTITY, &place.view_rect);
    }
    if place.transform == Affine::IDENTITY {
        blitz_paint::paint_scene(scene, doc, place.scale, place.view_w, place.view_h, 0, 0);
    } else {
        let mut placed = placed::Placed {
            inner: scene,
            transform: place.transform,
        };
        blitz_paint::paint_scene(
            &mut placed,
            doc,
            place.scale,
            place.view_w,
            place.view_h,
            0,
            0,
        );
    }
    if !whole {
        scene.pop_layer();
    }
}

#[cfg(test)]
mod tests {
    use super::input_of;

    #[test]
    fn ffrwd_urls_name_an_input() {
        assert_eq!(input_of("ffrwd:0"), Some(0));
        assert_eq!(input_of("ffrwd:2"), Some(2));
        assert_eq!(input_of(" ffrwd:1 "), Some(1));
        assert_eq!(input_of("ffrwd://1"), Some(1));
        assert_eq!(input_of("ffrwd:"), None);
        assert_eq!(input_of("ffrwd:x"), None);
        assert_eq!(input_of("data:image/png;base64,AAAA"), None);
    }
}
