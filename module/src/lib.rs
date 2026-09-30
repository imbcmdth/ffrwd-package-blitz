//! The window-filter shim every compose module shares.
//!
//! A module's describe takes no parameters, so how many video inputs it
//! reads, whether it acts on rows and whether it may run frame-parallel are
//! fixed per wasm file. Each of the package's modules is a [`Shape`] and one
//! line of [`compose_module!`]; the work is the same for all of them and
//! lives in `compose_core::Session`.
//!
//! Frames arrive rgba (the host's swscale converts on both sides, where the
//! stream's own range and matrix are known) and leave rgba, one out per
//! frame in, at the frame's own pts.

wit_bindgen::generate!({
    path: "wit",
    world: "window-module",
    pub_export_macro: true,
    export_macro_name: "export_window_filter",
    default_bindings_module: "compose_module",
});

use std::cell::RefCell;

pub use compose_core;
use compose_core::params::{Params, SCHEMA};
use compose_core::{Output, Session};
use exports::ffrwd::av::window_filter::{
    Format, FramePayload, InWindow, Meta, OutFrame, Processed, StreamInfo, WindowMeta,
};

pub use exports::ffrwd::av::window_filter::Guest;

/// The one pixel format in and out.
pub const PIXEL_FORMAT: &str = "rgba";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What a module declares about itself.
pub struct Shape {
    /// The export name, which a declaration's `AS 'path', '<name>'` repeats.
    pub name: &'static str,
    /// Video inputs, one frame off each per call, all at one pts.
    pub inputs: u32,
    /// Whether it acts on the rows arriving with pad 0's frames. Such a
    /// module keeps what the rows did from one frame to the next, so it is
    /// never pure.
    pub reads_rows: bool,
}

impl Shape {
    /// Pure when nothing but the parameters drives the document: every
    /// worker then builds the same document, resolves it at the same epoch
    /// and applies the same rows at the same times, so any worker renders
    /// any frame the same.
    pub const fn pure(&self) -> bool {
        !self.reads_rows
    }
}

struct Instance {
    session: Session,
    width: u32,
    height: u32,
    /// Seconds per timestamp tick.
    tick: f64,
}

thread_local! {
    static INSTANCE: RefCell<Option<Instance>> = const { RefCell::new(None) };
}

pub fn describe(shape: &Shape) -> WindowMeta {
    WindowMeta {
        meta: Meta {
            name: shape.name.to_string(),
            version: VERSION.to_string(),
            params_schema: SCHEMA.to_string(),
            // The rows it emits: none. What `compose` reads is declared on
            // the call's annotation column (src/blitz.sql).
            rows_schema: String::new(),
            pixel_formats: vec![PIXEL_FORMAT.to_string()],
            sample_formats: vec![],
            sample_rates: vec![],
            channel_counts: vec![],
            rows_language: vec![],
        },
        window: 1,
        stride: 1,
        pure: shape.pure(),
        one_to_one: true,
        reads_rows: shape.reads_rows,
        forwards_rows: false,
        inputs: shape.inputs,
        feeders: vec![],
    }
}

pub fn init(
    shape: &Shape,
    format: Format,
    stream_info: StreamInfo,
    params: String,
) -> Result<(), String> {
    let Format::Video(video) = format else {
        return Err(format!(
            "{} composes video, and was opened for audio",
            shape.name
        ));
    };
    if video.pix_fmt != PIXEL_FORMAT {
        return Err(format!(
            "{} takes {PIXEL_FORMAT} frames, got {}",
            shape.name, video.pix_fmt
        ));
    }
    let p = Params::parse(&params)?;
    let tb = stream_info.time_base;
    let tick = tb.num as f64 / tb.den as f64;
    let summary = p.log != compose_core::params::Log::Off;
    let t0 = std::time::Instant::now();
    // The host hands every pad at one size and refuses an output frame of
    // any other: the output is the inputs' size, and the canvas is fitted
    // into it.
    let size = (video.width, video.height);
    let mut session = Session::new(p, size, size, shape.inputs);
    for e in session.take_errors() {
        eprintln!("{}: row error: {e}", shape.name);
    }
    if summary {
        let geometry = *session.compositor().geometry();
        eprintln!(
            "{}: {}x{} rgba, {} input(s), canvas {}x{}, design {}x{} CSS px, {} parameter rows; document built in {:.1} ms",
            shape.name,
            video.width,
            video.height,
            shape.inputs,
            geometry.canvas_w,
            geometry.canvas_h,
            geometry.css_w,
            geometry.css_h,
            session.params.changes.len(),
            t0.elapsed().as_secs_f64() * 1e3
        );
    }
    INSTANCE.with_borrow_mut(|i| {
        *i = Some(Instance {
            session,
            width: video.width,
            height: video.height,
            tick,
        })
    });
    Ok(())
}

