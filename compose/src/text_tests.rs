//! The closed captions on `text`: what a call with sound writes from the
//! cues it draws. The clock is 10 frames a second, so a tick is 0.1 s.

use ffrwd_node::mock::Harness;
use ffrwd_node::{BoundStream, Rational, Runner};
use serde_json::json;

use super::*;

const TB: Rational = Rational::new(1, 10);
const W: u32 = 16;
const H: u32 = 9;
/// The two casters' cue streams.
const CAM1: u32 = 40;
const CAM2: u32 = 41;

fn shape(bound: &[&str]) -> Shape {
    let bound: Vec<String> = bound.iter().map(|b| b.to_string()).collect();
    Runner::<Compose>::shape(r##"{"captions": "#captions"}"##, &bound).unwrap()
}

#[test]
fn a_call_with_sound_and_cues_writes_text() {
    let s = shape(&["v", "inputs", "audio", "cues"]);
    let text = s.find_output(TEXT).expect("text");
    assert_eq!(text.schema.as_deref(), Some(TEXT_SCHEMA));
    assert!(!s.pure);
    assert!(shape(&["v", "a", "cues"]).find_output(TEXT).is_some());
    let silent = shape(&["v", "inputs", "cues"]);
    assert!(
        silent.pure && silent.find_output(TEXT).is_none(),
        "a call without sound spreads over workers and writes no text"
    );
    assert!(
        shape(&["v", "inputs", "audio"]).find_output(TEXT).is_none(),
        "no cues, no text"
    );
}

const HTML: &str = r#"<style>body { margin: 0 }</style><p id="captions"></p>
<p id="caster1">Mango</p><p id="caster2"></p><p id="handle2">@ludwig</p>"#;

/// Two casters' cues, the closed captions switched on at 0.5 s, off at
/// 2.0 s and on again at 2.5 s, and caster 1 renamed at 1.2 s.
fn open() -> Harness<Compose> {
    let rows = json!([
        {"at": 0.5, "select": "body", "change": "+captions-closed"},
        {"at": 1.2, "select": "#caster1", "text": "MangoPlays"},
        {"at": 2.0, "select": "body", "change": "-captions-closed"},
        {"at": 2.5, "select": "body", "change": "+captions-closed"},
    ]);
    let p = json!({
        "html": HTML,
        "rows": rows.to_string(),
        "captions": "#captions",
        "speakers": "#caster{n}, #handle{n}",
    });
    let bound = vec![
        BoundStream::video("v", 0, W, H, "rgba", TB).rate(Rational::new(10, 1)),
        BoundStream::rows("cues", CAM1, TB),
        BoundStream::rows("cues", CAM2, TB),
        BoundStream::audio("a", 5, sound::RATE, 2, "f32"),
    ];
    Harness::<Compose>::new(&p.to_string(), bound).unwrap_or_else(|e| panic!("{e}"))
}

fn cue(text: &str, start: f64, done: bool) -> Value {
    json!({"text": text, "start_t": start, "end_t": start + 1.0, "final": done})
}

#[test]
fn the_cues_drawn_go_out_as_cue_rows_with_their_voices_while_the_class_is_on() {
    let mut h = open();
    let arriving: &[(i64, u32, Value)] = &[
        (3, CAM1, cue("he goes", 10.0, false)),
        (8, CAM1, cue("he goes for the", 10.0, false)),
        (10, CAM2, cue("what a read", 3.0, true)),
        (15, CAM1, cue("he goes for the flank", 10.0, false)),
        (22, CAM1, cue("he goes for the flank and", 10.0, false)),
        (28, CAM1, cue("he goes for the flank and wins", 10.0, true)),
    ];
    let mut sent = Vec::new();
    for k in 0..=30 {
        let mut tick = h.tick(k).frame(0, k, vec![0; (W * H * 4) as usize]);
        for (_, stream, row) in arriving.iter().filter(|(at, _, _)| *at == k) {
            tick = tick.row(*stream, k, row);
        }
        if k == 30 {
            tick = tick.last();
        }
        for (pts, row) in h.process(&tick).unwrap().messages(TEXT) {
            assert_eq!(pts, k, "a row goes out on the tick that saw it");
            let row: Value = serde_json::from_str(&row).unwrap();
            sent.push((k, row));
        }
    }
    let row = |text: &str, start: f64, end: f64, done: bool, voice: &str| json!({"text": text, "start_t": start, "end_t": end, "final": done, "voice": voice});
    assert_eq!(
        sent,
        [
            (5, row("he goes", 0.5, 0.5, false, "Mango")),
            (8, row("he goes for the", 0.5, 0.8, false, "Mango")),
            (10, row("what a read", 1.0, 4.0, true, "@ludwig")),
            (12, row("he goes for the", 0.5, 1.2, false, "MangoPlays")),
            (
                15,
                row("he goes for the flank", 0.5, 1.5, false, "MangoPlays")
            ),
            (
                20,
                row("he goes for the flank", 0.5, 2.0, true, "MangoPlays")
            ),
            (
                25,
                row("he goes for the flank and", 2.5, 2.5, false, "MangoPlays")
            ),
            (
                28,
                row(
                    "he goes for the flank and wins",
                    2.5,
                    5.8,
                    true,
                    "MangoPlays"
                )
            ),
        ]
    );
}

#[test]
fn the_last_tick_ends_a_cue_still_growing() {
    let mut h = open();
    let mut last = Vec::new();
    for k in 0..=7 {
        let mut tick = h.tick(k).frame(0, k, vec![0; (W * H * 4) as usize]);
        if k == 6 {
            tick = tick.row(CAM1, k, &cue("and that is", 6.0, false));
        }
        if k == 7 {
            tick = tick.last();
        }
        last = h.process(&tick).unwrap().messages(TEXT);
    }
    let ended: Value = serde_json::from_str(&last[0].1).unwrap();
    assert_eq!(
        ended,
        json!({"text": "and that is", "start_t": 0.6, "end_t": 0.7, "final": true, "voice": "Mango"})
    );
}
