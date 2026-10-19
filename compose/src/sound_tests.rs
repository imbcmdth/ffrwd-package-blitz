//! The held inputs' sound: where each run lands, and the mix the document's
//! levels make of it. The clock is 60 frames a second, so a tick is 800
//! samples at 48 kHz, and each input's samples are one value throughout, so
//! a tick's output says the gains it was mixed at.

use ffrwd_node::mock::{Harness, Tick as Mock};
use ffrwd_node::{BoundStream, Feed, FeedStart, Format, Pairing, Payload, Rational, Runner};

use super::*;

const TB: Rational = Rational::new(1, 60);
/// The samples one tick holds.
const N: usize = 800;
const W: u32 = 16;
const H: u32 = 9;

/// The picture streams are 10 on and their sound 30 on, input 1 first.
fn picture_id(input: u32) -> u32 {
    9 + input
}

fn sound_id(input: u32) -> u32 {
    29 + input
}

/// `held` inputs, the first `heard` of them with sound.
fn streams(held: u32, heard: u32) -> Vec<BoundStream> {
    let mut bound = vec![BoundStream::video("v", 0, W, H, "rgba", TB).rate(Rational::new(60, 1))];
    for input in 1..=held {
        bound.push(BoundStream::video(
            "inputs",
            picture_id(input),
            W,
            H,
            "rgba",
            TB,
        ));
    }
    for input in 1..=heard {
        bound.push(BoundStream::audio(
            "audio",
            sound_id(input),
            sound::RATE,
            2,
            "f32",
        ));
    }
    bound
}

fn open(html: &str, rows: &str, held: u32, heard: u32) -> Harness<Compose> {
    let mut p = serde_json::json!({ "html": html, "lead": 0 });
    if !rows.is_empty() {
        p["rows"] = rows.into();
    }
    Harness::<Compose>::new(&p.to_string(), streams(held, heard)).unwrap_or_else(|e| panic!("{e}"))
}

/// The document: input `k` drawn by `<img id="i{k}" src="ffrwd:{k}">`
/// with the given extra attributes.
fn doc(elements: &str) -> String {
    format!(
        "<style>html, body {{ margin: 0 }} img {{ position: absolute; width: 4px; height: 4px; }}
.quiet {{ --ffrwd-volume: 0 }} .hide {{ display: none }}</style><img src=\"ffrwd:0\">{elements}"
    )
}

/// A feed that shows from tick `at`, its source counting from 0.
fn feed(at: i64) -> Feed {
    Feed {
        start: FeedStart {
            tags: Vec::new(),
            first_pts: 0,
            at,
            known: at,
        },
        ends: None,
    }
}

fn samples(value: f32, frames: usize) -> Vec<u8> {
    std::iter::repeat_n(value, frames * 2)
        .flat_map(f32::to_le_bytes)
        .collect()
}

/// Tick `k`, with each `(input, value)` present since tick 0, its picture
/// shown and its sound (where the call binds it) one value for the tick.
fn tick(h: &Harness<Compose>, k: i64, present: &[(u32, f32)], heard: u32) -> Mock {
    let mut t = h.tick(k).frame(0, k, vec![0; (W * H * 4) as usize]);
    for &(input, value) in present {
        t = t.feed(picture_id(input), feed(0)).frame(
            picture_id(input),
            k,
            vec![255; (W * H * 4) as usize],
        );
        if input <= heard {
            t = t.feed(sound_id(input), feed(0)).frame(
                sound_id(input),
                k * N as i64,
                samples(value, N),
            );
        }
    }
    t
}

/// What left on `audio`: its pts and samples, left channel.
fn heard(emitted: &ffrwd_node::Emitted) -> (i64, Vec<f32>) {
    match emitted.on("audio").as_slice() {
        [
            Payload::Frame {
                pts,
                duration,
                data,
            },
        ] => {
            let all: Vec<f32> = data
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect();
            assert_eq!(*duration, Some((all.len() / 2) as i64));
            (*pts, all.iter().step_by(2).copied().collect())
        }
        other => panic!("one run of samples leaves a tick, not {other:?}"),
    }
}