pub fn set_params(params: String) -> Result<(), String> {
    let p = Params::parse(&params)?;
    INSTANCE.with_borrow_mut(|i| match i.as_mut() {
        Some(inst) => {
            inst.session.set_params(p);
            Ok(())
        }
        None => Err("set_params before init".into()),
    })
}

pub fn process(shape: &Shape, window: &InWindow, last: bool) -> Processed {
    INSTANCE.with_borrow_mut(|i| {
        let inst = i.as_mut().expect("process before init");
        let mut frames = Vec::new();
        if window.len() > 0 {
            let pts = window.pts(0);
            let t = pts as f64 * inst.tick;
            if shape.reads_rows {
                // TODO(ffrwd:av 0.19): declare `rows: state` in describe and
                // fold `window.earlier_rows()` here first, each entry at its
                // own pts, before the frame's own rows; then `compose` is
                // pure. Session::fold already takes rows at any earlier time.
                let lines = window.rows(0);
                if !lines.is_empty() {
                    inst.session.fold(t, &lines);
                }
            }
            let (w, h) = (inst.width as usize, inst.height as usize);
            let name = shape.name;
            let mut fetch = |pad: u32| -> Vec<u8> {
                let bytes = window.fetch(pad);
                match ffrwd_frame::Rgba::new(&bytes, w, h) {
                    Ok(_) => bytes,
                    Err(e) => {
                        eprintln!("{name}: input {pad}: {e}");
                        Vec::new()
                    }
                }
            };
            let payload = match inst.session.frame(t, &mut fetch) {
                Output::Same => FramePayload::Same,
                Output::New(bytes) => FramePayload::New(bytes),
            };
            for e in inst.session.take_errors() {
                eprintln!("{name}: row error: {e}");
            }
            if let Some(line) = inst.session.frame_line(t) {
                eprintln!("{name} {line}");
            }
            frames.push(OutFrame {
                pts,
                frame: payload,
                rows: vec![],
            });
        }
        if last && let Some(line) = inst.session.summary_line() {
            eprintln!("{} {line}", shape.name);
        }
        Processed {
            frames,
            trailing: vec![],
        }
    })
}

/// Declares one module: a unit struct implementing the window filter for
/// `$shape`, exported.
#[macro_export]
macro_rules! compose_module {
    ($ty:ident, $shape:expr) => {
        struct $ty;

        const SHAPE: $crate::Shape = $shape;

        impl $crate::Guest for $ty {
            fn describe() -> $crate::exports::ffrwd::av::window_filter::WindowMeta {
                $crate::describe(&SHAPE)
            }

            fn init(
                format: $crate::exports::ffrwd::av::window_filter::Format,
                stream_info: $crate::exports::ffrwd::av::window_filter::StreamInfo,
                params: String,
            ) -> Result<(), String> {
                $crate::init(&SHAPE, format, stream_info, params)
            }

            fn set_params(params: String) -> Result<(), String> {
                $crate::set_params(params)
            }

            fn process(
                window: &$crate::exports::ffrwd::av::window_filter::InWindow,
                _trailing: Vec<String>,
                last: bool,
            ) -> $crate::exports::ffrwd::av::window_filter::Processed {
                $crate::process(&SHAPE, window, last)
            }
        }

        $crate::export_window_filter!($ty with_types_in $crate);
    };
}
