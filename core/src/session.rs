//! One instance's life: the document, its timeline, and what each frame
//! turns into. The wasm module is a thin shim over this, so everything it
//! decides can be tested on the native target.

use std::sync::Arc;
use std::time::Instant;

use crate::changes::{Row, Source, Timed, Timeline};
use crate::params::{Log, Params};
use crate::{Compositor, Plan};

/// What one frame turns into.
#[derive(Debug, PartialEq)]
pub enum Output {
    /// Input 0's bytes, which were never fetched.
    Same,
    /// A new frame.
    New(Vec<u8>),
}

/// Per-frame timings in microseconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub rows: usize,
    pub apply_us: f64,
    pub resolve_us: f64,
    pub plan_us: f64,
    pub fetch_us: f64,
    pub paint_cmds_us: f64,
    pub raster_us: f64,
    pub total_us: f64,
    pub bypassed: bool,
    pub fetched: u32,
}

#[derive(Default)]
struct Totals {
    frames: u64,
    bypassed: u64,
    rows: u64,
    total_us: f64,
    rendered_us: f64,
}

pub struct Session {
    pub params: Params,
    out: (u32, u32),
    input: (u32, u32),
    inputs: u32,
    comp: Compositor,
    tl: Timeline,
    totals: Totals,
    pub last: FrameStats,
}

impl Session {
    /// Builds the document and anchors its timeline at stream time 0, the
    /// epoch every instance shares; rows from `changes` due by then are
    /// applied there. `out` is the output frame's size and `input` the input
    /// frames' (one size for every input).
    pub fn new(params: Params, out: (u32, u32), input: (u32, u32), inputs: u32) -> Session {
        let (comp, tl) = Session::build(&params, out, input, inputs, &[]);
        Session {
            params,
            out,
            input,
            inputs,
            comp,
            tl,
            totals: Totals::default(),
            last: FrameStats::default(),
        }
    }

    /// A late joiner: the document another instance was built from, plus
    /// that instance's log (`Timeline::log`, the rows applied since the last
    /// full-page replace, each at the time it took effect). Its frames from
    /// then on are the other instance's.
    pub fn join(
        params: Params,
        out: (u32, u32),
        input: (u32, u32),
        inputs: u32,
        log: &[Timed],
    ) -> Session {
        let (comp, tl) = Session::build(&params, out, input, inputs, log);
        Session {
            params,
            out,
            input,
            inputs,
            comp,
            tl,
            totals: Totals::default(),
            last: FrameStats::default(),
        }
    }

    fn build(
        params: &Params,
        out: (u32, u32),
        input: (u32, u32),
        inputs: u32,
        stream_log: &[Timed],
    ) -> (Compositor, Timeline) {
        let mut comp = Compositor::new(&params.html, params.geometry(out, input), inputs);
        let mut tl = Timeline::new();
        tl.start(&mut comp, 0.0);
        tl.extend(params.changes.iter().cloned(), 0.0, Source::Param);
        // Rows from the parameters come from `params`; the log adds the
        // ones that arrived with frames.
        for t in stream_log.iter().filter(|t| t.source == Source::Stream) {
            tl.push(
                Row {
                    at: Some(t.at),
                    ..t.row.clone()
                },
                t.at,
                Source::Stream,
            );
        }
        tl.replay_due(&mut comp, 0.0);
        (comp, tl)
    }

    /// New parameters. A new document, `changes` or `css_width` builds the
    /// document again from the epoch: the new `changes`, then the rows that
    /// arrived with frames since the last full-page replace, each at its own
    /// time, up to where this instance had got.
    pub fn set_params(&mut self, params: Params) {
        if !self.params.same_document(&params) {
            let now = self.tl.now();
            let stream_log: Vec<_> = self
                .tl
                .log()
                .iter()
                .chain(self.tl.pending_rows())
                .filter(|t| t.source == Source::Stream)
                .cloned()
                .collect();
            let (mut comp, mut tl) =
                Session::build(&params, self.out, self.input, self.inputs, &stream_log);
            if now.is_finite() {
                tl.advance(&mut comp, now);
            }
            self.comp = comp;
            self.tl = tl;
        }
        self.params = params;
    }

    pub fn compositor(&mut self) -> &mut Compositor {
        &mut self.comp
    }

    pub fn timeline(&self) -> &Timeline {
        &self.tl
    }

