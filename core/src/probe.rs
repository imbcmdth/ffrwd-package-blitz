//! A paint target that draws nothing and answers two questions about the
//! frame the document would paint: which video inputs appear in it, and
//! whether the picture is exactly one input, untouched.
//!
//! The compositor paints the document into this before it has any pixels:
//! every video element holds a placeholder image whose blob id says which
//! input it stands for. An input whose placeholder is never drawn inside the
//! frame is never fetched, and an input with no picture this frame draws
//! nothing. The frame is an input untouched (the bypass) when the last
//! thing drawn that could show is that input's image, drawn 1:1 over the
//! whole frame, opaque, with no layer between it and the frame that could
//! change it.
//!
//! It is conservative: anything it cannot prove invisible counts as
//! visible, so a document it is unsure of is rendered rather than bypassed.

use std::sync::Arc;

use anyrender::{Filter, Glyph, NormalizedCoord, PaintRef, PaintScene, RenderContext};
use kurbo::{Affine, PathEl, Rect, Shape, Stroke};
use peniko::{BlendMode, Color, Fill, FontData, StyleRef};

/// One entry of the layer stack.
#[derive(Clone, Copy)]
struct Layer {
    /// Composites its contents unchanged over the whole frame: alpha 1,
    /// normal blending, no filter, a clip that holds the frame.
    clear: bool,
    /// Alpha 0: nothing inside shows.
    hidden: bool,
}

pub struct Probe<'a> {
    /// (blob id, input) of each placeholder.
    placeholders: &'a [(u64, u32)],
    /// Bit `i` set: input `i` has no picture this frame.
    absent: u64,
    frame: Rect,
    stack: Vec<Layer>,
    /// Bit `i` set: input `i` is drawn somewhere inside the frame.
    pub drawn: u64,
    /// The input covering the frame, 1:1 and opaque, as last drawn.
    covered: Option<u32>,
    /// Something that could show was drawn after that cover.
    over: bool,
}

const EPS: f64 = 1e-6;

impl<'a> Probe<'a> {
    pub fn new(placeholders: &'a [(u64, u32)], absent: u64, width: u32, height: u32) -> Self {
        Probe {
            placeholders,
            absent,
            frame: Rect::new(0.0, 0.0, width as f64, height as f64),
            stack: Vec::new(),
            drawn: 0,
            covered: None,
            over: false,
        }
    }

    /// The input the frame is, untouched.
    pub fn same(&self) -> Option<u32> {
        self.covered.filter(|_| !self.over)
    }

    fn hidden(&self) -> bool {
        self.stack.iter().any(|l| l.hidden)
    }

    fn clear_path(&self) -> bool {
        self.stack.iter().all(|l| l.clear)
    }

    fn inside(&self, device_bbox: Rect) -> bool {
        let i = device_bbox.intersect(self.frame);
        i.width() > 0.0 && i.height() > 0.0
    }

    /// Something that could show was drawn.
    fn visible(&mut self) {
        if !self.hidden() {
            self.over = true;
        }
    }

    fn placeholder(&self, id: u64) -> Option<u32> {
        self.placeholders
            .iter()
            .find(|(b, _)| *b == id)
            .map(|(_, i)| *i)
    }

    /// The input `brush` shows, and whether it has no picture this frame.
    fn input_of(&self, brush: &PaintRef<'_>) -> Option<(u32, bool)> {
        let PaintRef::Image(img) = brush else {
            return None;
        };
        let i = self.placeholder(img.image.data.id())?;
        Some((i, self.absent & crate::bit(i) != 0))
    }

    /// Whether `shape` under `transform` covers the whole frame.
    fn covers_frame(&self, transform: Affine, shape: &impl Shape) -> bool {
        let Some(r) = rect_of(shape) else {
            return false;
        };
        let [a, b, c, d, _, _] = transform.as_coeffs();
        if b.abs() > EPS || c.abs() > EPS || a <= 0.0 || d <= 0.0 {
            return false;
        }
        let dev = transform.transform_rect_bbox(r);
        dev.x0 <= self.frame.x0 + EPS
            && dev.y0 <= self.frame.y0 + EPS
            && dev.x1 >= self.frame.x1 - EPS
            && dev.y1 >= self.frame.y1 - EPS
    }
}

/// The rectangle `shape` is, when it is one: a `Rect`, a rounded rectangle
/// with no rounding, or a path of straight edges along the axes whose
/// corners are its bounding box's.
fn rect_of(shape: &impl Shape) -> Option<Rect> {
    if let Some(r) = shape.as_rect() {
        return Some(r);
    }
    if let Some(rr) = shape.as_rounded_rect() {
        let r = rr.radii();
        return (r.top_left == 0.0
            && r.top_right == 0.0
            && r.bottom_left == 0.0
            && r.bottom_right == 0.0)
            .then(|| rr.rect());
    }
    let bbox = shape.bounding_box();
    let corner =
        |p: kurbo::Point| (p.x == bbox.x0 || p.x == bbox.x1) && (p.y == bbox.y0 || p.y == bbox.y1);
    let mut last: Option<kurbo::Point> = None;
    let mut first: Option<kurbo::Point> = None;
    let mut corners = 0;
    for el in shape.path_elements(0.1) {
        match el {
            PathEl::MoveTo(p) => {
                if first.is_some() || !corner(p) {
                    return None;
                }
                first = Some(p);
                last = Some(p);
                corners += 1;
            }
            PathEl::LineTo(p) => {
                let l = last?;
                if !corner(p) || (p.x != l.x && p.y != l.y) {
                    return None;
                }
                if p != l {
                    corners += 1;
                }
                last = Some(p);
            }
            PathEl::ClosePath => {}
            _ => return None,
        }
    }
    // The path closes back to its start either way, which is the fourth
    // edge; three or four distinct corner visits make the rectangle.
    let closes = match (first, last) {
        (Some(f), Some(l)) => f.x == l.x || f.y == l.y,
        _ => false,
    };
    (corners >= 4 && closes && bbox.width() > 0.0 && bbox.height() > 0.0).then_some(bbox)
}

