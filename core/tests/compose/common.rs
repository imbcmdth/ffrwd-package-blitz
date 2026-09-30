//! What the tests share: documents, frames, sessions and pixels.

use std::cell::RefCell;

use compose_core::params::{Log, Params};
use compose_core::{Output, Session};

pub const FPS: f64 = 30.0;

/// Frame `i`'s time at 30 fps.
pub fn ft(i: usize) -> f64 {
    i as f64 / FPS
}

/// A document: `css` in one `<style>` in the head, `body` as the body.
pub fn doc(css: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html><html><head><style>html, body {{ margin: 0; padding: 0; overflow: hidden; background: #112233; }}\n{css}</style></head><body>{body}</body></html>"
    )
}

pub fn params(html: &str) -> Params {
    Params {
        html: html.to_string(),
        log: Log::Off,
        ..Params::default()
    }
}

pub fn session(html: &str, w: u32, h: u32, inputs: u32) -> Session {
    Session::new(params(html), (w, h), (w, h), inputs)
}

/// A picture for input `input`: a gradient with bars, different per input
/// and per frame `n`, alpha 255.
pub fn picture(w: u32, h: u32, input: u32, n: usize) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let mut f = vec![255u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            f[i] = ((x * 255 / w.max(1)) as u8).wrapping_add((n * 7) as u8);
            f[i + 1] = (y * 255 / h.max(1)) as u8;
            f[i + 2] = if ((x + n) / 37) % 2 == 0 { 40 } else { 200 } ^ (input as u8 * 90);
        }
    }
    f
}

/// Renders frame `n` at `t`: each input's picture is `picture(.., input, n)`.
/// Returns the output and the inputs fetched.
pub fn render(s: &mut Session, w: u32, h: u32, t: f64, n: usize) -> (Output, Vec<u32>) {
    let fetched = RefCell::new(Vec::new());
    let mut fetch = |i: u32| {
        fetched.borrow_mut().push(i);
        picture(w, h, i, n)
    };
    let out = s.frame(t, &mut fetch);
    (out, fetched.into_inner())
}

pub fn bytes(out: Output) -> Vec<u8> {
    match out {
        Output::New(b) => b,
        Output::Same => panic!("expected a rendered frame, got the bypass"),
    }
}

/// The pixel at device px (x, y).
pub fn px(frame: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [frame[i], frame[i + 1], frame[i + 2], frame[i + 3]]
}

/// (max abs channel difference, pixels that differ).
pub fn diff(a: &[u8], b: &[u8]) -> (u8, usize) {
    assert_eq!(a.len(), b.len());
    let mut max = 0u8;
    let mut n = 0usize;
    for (pa, pb) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        let d = pa
            .iter()
            .zip(pb)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap();
        if d > 0 {
            n += 1;
            max = max.max(d);
        }
    }
    (max, n)
}

pub fn lines(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|r| r.to_string()).collect()
}

pub fn frac(x: f64) -> f64 {
    x - x.floor()
}

/// Distance between two phases of a unit loop (0 and 1 are the same).
pub fn phase_err(a: f64, b: f64) -> f64 {
    let d = (a - b).abs();
    d.min(1.0 - d)
}
