//! `compose` and `page`: one HTML document rendered by Blitz each tick over
//! the video inputs it shows, as a node.
//!
//! The clock is `v` when the call binds it (`compose`), and otherwise a rate
//! of `fps` (`page`, a source). `inputs` are held, each showing its newest
//! frame at the tick and nothing while its feed is down; one given by a
//! port is whatever connects there. `v` is `ffrwd:0` in the document and
//! the held inputs `ffrwd:1` on, in the order the call names them. Rows on
//! `changes` are folded as state, so a call without `presence` is pure and
//! a host spreads it over workers, each folding the rows of the ticks the
//! others rendered.

use std::collections::HashMap;
use std::time::Instant;

use compose_core::params::{Log, Params as Document};
use compose_core::presence::{self, Clock, Entry, Presence};
use compose_core::{MAX_INPUTS, Output as Made, Session, bit};
use ffrwd_node::{
    Anchor, Bound, BoundStream, Init, Input, Node, Out, Output, Rational, Result, Shape, StateRow,
    Tick,
};
use serde::Deserialize;
use serde_json::{Map, Value};

const PARAMS_SCHEMA: &str = r#"{"type":"object","properties":{"html":{"type":"string","default":""},"rows":{"type":"string","default":""},"width":{"type":"number","minimum":1},"height":{"type":"number","minimum":1},"css_width":{"type":"number","minimum":1},"css_height":{"type":"number","minimum":1},"fit":{"type":"string","enum":["contain","stretch"],"default":"contain"},"bypass":{"type":"boolean","default":true},"fps":{"type":"number","exclusiveMinimum":0,"maximum":240,"default":30},"port":{"type":["array","integer"],"items":{"type":"integer","minimum":1,"maximum":65535},"minimum":1,"maximum":65535},"lead":{"type":"number","minimum":0,"maximum":60,"default":0.3},"linger":{"type":"number","minimum":0,"maximum":60,"default":0},"timeout":{"type":"number","minimum":0,"maximum":60,"default":1},"latency":{"type":"number","minimum":0},"presence":{"type":"string","default":""},"log":{"type":"string","enum":["off","summary","frame"],"default":"off"}},"additionalProperties":false}"#;

/// What `changes` reads: rows with a selector. The rest of a row is read
/// when it is applied, and a row that does not read is logged.
const CHANGES_SCHEMA: &str =
    r#"{"type":"object","properties":{"select":{"type":"string"}},"required":["select"]}"#;

/// The tag a source puts on its stream to say its timestamps are the
/// clock's, as a timed feeder's are.
const TIMED_TAG: &str = "smart_timed";

const PIXEL_FORMAT: &str = "rgba";

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

pub struct Compose {
    v: Option<u32>,
    /// The streams of `inputs`, input 1 on.
    held: Vec<u32>,
    sizes: Vec<Option<(u32, u32)>>,
    bases: HashMap<u32, Rational>,
    session: Session,
    presence: Presence,
    /// The stream each presence entry watches.
    watched: Vec<u32>,
    last_pts: Option<i64>,
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

impl Node for Compose {
    const NAME: &'static str = "compose";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const PARAMS_SCHEMA: &'static str = PARAMS_SCHEMA;
    type Params = Params;