fn is_identity(t: Affine) -> bool {
    let [a, b, c, d, e, f] = t.as_coeffs();
    (a - 1.0).abs() < EPS
        && b.abs() < EPS
        && c.abs() < EPS
        && (d - 1.0).abs() < EPS
        && e.abs() < EPS
        && f.abs() < EPS
}

fn transparent(c: &Color) -> bool {
    c.components[3] <= 0.0
}

impl RenderContext for Probe<'_> {}

impl PaintScene for Probe<'_> {
    fn reset(&mut self) {
        self.stack.clear();
        self.drawn = 0;
        self.covered = None;
        self.over = false;
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
        let blend = blend.into();
        if backdrop_filter.is_some() {
            // It changes what is already drawn.
            self.visible();
        }
        let clear = alpha >= 1.0
            && blend == BlendMode::default()
            && filter.is_none()
            && backdrop_filter.is_none()
            && self.covers_frame(transform, clip);
        self.stack.push(Layer {
            clear,
            hidden: alpha <= 0.0,
        });
    }

    fn push_clip_layer(&mut self, transform: Affine, clip: &impl Shape) {
        let clear = self.covers_frame(transform, clip);
        self.stack.push(Layer {
            clear,
            hidden: false,
        });
    }

    fn pop_layer(&mut self) {
        self.stack.pop();
    }

    fn stroke<'b>(
        &mut self,
        style: &Stroke,
        transform: Affine,
        brush: impl Into<PaintRef<'b>>,
        _brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        let bbox = shape.bounding_box().inflate(style.width, style.width);
        if !self.inside(transform.transform_rect_bbox(bbox)) {
            return;
        }
        let brush = brush.into();
        if let PaintRef::Solid(c) = &brush
            && transparent(c)
        {
            return;
        }
        match self.input_of(&brush) {
            Some((_, true)) => return,
            Some((i, false)) => self.drawn |= crate::bit(i),
            None => {}
        }
        self.visible();
    }

    fn fill<'b>(
        &mut self,
        _style: Fill,
        transform: Affine,
        brush: impl Into<PaintRef<'b>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        if !self.inside(transform.transform_rect_bbox(shape.bounding_box())) {
            return;
        }
        let brush = brush.into();
        match &brush {
            PaintRef::Solid(c) if transparent(c) => return,
            PaintRef::Image(img) => match self.input_of(&brush) {
                Some((_, true)) => return,
                Some((i, false)) => {
                    self.drawn |= crate::bit(i);
                    let whole = transform * brush_transform.unwrap_or(Affine::IDENTITY);
                    if !self.hidden()
                        && self.clear_path()
                        && is_identity(whole)
                        && img.sampler.alpha >= 1.0
                        && self.covers_frame(transform, shape)
                    {
                        self.covered = Some(i);
                        self.over = false;
                        return;
                    }
                }
                None => {}
            },
            _ => {}
        }
        self.visible();
    }

    fn draw_glyphs<'b, 's: 'b>(
        &'s mut self,
        _font: &'b FontData,
        font_size: f32,
        _hint: bool,
        _normalized_coords: &'b [NormalizedCoord],
        _embolden: kurbo::Vec2,
        _style: impl Into<StyleRef<'b>>,
        brush: impl Into<PaintRef<'b>>,
        brush_alpha: f32,
        transform: Affine,
        glyph_transform: Option<Affine>,
        glyphs: impl Iterator<Item = Glyph> + Clone,
    ) {
        if brush_alpha <= 0.0 {
            return;
        }
        if let PaintRef::Solid(c) = &brush.into()
            && transparent(c)
        {
            return;
        }
        let mut bbox: Option<Rect> = None;
        for g in glyphs {
            let p = Rect::new(g.x as f64, g.y as f64, g.x as f64, g.y as f64);
            bbox = Some(bbox.map_or(p, |b| b.union(p)));
        }
        let Some(bbox) = bbox else { return };
        // Glyph origins sit on the baseline; a glyph reaches about one em
        // either way of its origin, more under a glyph transform.
        let reach = font_size as f64 * if glyph_transform.is_some() { 4.0 } else { 2.0 };
        let bbox = bbox.inflate(reach, reach);
        if self.inside(transform.transform_rect_bbox(bbox)) {
            self.visible();
        }
    }

    fn draw_box_shadow(
        &mut self,
        transform: Affine,
        rect: Rect,
        brush: Color,
        radius: f64,
        std_dev: f64,
    ) {
        if transparent(&brush) {
            return;
        }
        let reach = radius + 3.0 * std_dev;
        if self.inside(transform.transform_rect_bbox(rect.inflate(reach, reach))) {
            self.visible();
        }
    }
}
