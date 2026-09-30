//! When a row takes effect: at its `at` exactly, at its arrival when it has
//! no `at` or an earlier one, and what it starts runs from then.

use crate::common::*;

const CSS: &str = "
#slot { position: absolute; left: 10px; top: 10px; width: 40px; height: 40px; }
.pulse { width: 40px; height: 40px; background: #1e90ff; animation: pulse 1s linear infinite; }
@keyframes pulse { from { opacity: 0 } to { opacity: 1 } }
#tr { position: absolute; left: 60px; top: 10px; width: 40px; height: 40px;
      background: #fff; opacity: 0; transition: opacity 1s linear; }
#tr.on { opacity: 1; }
.gone { display: none; }
.gone2 { display: none; animation: none; }";

const BODY: &str = r#"<div id="slot"></div><div id="tr"></div>"#;

fn html() -> String {
    doc(CSS, BODY)
}

/// Opacity of `sel` at each frame from `from` to `to`, after folding
/// `rows` with the frame at `arrive`.
fn sample(rows: &[&str], arrive: usize, from: usize, to: usize, sel: &str) -> Vec<(f64, f64)> {
    let mut s = session(&html(), 128, 64, 1);
    let mut out = Vec::new();
    for i in 0..=to {
        if i == arrive {
            s.fold(ft(i), &lines(rows));
        }
        render(&mut s, 128, 64, ft(i), i);
        if i >= from {
            let o = s.compositor().opacity_of(sel).map(|o| o as f64);
            out.push((ft(i), o.unwrap_or(f64::NAN)));
        }
    }
    out
}

#[test]
fn an_animation_inserted_at_3_01_starts_at_3_01() {
    // Frames at 3.0000 and 3.0333; the row arrives with the frame at 2.0.
    let rows =
        [r##"{"at": 3.01, "select": "#slot", "html": "<div class=\"pulse\" id=\"p\"></div>"}"##];
    let got = sample(&rows, 60, 91, 150, "#p");
    let err = got
        .iter()
        .map(|(t, o)| phase_err(*o, frac(t - 3.01)))
        .fold(0.0, f64::max);
    assert!(err < 1e-4, "max error against a start at 3.01: {err}");
    // And not at the frame after it.
    let err_frame = got
        .iter()
        .map(|(t, o)| phase_err(*o, frac(t - ft(91))))
        .fold(0.0, f64::max);
    assert!(err_frame > 0.02, "{err_frame}");
}

#[test]
fn a_transition_cued_at_3_01_starts_at_3_01() {
    let rows = [r##"{"at": 3.01, "select": "#tr", "change": "+on"}"##];
    let got = sample(&rows, 0, 91, 150, "#tr");
    let err = got
        .iter()
        .map(|(t, o)| (o - (t - 3.01).clamp(0.0, 1.0)).abs())
        .fold(0.0, f64::max);
    assert!(err < 1e-4, "max error against a start at 3.01: {err}");
}

#[test]
fn a_row_without_at_takes_effect_at_the_frame_it_arrives_with() {
    let rows = [r##"{"select": "#slot", "html": "<div class=\"pulse\" id=\"p\"></div>"}"##];
    let got = sample(&rows, 95, 95, 140, "#p");
    let err = got
        .iter()
        .map(|(t, o)| phase_err(*o, frac(t - ft(95))))
        .fold(0.0, f64::max);
    assert!(err < 1e-4, "{err}");
}

#[test]
fn a_row_arriving_after_its_at_takes_effect_on_arrival() {
    let rows =
        [r##"{"at": 2.0, "select": "#slot", "html": "<div class=\"pulse\" id=\"p\"></div>"}"##];
    let got = sample(&rows, 95, 95, 140, "#p");
    let err = got
        .iter()
        .map(|(t, o)| phase_err(*o, frac(t - ft(95))))
        .fold(0.0, f64::max);
    assert!(err < 1e-4, "{err}");
}

#[test]
fn a_loop_runs_until_it_is_removed() {
    let rows = [
        r##"{"at": 1.01, "select": "#slot", "html": "<div class=\"pulse\" id=\"p\"></div>"}"##,
        r##"{"at": 4.51, "select": "#slot", "html": ""}"##,
    ];
    let got = sample(&rows, 0, 31, 150, "#p");
    for (t, o) in got {
        if t < 4.51 {
            assert!(phase_err(o, frac(t - 1.01)) < 1e-4, "t={t} o={o}");
        } else {
            assert!(o.is_nan(), "t={t}: still there");
        }
    }
}

/// Stylo's behaviour, which differs from a browser's: `display: none` does
/// not cancel a CSS animation. `animation: none` in the same rule does.
#[test]
fn display_none_keeps_an_animation_and_animation_none_cancels_it() {
    let insert =
        r##"{"at": 1.01, "select": "#slot", "html": "<div class=\"pulse\" id=\"p\"></div>"}"##;
    let hidden = [
        insert,
        r##"{"at": 2.21, "select": "#p", "change": "+gone"}"##,
        r##"{"at": 2.61, "select": "#p", "change": "-gone"}"##,
    ];
    let got = sample(&hidden, 0, 79, 120, "#p");
    let continuing = got
        .iter()
        .map(|(t, o)| phase_err(*o, frac(t - 1.01)))
        .fold(0.0, f64::max);
    assert!(
        continuing < 1e-4,
        "display: none cancelled it ({continuing})"
    );

    let cancelled = [
        insert,
        r##"{"at": 2.21, "select": "#p", "change": "+gone2"}"##,
        r##"{"at": 2.61, "select": "#p", "change": "-gone2"}"##,
    ];
    let got = sample(&cancelled, 0, 79, 120, "#p");
    let fresh = got
        .iter()
        .map(|(t, o)| phase_err(*o, frac(t - 2.61)))
        .fold(0.0, f64::max);
    assert!(fresh < 1e-4, "animation: none did not restart it ({fresh})");
}
