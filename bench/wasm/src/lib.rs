//! Step C of the 4K60 compositing experiment: the harness as a source node
//! in the shape of `page`. No inputs; N pictures it makes itself; one row a
//! frame on the run's rows output (`[@rows=...]`), the meta line first and
//! the end line last; the picture on `video` only when the query maps it,
//! since emitting it means a readback. It ends itself after `frames`.
//!
//! Built for wasm32-wasip2 it renders through `wgpu-wasi-webgpu`, so it
//! needs a `-gpu` grant under `ffrwd-wasm`. On the native target it builds
//! (so the workspace checks) but has no adapter to find.

use std::time::Instant;

use blitz_bench_doc::{self as doc, End, Variant};
use blitz_bench_harness::{Config, Harness};
use ffrwd_node::{Bound, Init, Node, Out, Output, Rational, Result, Shape, Tick};
use serde::Deserialize;

const PARAMS_SCHEMA: &str = r#"{"type":"object","properties":{"inputs":{"type":"integer","minimum":1,"maximum":64,"default":2},"variant":{"type":"integer","minimum":1,"maximum":4,"default":1},"all_4k":{"type":"boolean","default":false},"frames":{"type":"integer","minimum":1,"default":600},"fps":{"type":"number","exclusiveMinimum":0,"maximum":240,"default":60},"width":{"type":"integer","minimum":1,"default":3840},"height":{"type":"integer","minimum":1,"default":2160}},"additionalProperties":false}"#;

/// The rows are the meta line, one `Row` a frame and the end line: three
/// shapes on one stream, so the schema only asks for an object.
const ROWS_SCHEMA: &str = r#"{"type":"object"}"#;

#[derive(Deserialize)]
pub struct Params {
    inputs: u32,
    variant: u8,
    all_4k: bool,
    frames: u32,
    fps: f64,
    width: u32,
    height: u32,
}

fn instance() -> wgpu::Instance {
    #[cfg(target_arch = "wasm32")]
    {
        wgpu_wasi_webgpu::instance()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle())
    }
}

pub struct Bench {
    harness: Harness,
    frames: u32,
    next: u32,
    started: Instant,
    want_pixels: bool,
    ended: bool,
}

impl Node for Bench {
    const NAME: &'static str = "bench-compose";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const PARAMS_SCHEMA: &'static str = PARAMS_SCHEMA;
    const ROWS_SCHEMA: &'static str = ROWS_SCHEMA;
    type Params = Params;

    fn shape(params: &Params, _bound: &Bound) -> Result<Shape> {
        let (w, h) = (params.width, params.height);
        Ok(Shape::new()
            .rate(Rational::approximate(params.fps, 1001))
            .output(Output::video("video").size(w, h).pixel_format("rgba").row(0))
            .relation_row(&format!(r#"{{"width":{w},"height":{h}}}"#))
            // One instance: a bench of the single-threaded guest, not spread
            // over workers.
            .bounded(false))
    }

    fn init(params: Params, init: &Init) -> Result<Bench> {
        let variant = Variant::from_number(params.variant)
            .ok_or_else(|| format!("variant is 1 to 4, got {}", params.variant))?;
        if (params.width, params.height) != doc::OUT {
            return Err(format!(
                "the experiment renders {}x{}; width and height are fixed",
                doc::OUT.0,
                doc::OUT.1
            )
            .into());
        }
        let config = Config {
            n: params.inputs,
            variant,
            all_4k: params.all_4k,
            frames: params.frames,
            step: "C".into(),
        };
        let instance = instance();
        let harness = Harness::new(&instance, config, Some("wasi:webgpu"));
        Ok(Bench {
            harness,
            frames: params.frames,
            next: 0,
            started: Instant::now(),
            want_pixels: init.latched("video"),
            ended: false,
        })
    }

    fn process(&mut self, tick: &Tick, out: &mut Out) -> Result<()> {
        if self.ended {
            return Ok(());
        }
        // The tick number is the frame: the clock is the rate.
        let frame = u32::try_from(tick.pts()).unwrap_or(u32::MAX);
        if frame != self.next {
            return Err(format!("tick {frame} where frame {} was expected", self.next).into());
        }
        if frame == 0 {
            let setup = wgpu_wasi_webgpu::take_wit_bytes();
            let mut meta = self.harness.meta().clone();
            meta.os = "wasm32-wasip2 under ffrwd-wasm".into();
            meta.notes.push(format!(
                "setup crossed the wit: {} bytes up, {} down",
                setup.up, setup.down
            ));
            out.report(&meta)?;
            self.started = Instant::now();
        }
        if frame >= self.frames {
            let end = End {
                kind: "end".into(),
                frames: self.frames,
                wall_us: self.started.elapsed().as_secs_f64() * 1e6,
                peak_rss_bytes: None,
            };
            out.report(&end)?;
            out.finish();
            self.ended = true;
            return Ok(());
        }
        let (mut row, pixels) = self.harness.frame(frame, self.want_pixels);
        let wit = wgpu_wasi_webgpu::take_wit_bytes();
        row.wit_bytes = Some(wit.up + wit.down);
        out.report(&row)?;
        if let Some(pixels) = pixels {
            out.frame("video", tick.pts(), Some(1), pixels)?;
        }
        self.next += 1;
        Ok(())
    }
}

ffrwd_node::export!(Bench);