    /// Row errors since the last call, for the log.
    pub fn take_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.tl.errors)
    }

    /// Folds in the rows that arrived with a frame at `t` seconds: each line
    /// is one change object or an array of them, and a line that is not a
    /// change row is ignored. They take effect at their `at`, or at `t` when
    /// they have none or an earlier one, and are applied when the frame
    /// that reaches that time is rendered.
    ///
    /// A call folds, in pts order, the rows of every frame it has to know
    /// about and then renders: under ffrwd:av 0.18 that is its own frame's
    /// rows; under a host that hands a worker the rows of the frames it
    /// skipped, those first, each at its own frame's time. Either way every
    /// row is applied at the same time on every instance.
    pub fn fold(&mut self, t: f64, lines: &[String]) -> usize {
        let mut n = 0;
        for line in lines {
            let (rows, errors) = Row::from_upstream(line);
            n += rows.len();
            self.tl.extend(rows, t, Source::Stream);
            self.tl.errors.extend(errors);
        }
        n
    }

    /// One frame at `t` seconds: rows due by `t` are applied at their own
    /// times, the document is resolved at `t`, and the frame is either input
    /// 0 untouched (nothing fetched) or rendered from the inputs it draws,
    /// each fetched once through `fetch`.
    pub fn frame(&mut self, t: f64, fetch: &mut dyn FnMut(u32) -> Vec<u8>) -> Output {
        let start = Instant::now();
        let step = self.tl.advance(&mut self.comp, t);
        let s = Instant::now();
        let plan: Plan = self.comp.plan();
        let plan_us = us(s);
        let mut st = FrameStats {
            rows: step.rows,
            apply_us: step.apply_us,
            resolve_us: step.replay_resolve_us + step.resolve_us,
            plan_us,
            ..Default::default()
        };
        let same_size = self.out == self.input;
        let out = if self.params.bypass && plan.bypass && same_size {
            st.bypassed = true;
            Output::Same
        } else {
            let s = Instant::now();
            let in_len = (self.input.0 as usize) * (self.input.1 as usize) * 4;
            let len = (self.out.0 as usize) * (self.out.1 as usize) * 4;
            for input in plan.inputs().filter(|i| *i < self.inputs) {
                let bytes = fetch(input);
                if bytes.len() == in_len {
                    self.comp.set_frame(input, Arc::new(bytes));
                    st.fetched += 1;
                }
            }
            st.fetch_us = us(s);
            (st.paint_cmds_us, st.raster_us) = self.comp.paint();
            Output::New(std::mem::replace(&mut self.comp.out, vec![0u8; len]))
        };
        st.total_us = us(start);
        self.totals.frames += 1;
        self.totals.bypassed += st.bypassed as u64;
        self.totals.rows += st.rows as u64;
        self.totals.total_us += st.total_us;
        if !st.bypassed {
            self.totals.rendered_us += st.total_us;
        }
        self.last = st;
        out
    }

    /// The per-frame log line, when `log` asks for one.
    pub fn frame_line(&self, t: f64) -> Option<String> {
        if self.params.log != Log::Frame {
            return None;
        }
        let s = &self.last;
        Some(format!(
            "blitz: t={t:.3} rows={} bypass={} fetched={} apply={:.3} resolve={:.3} plan={:.3} fetch={:.3} paint_cmds={:.3} raster={:.3} total={:.3} ms",
            s.rows,
            s.bypassed as u8,
            s.fetched,
            s.apply_us / 1e3,
            s.resolve_us / 1e3,
            s.plan_us / 1e3,
            s.fetch_us / 1e3,
            s.paint_cmds_us / 1e3,
            s.raster_us / 1e3,
            s.total_us / 1e3
        ))
    }

    /// The closing log line, when `log` asks for one.
    pub fn summary_line(&self) -> Option<String> {
        if self.params.log == Log::Off {
            return None;
        }
        let t = &self.totals;
        let rendered = t.frames - t.bypassed;
        Some(format!(
            "blitz: {} frames, {} bypassed, {} rows; mean {:.3} ms a frame, {:.3} ms a rendered frame",
            t.frames,
            t.bypassed,
            t.rows,
            t.total_us / t.frames.max(1) as f64 / 1e3,
            t.rendered_us / rendered.max(1) as f64 / 1e3
        ))
    }
}

fn us(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e6
}