/// The level each tick's output ends on, over `ticks`, for inputs present
/// as `present(k)` says.
fn levels(
    h: &mut Harness<Compose>,
    ticks: std::ops::Range<i64>,
    heard_inputs: u32,
    present: &dyn Fn(i64) -> Vec<(u32, f32)>,
) -> Vec<f32> {
    ticks
        .map(|k| {
            let emitted = h.process(&tick(h, k, &present(k), heard_inputs)).unwrap();
            let (pts, out) = heard(&emitted);
            assert_eq!((pts, out.len()), (k * N as i64, N), "tick {k}");
            (out[N - 1] * 1e4).round() / 1e4
        })
        .collect()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn sound_groups_the_held_inputs_and_leaves_a_call_without_it_as_it_was() {
    let shape = |bound: &[&str]| {
        let bound: Vec<String> = bound.iter().map(|b| b.to_string()).collect();
        Runner::<Compose>::shape("", &bound).unwrap()
    };
    let today = shape(&["v", "inputs"]);
    assert!(today.pure && today.find_output("audio").is_none());
    let Pairing::Hold(hold) = &today.find_input("inputs").unwrap().pairing else {
        panic!("inputs are held")
    };
    assert_eq!(hold.group, None, "no sound, no group");

    let s = shape(&["v", "inputs", "audio"]);
    assert!(!s.pure, "the squelches and ramps carry from tick to tick");
    let Pairing::Hold(pictures) = &s.find_input("inputs").unwrap().pairing else {
        panic!("inputs are held")
    };
    let audio = s.find_input("audio").unwrap();
    let Pairing::Hold(sound) = &audio.pairing else {
        panic!("audio is held")
    };
    assert_eq!(pictures.group.as_deref(), Some(GROUP));
    assert_eq!(sound.group.as_deref(), Some(GROUP));
    assert_eq!(sound.port_param.as_deref(), Some("audio_port"));
    assert_eq!(
        (sound.anchor.clone(), sound.lead, sound.timeout),
        (pictures.anchor.clone(), pictures.lead, pictures.timeout)
    );
    assert!(audio.many && !audio.required);
    assert_eq!(audio.accepts.sample_formats, ["f32"]);
    assert_eq!(audio.accepts.sample_rates, [48_000]);
    assert_eq!(audio.accepts.channel_counts, [2]);
    let out = s.find_output("audio").unwrap();
    assert!(matches!(
        &out.format,
        Some(Format::Audio(a)) if a.sample_rate == 48_000 && a.channels == 2 && a.sample_fmt == "f32"
    ));
    assert_eq!(out.time_base, Some(Rational::new(1, 48_000)));
    assert!(s.find_output("video").is_some());

    let bound: Vec<String> = ["audio"].iter().map(|b| b.to_string()).collect();
    let err = Runner::<Compose>::shape(r#"{"width":32,"height":18}"#, &bound).unwrap_err();
    assert!(err.contains("bind `v`"), "{err}");
}

#[test]
fn a_lateral_s_sound_is_matched_to_its_picture_by_port() {
    let p: Params = serde_json::from_str(r#"{"fps":30,"lead":0.3,"linger":0,"timeout":1,"presence":"","port":[9001,9002,9003],"audio_port":[9003,9001]}"#).unwrap();
    assert_eq!(sound_of(&p, 2, 3).unwrap(), [2, 0]);
    let p: Params =
        serde_json::from_str(r#"{"fps":30,"lead":0.3,"linger":0,"timeout":1,"presence":""}"#)
            .unwrap();
    assert_eq!(sound_of(&p, 2, 3).unwrap(), [0, 1], "by place");
    assert!(sound_of(&p, 4, 3).is_err());
}

#[test]
fn every_tick_has_its_samples_and_the_ticks_meet() {
    let mut h = open(&doc(r#"<img src="ffrwd:1">"#), "", 1, 1);
    let mut next = 0;
    for k in 0..6 {
        let present = if (2..4).contains(&k) {
            vec![(1, 0.25)]
        } else {
            vec![]
        };
        let (pts, out) = heard(&h.process(&tick(&h, k, &present, 1)).unwrap());
        assert_eq!((pts, out.len()), (next, N), "tick {k}");
        next = pts + N as i64;
        match k {
            2 => assert!(
                out[N - 1] == 0.25 && out.windows(2).all(|w| w[1] >= w[0]),
                "tick 2: the squelch opening ramps the input in"
            ),
            3 => assert!(out.iter().all(|s| *s == 0.25), "tick 3: the input"),
            _ => assert!(
                out.iter().all(|s| *s == 0.0),
                "tick {k}: silence while away"
            ),
        }
    }
}

#[test]
fn a_run_lands_where_its_picture_s_offset_puts_it() {
    // The picture counts in 1/30 and its feed's first frame, pts 50, stands
    // at tick 120 (2 s): the source's 1.667 s is the clock's 2 s. The sound
    // counts in samples from the same origin, so at tick 150 the host hands
    // the samples from 1.667 + 0.5 s, and they land at the window's start.
    let mut bound = streams(0, 0);
    bound.push(BoundStream::video(
        "inputs",
        10,
        W,
        H,
        "rgba",
        Rational::new(1, 30),
    ));
    bound.push(BoundStream::audio("audio", 30, sound::RATE, 2, "f32"));
    let p = serde_json::json!({"html": doc(r#"<img src="ffrwd:1">"#)});
    let mut h = Harness::<Compose>::new(&p.to_string(), bound).unwrap();
    let start = |first_pts| Feed {
        start: FeedStart {
            tags: Vec::new(),
            first_pts,
            at: 120,
            known: 120,
        },
        ends: None,
    };
    let source = (5 * sound::RATE / 3) as i64 + sound::RATE as i64 / 2;
    let late = 100;
    let at = |h: &Harness<Compose>, run_pts: i64| {
        h.tick(150)
            .frame(0, 150, vec![0; (W * H * 4) as usize])
            .feed(10, start(50))
            .frame(10, 65, vec![255; (W * H * 4) as usize])
            .feed(30, start(80_000))
            .frame(30, run_pts, samples(0.5, N - (run_pts - source) as usize))
    };
    let (pts, out) = heard(&h.process(&at(&h, source)).unwrap());
    assert_eq!(pts, 150 * N as i64);
    assert!(
        out.iter().all(|s| *s == 0.5),
        "on time, the window's first sample on"
    );
    let (_, out) = heard(&h.process(&at(&h, source + late)).unwrap());
    assert!(out[..late as usize].iter().all(|s| *s == 0.0));
    assert!(
        out[late as usize..].iter().all(|s| *s == 0.5),
        "a run starting late, from where it starts"
    );
}

#[test]
fn an_element_with_no_volume_is_at_1_and_two_of_one_input_add() {
    let mut h = open(&doc(r#"<img src="ffrwd:1">"#), "", 1, 1);
    assert_eq!(levels(&mut h, 0..2, 1, &|_| vec![(1, 0.3)]), [0.3, 0.3]);
    let mut h = open(
        &doc(r#"<img src="ffrwd:1"><img src="ffrwd:1" style="--ffrwd-volume: 0.5">"#),
        "",
        1,
        1,
    );
    assert_eq!(levels(&mut h, 0..2, 1, &|_| vec![(1, 0.3)]), [0.45, 0.45]);
    let mut h = open(
        &doc(r#"<img src="ffrwd:1" style="--ffrwd-volume: 4">"#),
        "",
        1,
        1,
    );
    assert_eq!(
        levels(&mut h, 0..1, 1, &|_| vec![(1, 0.3)]),
        [1.0],
        "clamped to the sample range"
    );
}

#[test]
fn two_inputs_mix_at_their_volumes_and_a_row_changes_one_with_a_ramp() {
    // #i1 at 1 and #i2 at 0.5: 0.2 + 0.4 x 0.5 = 0.4. A row at 0.08 s, due
    // on tick 5, makes #i1 quiet: 0.2 from there, ramped across tick 5.
    let html = doc(
        r#"<img id="i1" src="ffrwd:1"><img id="i2" src="ffrwd:2" style="--ffrwd-volume: 0.5">"#,
    );
    let rows = r##"{"at": 0.08, "select": "#i1", "change": "+quiet"}"##;
    let mut h = open(&html, rows, 2, 2);
    let both = |_| vec![(1, 0.2), (2, 0.4)];
    for k in 0..5 {
        let (_, out) = heard(&h.process(&tick(&h, k, &both(k), 2)).unwrap());
        assert!(out.iter().all(|s| near(*s, 0.4)), "tick {k}");
    }
    let (_, ramp) = heard(&h.process(&tick(&h, 5, &both(5), 2)).unwrap());
    assert!(near(ramp[0], 0.4 - 0.2 / N as f32), "the ramp's first step");
    assert!(near(ramp[N / 2 - 1], 0.3), "halfway");
    assert!(near(ramp[N - 1], 0.2), "the new level by the tick's end");
    assert!(ramp.windows(2).all(|w| w[1] <= w[0]), "no step");
    assert_eq!(levels(&mut h, 6..8, 2, &both), [0.2, 0.2]);
}

#[test]
fn a_hidden_element_still_sounds() {
    let rows = r##"{"at": 0.03, "select": "#i1", "change": "+hide"}"##;
    let mut h = open(&doc(r#"<img id="i1" src="ffrwd:1">"#), rows, 1, 1);
    assert_eq!(
        levels(&mut h, 0..4, 1, &|_| vec![(1, 0.3)]),
        [0.3, 0.3, 0.3, 0.3],
        "display: none from tick 2 on"
    );
    let rows = r##"{"at": 0.03, "select": "#box", "change": "+hide"}"##;
    let html = doc(r#"<div id="box"><img src="ffrwd:1" style="--ffrwd-volume: 0.5"></div>"#);
    let mut h = open(&html, rows, 1, 1);
    assert_eq!(
        levels(&mut h, 0..4, 1, &|_| vec![(1, 0.3)]),
        [0.15, 0.15, 0.15, 0.15],
        "inside a box hidden from tick 2 on"
    );
    let html = doc(r#"<div class="hide"><img src="ffrwd:1" style="--ffrwd-volume: 0.5"></div>"#);
    let mut h = open(&html, "", 1, 1);
    assert_eq!(
        levels(&mut h, 0..2, 1, &|_| vec![(1, 0.3)]),
        [0.15, 0.15],
        "inside a box hidden from the start"
    );
}

/// Main (input 1) at 1, ducking to 0.5; the cam (input 2) as `cam` says.
fn ducking(cam: &str) -> String {
    doc(&format!(
        r#"<img id="i1" src="ffrwd:1" style="--ffrwd-volume-duck: 0.5"><img id="i2" src="ffrwd:2" {cam}>"#
    ))
}

#[test]
fn main_ducks_while_a_cam_sounds_and_comes_back_when_it_drops() {
    // Main at 0.1, the cam at 0.2, present on ticks 2 to 4.
    let mut h = open(&ducking(""), "", 2, 2);
    let present = |k: i64| {
        let mut p = vec![(1, 0.1)];
        if (2..5).contains(&k) {
            p.push((2, 0.2));
        }
        p
    };
    assert_eq!(
        levels(&mut h, 0..7, 2, &present),
        [0.1, 0.1, 0.25, 0.25, 0.25, 0.1, 0.1],
        "0.1 alone, 0.1 x 0.5 + 0.2 with the cam"
    );
}

#[test]
fn a_cam_at_0_or_without_sound_ducks_nothing_and_no_duck_is_no_change() {
    let both = |_| vec![(1, 0.1), (2, 0.2)];
    let mut h = open(&ducking(r#"class="quiet""#), "", 2, 2);
    assert_eq!(levels(&mut h, 0..2, 2, &both), [0.1, 0.1], "a cam at 0");
    let mut h = open(&ducking(""), "", 2, 1);
    assert_eq!(
        levels(&mut h, 0..2, 1, &both),
        [0.1, 0.1],
        "a picture-only cam at 1"
    );
    let html = doc(r#"<img id="i1" src="ffrwd:1"><img id="i2" src="ffrwd:2">"#);
    let mut h = open(&html, "", 2, 2);
    assert_eq!(levels(&mut h, 0..2, 2, &both), [0.3, 0.3], "no duck at all");
}

#[test]
fn two_ducking_elements_duck_each_other() {
    let mut h = open(&ducking(r#"style="--ffrwd-volume-duck: 0.25""#), "", 2, 2);
    assert_eq!(
        levels(&mut h, 0..2, 2, &|_| vec![(1, 0.4), (2, 0.4)]),
        [0.3, 0.3],
        "0.4 x 0.5 + 0.4 x 0.25"
    );
}

#[test]
fn a_track_below_its_squelch_is_silent_and_ducks_nobody_until_it_is_loud() {
    // The cam at -60 dBFS (0.001), then at 0.2 on ticks 3 and 4, then
    // silent; main at 0.1. Its squelch opens on tick 3 and stays open for the
    // release, 0.5 s (30 ticks), after tick 4: the cam quiet but heard and
    // main ducked to tick 34, then the cam silent and main back.
    let mut h = open(&ducking(""), "", 2, 2);
    let cam = |k: i64| match k {
        3 | 4 => 0.2,
        _ if k < 3 => 0.001,
        _ => 0.0,
    };
    let present = |k: i64| vec![(1, 0.1), (2, cam(k))];
    let got = levels(&mut h, 0..40, 2, &present);
    assert_eq!(
        &got[..3],
        [0.1, 0.1, 0.1],
        "a quiet cam is silent and ducks nobody"
    );
    assert_eq!(
        &got[3..5],
        [0.25, 0.25],
        "loud: heard, and main ducked from that tick"
    );
    assert!(
        got[5..35].iter().all(|l| near(*l, 0.05)),
        "the release holds the duck: {got:?}"
    );
    assert!(
        got[35..].iter().all(|l| near(*l, 0.1)),
        "past it, main is back: {got:?}"
    );
}

#[test]
fn the_release_keeps_a_squelch_open_across_a_short_pause() {
    // The cam loud, silent for 0.2 s (12 ticks), loud again: main stays
    // ducked throughout.
    let mut h = open(&ducking(""), "", 2, 2);
    let cam = |k: i64| if (2..14).contains(&k) { 0.0 } else { 0.2 };
    let got = levels(&mut h, 0..20, 2, &|k| vec![(1, 0.1), (2, cam(k))]);
    assert!(
        got.iter().all(|l| near(*l, 0.05) || near(*l, 0.25)),
        "{got:?}"
    );
}

#[test]
fn a_squelch_of_a_track_s_own_decides_what_crosses_it() {
    // 0.01 is -40 dBFS: under a squelch of -30, over one of -45.
    let present = |_| vec![(1, 0.1), (2, 0.01)];
    let mut h = open(&ducking(r#"style="--ffrwd-squelch: -30""#), "", 2, 2);
    assert_eq!(levels(&mut h, 0..2, 2, &present), [0.1, 0.1]);
    let mut h = open(&ducking(r#"style="--ffrwd-squelch: -45""#), "", 2, 2);
    assert_eq!(levels(&mut h, 0..2, 2, &present), [0.06, 0.06]);
}

#[test]
fn a_track_with_no_squelch_is_always_heard_and_ducks_on_presence() {
    let mut h = open(&ducking(r#"style="--ffrwd-squelch: none""#), "", 2, 2);
    let got = levels(&mut h, 0..4, 2, &|k| {
        let mut p = vec![(1, 0.1)];
        if k >= 2 {
            p.push((2, 0.0005));
        }
        p
    });
    assert_eq!(got, [0.1, 0.1, 0.0505, 0.0505]);
}

#[test]
fn a_background_of_an_input_sounds_as_an_img_does() {
    for html in [
        r#"<div style="width:4px;height:4px;background-image:url(ffrwd:1)"></div>"#,
        r#"<div class="hide" style="width:4px;height:4px;background-image:url(ffrwd:1)"></div>"#,
    ] {
        let mut h = open(&doc(html), "", 1, 1);
        assert_eq!(
            levels(&mut h, 0..2, 1, &|_| vec![(1, 0.3)]),
            [0.3, 0.3],
            "{html}"
        );
    }
}

#[test]
fn the_programme_s_sound_is_input_0_s_and_a_feed_follows_its_format() {
    let bound: Vec<String> = ["v", "inputs", "audio", "a"]
        .iter()
        .map(|b| b.to_string())
        .collect();
    let s = Runner::<Compose>::shape("", &bound).unwrap();
    assert!(!s.pure);
    let a = s.find_input("a").unwrap();
    assert!(matches!(a.pairing, Pairing::Lockstep) && !a.required);
    assert_eq!(a.accepts.sample_rates, [48_000]);
    let audio = s.find_input("audio").unwrap();
    assert_eq!(
        audio.accepts.like.as_deref(),
        Some("a"),
        "a port's feed is conformed to `a`"
    );

    // `a` at 0.1 under the document's <img src="ffrwd:0">, input 1 at 0.2.
    let mut bound = streams(1, 1);
    bound.push(BoundStream::audio("a", 5, sound::RATE, 2, "f32"));
    let p = serde_json::json!({"html": doc(r#"<img src="ffrwd:1">"#)});
    let mut h = Harness::<Compose>::new(&p.to_string(), bound).unwrap();
    for k in 0..3 {
        let t = tick(&h, k, &[(1, 0.2)], 1).frame(5, k * N as i64, samples(0.1, N));
        let (_, out) = heard(&h.process(&t).unwrap());
        assert!(near(out[N - 1], 0.3), "tick {k}: {}", out[N - 1]);
    }
}

/// The clock of a NUT input: 1/61440, so a frame at 60 fps is 1024 and one
/// unit of it is less than a sample.
const FINE: Rational = Rational::new(1, 61_440);

/// `v` on the fine clock, with its rate hint or without, its sound on `a`,
/// and input 1 with its sound, at 0.5.
fn fine(rated: bool) -> Harness<Compose> {
    let mut v = BoundStream::video("v", 0, W, H, "rgba", FINE);
    if rated {
        v = v.rate(Rational::new(60, 1));
    }
    let bound = vec![
        v,
        BoundStream::video("inputs", picture_id(1), W, H, "rgba", FINE),
        BoundStream::audio("audio", sound_id(1), sound::RATE, 2, "f32"),
        BoundStream::audio("a", 5, sound::RATE, 2, "f32"),
    ];
    let html = doc(r#"<img src="ffrwd:1" style="--ffrwd-volume: 0.5">"#);
    let p = serde_json::json!({ "html": html, "lead": 0 });
    Harness::<Compose>::new(&p.to_string(), bound).unwrap_or_else(|e| panic!("{e}"))
}

/// `a`'s sample `n`: a value of its own on each sample, well above the
/// squelch.
fn programme(n: i64) -> f32 {
    0.1 + (n % 997) as f32 / 4000.0
}

/// What `audio` holds at sample `n` on the fine clock: `a` at 1 and input 1,
/// 0.1 throughout, at 0.5.
fn expected(n: i64) -> f32 {
    programme(n) + 0.05
}

/// What left on `audio` on a tick: none, or one run that is not empty, its
/// pts and samples, left channel.
fn ran(emitted: &ffrwd_node::Emitted) -> Option<(i64, Vec<f32>)> {
    match emitted.on("audio").as_slice() {
        [] => None,
        [
            Payload::Frame {
                pts,
                duration,
                data,
            },
        ] => {
            assert!(!data.is_empty(), "an empty run at {pts}");
            let all: Vec<f32> = data
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect();
            assert_eq!(*duration, Some((all.len() / 2) as i64));
            Some((*pts, all.iter().step_by(2).copied().collect()))
        }
        other => panic!("at most one run of samples leaves a tick, not {other:?}"),
    }
}

/// Ticks of `v` at `times` on the fine clock, with no durations, each
/// tick's sound handed from its time to the next tick's (a frame for the
/// last) as the host cuts it. What left on `audio` on each.
fn run_fine(h: &mut Harness<Compose>, times: &[i64]) -> Vec<Option<(i64, Vec<f32>)>> {
    let sample = |pts: i64| pts * sound::RATE as i64 / 61_440;
    let picture = |v| vec![v; (W * H * 4) as usize];
    times
        .iter()
        .enumerate()
        .map(|(k, &pts)| {
            let next = times.get(k + 1).copied().unwrap_or(pts + 1024);
            let (s0, s1) = (sample(pts), sample(next));
            let a: Vec<u8> = (s0..s1)
                .flat_map(|n| [programme(n); 2])
                .flat_map(f32::to_le_bytes)
                .collect();
            let t = h
                .tick(pts)
                .frame(0, pts, picture(0))
                .feed(picture_id(1), feed(0))
                .frame(picture_id(1), pts, picture(255))
                .feed(sound_id(1), feed(0))
                .frame(sound_id(1), s0, samples(0.1, (s1 - s0) as usize))
                .frame(5, s0, a);
            ran(&h.process(&t).unwrap())
        })
        .collect()
}

/// The runs' pts and lengths, and their samples end to end, checking that
/// each starts where the last ended.
fn joined(runs: &[Option<(i64, Vec<f32>)>]) -> (Vec<Option<(i64, usize)>>, Vec<f32>) {
    let mut all = Vec::new();
    let mut next = None;
    let spans = runs
        .iter()
        .map(|run| {
            let (pts, samples) = run.as_ref()?;
            if let Some(next) = next {
                assert_eq!(*pts, next, "the runs meet");
            }
            next = Some(pts + samples.len() as i64);
            all.extend_from_slice(samples);
            Some((*pts, samples.len()))
        })
        .collect();
    (spans, all)
}

#[test]
fn with_no_rate_and_no_durations_the_step_is_learned_from_the_frames() {
    // The first frame's step is not known, so it writes nothing, and the
    // second run covers both frames: the same samples as with the rate.
    let times: Vec<i64> = (0..12).map(|k| k * 1024).collect();
    let (spans, unrated) = joined(&run_fine(&mut fine(false), &times));
    assert_eq!(
        spans[0], None,
        "nothing for the first frame on its own tick"
    );
    assert_eq!(
        spans[1],
        Some((0, 2 * N)),
        "the second run from the first frame"
    );
    for (k, span) in spans.iter().enumerate().skip(2) {
        assert_eq!(*span, Some((k as i64 * N as i64, N)), "tick {k}");
    }
    let (spans, rated) = joined(&run_fine(&mut fine(true), &times));
    for (k, span) in spans.iter().enumerate() {
        assert_eq!(
            *span,
            Some((k as i64 * N as i64, N)),
            "with the rate, tick {k}"
        );
    }
    assert_eq!(unrated.len(), 12 * N);
    assert_eq!(unrated, rated, "the same samples, rate or not");
    for (n, s) in unrated.iter().enumerate() {
        assert!(near(*s, expected(n as i64)), "sample {n}: {s}");
    }
}

#[test]
fn a_change_of_frame_spacing_is_followed() {
    // 60 fps, 30 from the fifth frame, 60 again from the eighth.
    let times = [0, 1024, 2048, 3072, 5120, 7168, 9216, 10240, 11264, 12288];
    let (spans, all) = joined(&run_fine(&mut fine(false), &times));
    assert_eq!(
        spans,
        [
            None,
            Some((0, 1600)),
            Some((1600, 800)),
            Some((2400, 800)),
            // The fourth frame's step said 3200, and the fifth came at
            // 4000: the run starts at 3200 with what the fourth tick's
            // sound held past it, and goes on at the new step.
            Some((3200, 2400)),
            Some((5600, 1600)),
            Some((7200, 1600)),
            // At 8000, earlier than the step said: the last run reached
            // 8800 and this tick's own samples end there, so it writes
            // nothing.
            None,
            Some((8800, 800)),
            Some((9600, 800)),
        ]
    );
    for (n, s) in all.iter().enumerate() {
        // From 8000 to 8800 the last run went past the sound it was handed,
        // which ended at 8000: silence.
        let want = if (8000..8800).contains(&n) {
            0.0
        } else {
            expected(n as i64)
        };
        assert!(near(*s, want), "sample {n}: {s}");
    }
}

#[test]
fn a_held_feed_s_beep_is_heard_on_the_tick_that_shows_its_flash() {
    // foundmeat's numbers: the clock counts in 1/61440 at 60 a second, a
    // tick 1024; the feed's picture counts in 1/90000 from 0.021 s and its
    // sound in samples from 0; the host fixed the feed at tick 206848
    // (3.367 s) from its first picture, so its offset is 3.345667 s, and
    // hands each tick the newest picture at or before the tick less the
    // offset and the samples of the tick's window less the offset, as the
    // hold does. The flash is picture 600 (source 10.021 s) and the beep
    // starts on the sample of the same source time.
    const CLOCK: Rational = Rational::new(1, 61_440);
    const PICTURE: Rational = Rational::new(1, 90_000);
    let mut bound =
        vec![BoundStream::video("v", 0, W, H, "rgba", CLOCK).rate(Rational::new(60, 1))];
    bound.push(BoundStream::video("inputs", 10, W, H, "rgba", PICTURE));
    bound.push(BoundStream::audio("audio", 30, sound::RATE, 2, "f32"));
    let p = serde_json::json!({"html": doc(r#"<img src="ffrwd:1">"#), "lead": 0.3});
    let mut h = Harness::<Compose>::new(&p.to_string(), bound).unwrap();
    let at = 206_848i64;
    let first_picture = 1_890i64;
    // The offset in exact rationals: clock seconds less source seconds.
    let offset =
        |t: i64| t as f64 / 61_440.0 - (at as f64 / 61_440.0 - first_picture as f64 / 90_000.0);
    let record = |first_pts| Feed {
        start: FeedStart {
            tags: Vec::new(),
            first_pts,
            at,
            known: 188_416,
        },
        ends: None,
    };
    // The sound's record: the sample the same offset puts at `at`, 0.021 s.
    let first_sound = 1_008i64;
    let flash = 600i64;
    let beep_from = ((first_picture + flash * 1_500) as f64 / 90_000.0 * 48_000.0).round() as i64;
    let mut heard_at: Option<i64> = None;
    let mut shown_at: Option<i64> = None;
    let mut written = 0i64;
    for k in 0..900i64 {
        let t = 188_416 + k * 1_024;
        let mut tick = h.tick(t).frame(0, t, vec![0; (W * H * 4) as usize]);
        if t >= at {
            let target = offset(t);
            let n = ((target - first_picture as f64 / 90_000.0) * 60.0 + 1e-9).floor() as i64;
            if n >= flash && shown_at.is_none() {
                shown_at = Some(t);
            }
            let s0 = (offset(t) * 48_000.0 + 1e-9).floor() as i64;
            let s1 = (offset(t + 1_024) * 48_000.0 + 1e-9).floor() as i64;
            let run: Vec<f32> = (s0..s1)
                .map(|i| {
                    if (beep_from..beep_from + 2_400).contains(&i) {
                        1.0
                    } else {
                        0.0
                    }
                })
                .collect();
            let bytes: Vec<u8> = run
                .iter()
                .flat_map(|v| [*v, *v])
                .flat_map(f32::to_le_bytes)
                .collect();
            tick = tick
                .feed(10, record(first_picture))
                .frame(
                    10,
                    first_picture + n.max(0) * 1_500,
                    vec![255; (W * H * 4) as usize],
                )
                .feed(30, record(first_sound))
                .frame(30, s0, bytes);
        }
        let (pts, out) = heard(&h.process(&tick).unwrap());
        written = written.max(pts + out.len() as i64);
        if heard_at.is_none()
            && let Some(i) = out.iter().position(|v| *v > 0.5)
        {
            heard_at = Some(pts + i as i64);
        }
    }
    let shown = shown_at.expect("the flash showed") as f64 / 61_440.0;
    let heard = heard_at.expect("the beep was heard") as f64 / 48_000.0;
    assert!(written > 0);
    assert!(
        (heard - shown).abs() < 1.0 / 60.0,
        "the beep at {heard:.4} s, the flash on the tick at {shown:.4} s"
    );
}
