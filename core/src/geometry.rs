//! Where the document lands in the output frame.
//!
//! Three sizes are involved:
//!
//! - the output frame, in pixels (today always the size of input 0: the
//!   host refuses an output frame of any other length);
//! - the canvas, in pixels (`width` x `height`, the output's size by
//!   default): the frame the document is composed for;
//! - the design, in CSS px (`css_width` x `css_height`, the canvas's size by
//!   default): the document's viewport.
//!
//! The design is fitted into the canvas (uniformly and centred, or
//! stretched), and the canvas into the output frame (uniformly and
//! centred). Whatever of the output the design does not cover is black. The
//! two fits compose into one: Blitz lays the document out at the design
//! size and paints it at a device scale, and a transform places the result.

use kurbo::{Affine, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Fit {
    /// Uniform scale, centred; bars where the aspects differ.
    #[default]
    Contain,
    /// Each axis scaled on its own to fill the canvas.
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    /// Output frame, px.
    pub out_w: u32,
    pub out_h: u32,
    /// Input frames, px.
    pub in_w: u32,
    pub in_h: u32,
    /// Canvas, px.
    pub canvas_w: f64,
    pub canvas_h: f64,
    /// Design, CSS px.
    pub css_w: f64,
    pub css_h: f64,
    pub fit: Fit,
}

/// What painting needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// Blitz's device scale: pixels per CSS px (the smaller axis's when
    /// stretched).
    pub scale: f64,
    /// The viewport Blitz lays out and paints, in pixels at `scale`.
    pub view_w: u32,
    pub view_h: u32,
    /// From viewport pixels to output pixels.
    pub transform: Affine,
    /// The design's rectangle in the output, px.
    pub view_rect: Rect,
}

impl Geometry {
    /// The output and the inputs at one size, the canvas and the design
    /// following it.
    pub fn same(width: u32, height: u32) -> Geometry {
        Geometry {
            out_w: width,
            out_h: height,
            in_w: width,
            in_h: height,
            canvas_w: width as f64,
            canvas_h: height as f64,
            css_w: width as f64,
            css_h: height as f64,
            fit: Fit::Contain,
        }
    }

    /// Settles the canvas and the design from what was asked for: a size
    /// left out follows the output, and a design with only one side given
    /// takes the canvas's aspect for the other.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        out: (u32, u32),
        input: (u32, u32),
        canvas: (Option<f64>, Option<f64>),
        css: (Option<f64>, Option<f64>),
        fit: Fit,
    ) -> Geometry {
        let (ow, oh) = (out.0 as f64, out.1 as f64);
        let (cw, ch) = match canvas {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * oh / ow),
            (None, Some(h)) => (h * ow / oh, h),
            (None, None) => (ow, oh),
        };
        let (dw, dh) = match css {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * ch / cw),
            (None, Some(h)) => (h * cw / ch, h),
            (None, None) => (cw, ch),
        };
        Geometry {
            out_w: out.0,
            out_h: out.1,
            in_w: input.0,
            in_h: input.1,
            canvas_w: cw,
            canvas_h: ch,
            css_w: dw,
            css_h: dh,
            fit,
        }
    }

    pub fn placement(&self) -> Placement {
        let (ow, oh) = (self.out_w as f64, self.out_h as f64);
        let (cw, ch) = (self.canvas_w, self.canvas_h);
        let (dw, dh) = (self.css_w, self.css_h);
        // The canvas in the output: uniform, centred.
        let s2 = (ow / cw).min(oh / ch);
        let o2 = ((ow - cw * s2) / 2.0, (oh - ch * s2) / 2.0);
        // The design in the canvas.
        let (sx1, sy1) = match self.fit {
            Fit::Contain => {
                let s = (cw / dw).min(ch / dh);
                (s, s)
            }
            Fit::Stretch => (cw / dw, ch / dh),
        };
        let (sx, sy) = (sx1 * s2, sy1 * s2);
        let scale = sx.min(sy);
        let view_w = (dw * scale).round().max(1.0) as u32;
        let view_h = (dh * scale).round().max(1.0) as u32;
        // The design's rectangle in the output, on whole pixels.
        let (vw, vh) = (dw * sx, dh * sy);
        let x0 = (o2.0 + (cw * s2 - vw) / 2.0).round();
        let y0 = (o2.1 + (ch * s2 - vh) / 2.0).round();
        let stretch = Affine::scale_non_uniform(sx / scale, sy / scale);
        let transform = Affine::translate((x0, y0)) * stretch;
        let view_rect = Rect::new(
            x0,
            y0,
            x0 + (view_w as f64 * sx / scale).round(),
            y0 + (view_h as f64 * sy / scale).round(),
        );
        Placement {
            scale,
            view_w,
            view_h,
            transform,
            view_rect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_size_is_identity() {
        let p = Geometry::same(1920, 1080).placement();
        assert_eq!(p.scale, 1.0);
        assert_eq!((p.view_w, p.view_h), (1920, 1080));
        assert_eq!(p.transform, Affine::IDENTITY);
        assert_eq!(p.view_rect, Rect::new(0.0, 0.0, 1920.0, 1080.0));
    }

    #[test]
    fn a_design_width_scales_the_document() {
        for (w, h, s) in [(1280, 720, 1.0), (1920, 1080, 1.5), (3840, 2160, 3.0)] {
            let g = Geometry::new(
                (w, h),
                (w, h),
                (None, None),
                (Some(1280.0), None),
                Fit::Contain,
            );
            assert_eq!((g.css_w, g.css_h), (1280.0, 720.0));
            let p = g.placement();
            assert_eq!(p.scale, s);
            assert_eq!((p.view_w, p.view_h), (w, h));
            assert_eq!(p.transform, Affine::IDENTITY);
        }
    }

    #[test]
    fn a_portrait_canvas_in_a_landscape_output_is_pillarboxed() {
        let g = Geometry::new(
            (1920, 1080),
            (1920, 1080),
            (Some(1080.0), Some(1920.0)),
            (None, None),
            Fit::Contain,
        );
        let p = g.placement();
        assert_eq!(p.scale, 1080.0 / 1920.0);
        assert_eq!((p.view_w, p.view_h), (608, 1080));
        assert_eq!(p.view_rect, Rect::new(656.0, 0.0, 1264.0, 1080.0));
    }

    #[test]
    fn a_landscape_design_in_a_square_canvas_is_letterboxed() {
        let g = Geometry::new(
            (1080, 1080),
            (1920, 1080),
            (None, None),
            (Some(1920.0), Some(1080.0)),
            Fit::Contain,
        );
        let p = g.placement();
        assert_eq!(p.scale, 1080.0 / 1920.0);
        assert_eq!((p.view_w, p.view_h), (1080, 608));
        assert_eq!(p.view_rect, Rect::new(0.0, 236.0, 1080.0, 844.0));
    }

    #[test]
    fn stretch_fills_the_canvas() {
        let g = Geometry::new(
            (1920, 1080),
            (1920, 1080),
            (None, None),
            (Some(1000.0), Some(1000.0)),
            Fit::Stretch,
        );
        let p = g.placement();
        assert_eq!(p.scale, 1.08);
        assert_eq!((p.view_w, p.view_h), (1080, 1080));
        assert_eq!(p.view_rect, Rect::new(0.0, 0.0, 1920.0, 1080.0));
    }
}