    fn shape(params: &Params, bound: &Bound) -> Result<Shape> {
        let doc = params.document()?;
        let entries = Entry::parse_list(&params.presence)?;
        let clocked = bound.has("v");
        let mut held = Input::video("inputs")
            .optional()
            .many()
            .anchor(Anchor::Tagged(TIMED_TAG.to_owned()))
            .lead(params.lead)
            .pixel_formats(&[PIXEL_FORMAT]);
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
                .input(Input::video("v").clock().pixel_formats(&[PIXEL_FORMAT]))
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
        if entries.is_empty() {
            shape = shape.pure();
        }
        Ok(shape)
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
        let mut sizes = Vec::new();
        for stream in v.iter().chain(held.iter()) {
            let video = stream
                .video_format()
                .ok_or_else(|| format!("`{}` is a video input", stream.port))?;
            if video.pix_fmt != PIXEL_FORMAT {
                return Err(format!(
                    "compose takes {PIXEL_FORMAT} pictures, and `{}` is {}",
                    stream.port, video.pix_fmt
                )
                .into());
            }
            sizes.push(Some((video.width, video.height)));
        }
        if v.is_none() {
            sizes.insert(0, None);
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
        let summary = doc.log != Log::Off;
        let started = Instant::now();
        let mut session = Session::with_sizes(doc, out, sizes.clone());
        for e in session.take_errors() {
            eprintln!("compose: row error: {e}");
        }
        if summary {
            let g = *session.compositor().geometry();
            eprintln!(
                "compose: {}x{} rgba, {} picture(s), canvas {}x{}, design {}x{} CSS px, {} parameter rows, {} presence entr{}; document built in {:.1} ms",
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
                started.elapsed().as_secs_f64() * 1e3
            );
        }
        Ok(Compose {
            v: v.map(|s| s.id),
            held,
            sizes,
            bases,
            session,
            presence: Presence::new(entries),
            watched,
            last_pts: None,
        })
    }

    fn set_params(&mut self, params: Params) -> Result<()> {
        let doc = params.document()?;
        let entries = Entry::parse_list(&params.presence)?;
        self.watched = watched(&entries, &self.held)?;
        self.presence.set(entries);
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
            let step = duration
                .or(self.last_pts.map(|last| pts - last))
                .filter(|d| *d > 0);
            let feeds: Vec<Option<presence::Feed>> = self
                .watched
                .iter()
                .map(|id| {
                    tick.feed(*id).map(|f| presence::Feed {
                        at: f.start.at,
                        first_pts: f.start.first_pts,
                        ends: f.ends,
                    })
                })
                .collect();
            let clock = Clock {
                num: clock.num,
                den: clock.den,
            };
            let rows = self.presence.tick(pts, step, clock, &feeds);
            self.session.schedule(rows);
        }
        self.last_pts = Some(pts);

        let shown: Vec<_> = self.held.iter().map(|id| tick.frame(*id)).collect();
        let mut absent = if clocked.is_some() { 0 } else { bit(0) };
        for (k, frame) in shown.iter().enumerate() {
            if frame.is_none() {
                absent |= bit(k as u32 + 1);
            }
        }
        let (held, sizes) = (&self.held, &self.sizes);
        let mut fetch = |input: u32| -> Vec<u8> {
            let found = match input {
                0 => clocked.as_ref().map(|(v, frame)| (*v, frame.index)),
                k => held
                    .get(k as usize - 1)
                    .zip(shown.get(k as usize - 1).cloned().flatten())
                    .map(|(id, frame)| (*id, frame.index)),
            };
            let Some(((id, index), Some((w, h)))) = found.zip(sizes.get(input as usize).copied())
            else {
                return Vec::new();
            };
            let bytes = tick.fetch(id, index);
            match ffrwd_frame::Rgba::new(&bytes, w as usize, h as usize) {
                Ok(_) => bytes,
                Err(e) => {
                    eprintln!("compose: input {input}: {e}");
                    Vec::new()
                }
            }
        };
        let made = self.session.frame_without(t, absent, &mut fetch);
        match made {
            Made::Same(0) => {
                if let Some((v, frame)) = &clocked {
                    out.pass("video", *v, frame)?;
                }
            }
            Made::Same(k) => {
                let k = k as usize - 1;
                if let (Some(id), Some(Some(frame))) = (self.held.get(k), shown.get(k)) {
                    out.same("video", pts, duration, *id, frame.index)?;
                }
            }
            Made::New(bytes) => out.frame("video", pts, duration, bytes)?,
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

ffrwd_node::export!(Compose);

#[cfg(test)]
mod tests;
