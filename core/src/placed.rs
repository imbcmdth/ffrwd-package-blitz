//! A paint target that places everything painted into it: each command's
//! transform is prefixed with one fixed transform on its way to the target
//! underneath. Blitz paints the viewport at its own origin; this puts it
//! where the geometry says, stretched where it says.

use std::any::Any;
use std::sync::Arc;

use anyrender::{
    Filter, Glyph, NormalizedCoord, PaintRef, PaintScene, RegisterResourceError, RenderContext,
    ResourceId,
};
use kurbo::{Affine, Rect, Shape, Stroke};
use peniko::{BlendMode, Color, Fill, FontData, StyleRef};

pub struct Placed<'a, S: PaintScene> {
    pub inner: &'a mut S,
    pub transform: Affine,
}

impl<S: PaintScene> RenderContext for Placed<'_, S> {
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

impl<S: PaintScene> PaintScene for Placed<'_, S> {
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
        self.inner.push_layer(
            blend,
            alpha,
            self.transform * transform,
            clip,
            filter,
            backdrop_filter,
        )
    }

    fn push_clip_layer(&mut self, transform: Affine, clip: &impl Shape) {
        self.inner.push_clip_layer(self.transform * transform, clip)
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
        self.inner.stroke(
            style,
            self.transform * transform,
            brush,
            brush_transform,
            shape,
        )
    }

    fn fill<'b>(
        &mut self,
        style: Fill,
        transform: Affine,
        brush: impl Into<PaintRef<'b>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        self.inner.fill(
            style,
            self.transform * transform,
            brush,
            brush_transform,
            shape,
        )
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
        let t = self.transform * transform;
        self.inner.draw_glyphs(
            font,
            font_size,
            hint,
            normalized_coords,
            embolden,
            style,
            brush,
            brush_alpha,
            t,
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
            .draw_box_shadow(self.transform * transform, rect, brush, radius, std_dev)
    }
}
