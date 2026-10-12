use ffrwd_node::mock::Harness;
use ffrwd_node::{
    Anchor, BoundStream, Clock as ClockKind, Feed, FeedStart, Format, Pairing, Payload, Rational,
    RowsUse, Runner, VideoFormat,
};

use super::*;

const TB: Rational = Rational::new(1, 30);
const W: u32 = 64;
const H: u32 = 36;

fn shape(params: &str, bound: &[&str]) -> std::result::Result<Shape, String> {
    let bound: Vec<String> = bound.iter().map(|b| b.to_string()).collect();
    Runner::<Compose>::shape(params, &bound)
}

fn params(html: &str, extra: &str) -> String {
    let html = serde_json::to_string(html).unwrap();
    if extra.is_empty() {
        format!(r#"{{"html":{html}}}"#)
    } else {
        format!(r#"{{"html":{html},{extra}}}"#)
    }
}

fn picture(mark: u8) -> Vec<u8> {
    let mut p = vec![255u8; (W * H * 4) as usize];
    for (i, px) in p.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        px[0] = mark;
        px[1] = (i % 251) as u8;
    }
    p
}

fn streams(held: u32) -> Vec<BoundStream> {
    let mut bound = vec![BoundStream::video("v", 0, W, H, "rgba", TB).rate(Rational::new(30, 1))];
    for k in 0..held {
        bound.push(BoundStream::video("inputs", 10 + k, W, H, "rgba", TB));
    }
    bound
}

/// What left on `video`: the stream id handed back, or none for a new frame.
fn left(emitted: &ffrwd_node::Emitted) -> Option<u32> {
    match emitted.on("video").as_slice() {
        [Payload::Same { id, .. }] => Some(*id),
        [Payload::Frame { .. }] => None,
        other => panic!("one picture leaves a tick, not {other:?}"),
    }
}

const FULL: &str = "<style>html, body { margin: 0; background: #000; }
img { position: absolute; left: 0; top: 0; width: 100vw; height: 100vh; }
#b { display: none; position: absolute; left: 4px; top: 4px; width: 8px; height: 8px; background: red; }
#stage.on #b, #b.on { display: block; }</style>";

#[test]
fn compose_is_clocked_by_its_picture_and_holds_the_rest() {
    let s = shape("", &["v", "inputs"]).unwrap();
    assert_eq!(s.clock, Some(ClockKind::Input("v".into())));
    assert!(s.pure && s.one_to_one);
    let names: Vec<&str> = s.inputs.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["v", "inputs", "changes"]);
    let held = s.find_input("inputs").unwrap();
    assert!(held.many && !held.required);
    let Pairing::Hold(hold) = &held.pairing else {
        panic!("inputs are held")
    };
    assert_eq!(hold.anchor, Anchor::Tagged("smart_timed".into()));
    assert_eq!(
        (hold.lead, hold.linger, hold.timeout),
        (0.3, None, Some(1.0))
    );
    assert_eq!(hold.port_param.as_deref(), Some("port"));
    let alone = shape("", &["v"]).unwrap();
    let Pairing::Hold(hold) = &alone.find_input("inputs").unwrap().pairing else {
        panic!("inputs are held")
    };
    assert_eq!(
        hold.port_param, None,
        "nothing bound and no port: no listener"
    );
    let changes = s.find_input("changes").unwrap();
    assert!(changes.many && changes.rows == RowsUse::State);
    assert!(matches!(changes.pairing, Pairing::Interval(_)));
    let video = s.find_output("video").unwrap();
    assert_eq!(video.like.as_ref().unwrap().port.as_deref(), Some("v"));
}

#[test]
fn a_size_given_is_the_output_s_and_a_port_holds_an_input_on_it() {
    let s = shape(r#"{"width":1280,"height":720,"port":[9100]}"#, &["v"]).unwrap();
    assert!(matches!(
        s.find_output("video").unwrap().format,
        Some(Format::Video(VideoFormat {
            width: 1280,
            height: 720,
            ..
        }))
    ));
    let Pairing::Hold(hold) = &s.find_input("inputs").unwrap().pairing else {
        panic!("inputs are held")
    };
    assert_eq!(hold.port_param.as_deref(), Some("port"));
    let each = shape(r#"{"port":[9100,9101]}"#, &["v"]).unwrap();
    let Pairing::Hold(hold) = &each.find_input("inputs").unwrap().pairing else {
        panic!("inputs are held")
    };
    assert_eq!(hold.port_param.as_deref(), Some("port"));
    assert!(shape(r#"{"port":[0]}"#, &["v"]).is_err());
    assert!(shape(r#"{"port":9100}"#, &["v"]).is_ok());
    assert!(shape(r#"{"port":"9100"}"#, &["v"]).is_err());
}

#[test]
fn page_is_a_source_at_its_rate() {
    let s = shape(r#"{"width":640,"height":360,"fps":25}"#, &[]).unwrap();
    assert_eq!(s.clock, Some(ClockKind::Rate(Rational::new(25, 1))));
    assert!(s.pure && !s.bounded && s.find_input("v").is_none());
    assert_eq!(s.relation, [r#"{"width":640,"height":360}"#]);
    assert_eq!(s.find_output("video").unwrap().row, Some(0));
    let err = shape("", &[]).unwrap_err();
    assert!(err.contains("width and height"), "{err}");
}

#[test]
fn presence_spreads_over_workers() {
    let p = r##"{"presence":"{\"input\":1,\"on\":{\"select\":\"#stage\",\"change\":\"~on\"}}"}"##;
    assert!(shape(p, &["v", "inputs"]).unwrap().pure);
    assert!(shape(r#"{"presence":"{\"input\":1,\"colour\":1}"}"#, &["v"]).is_err());
}

#[test]
fn a_malformed_row_is_refused_before_anything_runs() {
    let err = shape(r#"{"rows":"[{\"select\":\"a\"}]"}"#, &["v"]).unwrap_err();
    assert!(err.contains("rows"), "{err}");
}

#[test]
fn whichever_input_is_drawn_whole_leaves_uncopied() {
    let html = format!(r#"{FULL}<img src="ffrwd:0"><img src="ffrwd:1">"#);
    let mut node = Harness::<Compose>::new(&params(&html, ""), streams(1)).unwrap();
    let held = node.process(&node.tick(0).frame(0, 0, picture(1))).unwrap();
    assert_eq!(left(&held), Some(0), "input 1 has no frame yet");
    let tick = node
        .tick(1)
        .frame(0, 1, picture(1))
        .frame(10, 7, picture(2));
    assert_eq!(left(&node.process(&tick).unwrap()), Some(10));
}

#[test]
fn a_page_renders_at_its_size() {
    let html = format!(r#"{FULL}<div id="b" class="on"></div>"#);
    let mut node =
        Harness::<Compose>::new(&params(&html, r#""width":32,"height":18"#), vec![]).unwrap();
    let emitted = node.process(&node.tick(3)).unwrap();
    let [Payload::Frame { pts, data, .. }] = emitted.on("video").as_slice() else {
        panic!("a page makes a picture a tick")
    };
    assert_eq!((*pts, data.len()), (3, 32 * 18 * 4));
    assert_eq!(&data[(5 * 32 + 5) * 4..][..4], &[255, 0, 0, 255]);
}

#[test]
fn rows_on_changes_are_state() {
    let html = format!(r#"{FULL}<img src="ffrwd:0"><div id="b"></div>"#);
    let mut bound = streams(0);
    bound.push(BoundStream::rows("changes", 20, TB));
    let mut node = Harness::<Compose>::new(&params(&html, ""), bound).unwrap();
    let quiet = node.process(&node.tick(0).frame(0, 0, picture(1))).unwrap();
    assert_eq!(left(&quiet), Some(0));
    let tick = node.tick(5).frame(0, 5, picture(1)).earlier(
        20,
        3,
        &[r##"{"select": "#b", "change": "+on"}"##],
    );
    assert_eq!(left(&node.process(&tick).unwrap()), None);
    let tick = node.tick(6).frame(0, 6, picture(1)).row(
        20,
        6,
        &serde_json::json!({"select": "#b", "change": "-on"}),
    );
    assert_eq!(left(&node.process(&tick).unwrap()), Some(0));
}

#[test]
fn a_feed_starting_and_ending_drives_presence() {
    let html = format!(r#"{FULL}<div id="stage"><img src="ffrwd:0"><div id="b"></div></div>"#);
    let presence = serde_json::to_string(
        r##"{"input": 1, "on": {"select": "#stage", "change": "~on"}, "lead_out": 0.1}"##,
    )
    .unwrap();
    let p = params(&html, &format!(r#""presence":{presence}"#));
    let mut node = Harness::<Compose>::new(&p, streams(1)).unwrap();
    let feed = |ends| Feed {
        start: FeedStart {
            tags: Vec::new(),
            first_pts: 0,
            at: 10,
            known: 5,
        },
        ends,
    };
    let mut on = Vec::new();
    for pts in 0..30 {
        let mut tick = node.tick(pts).frame(0, pts, picture(1));
        if (5..=20).contains(&pts) {
            tick = tick.feed(10, feed((pts >= 12).then_some(20)));
        }
        if left(&node.process(&tick).unwrap()).is_none() {
            on.push(pts);
        }
    }
    // On at the feed's first frame; off 0.1 s (three frames) before the
    // picture is the clock's own again, at tick 21.
    assert_eq!(on, (10..18).collect::<Vec<_>>());
}

#[test]
fn workers_handed_alternate_ticks_show_presence_as_one_handed_every_tick_does() {
    let html = format!(r#"{FULL}<div id="stage"><img src="ffrwd:0"><div id="b"></div></div>"#);
    let presence = serde_json::to_string(
        r##"{"input": 1, "on": {"select": "#stage", "change": "~on"}, "lead_out": 0.1}"##,
    )
    .unwrap();
    let p = params(&html, &format!(r#""presence":{presence}"#));
    let feed = |known, at, ends| Feed {
        start: FeedStart {
            tags: Vec::new(),
            first_pts: at,
            at,
            known,
        },
        ends,
    };
    // A start told ahead and an end told ahead, then a start seen as it
    // shows and an end no one was told of.
    let current = |pts: i64| match pts {
        3..=20 => Some(feed(3, 8, (pts >= 12).then_some(20))),
        26..=33 => Some(feed(26, 26, None)),
        _ => None,
    };
    let ended = |pts: i64| match pts {
        21 => Some(feed(3, 8, Some(20))),
        34 => Some(feed(26, 26, Some(33))),
        _ => None,
    };
    let shown = |workers: i64, k: i64| -> Vec<(i64, bool)> {
        let mut node = Harness::<Compose>::new(&p, streams(1)).unwrap();
        let mut gone = Vec::new();
        let mut out = Vec::new();
        for pts in 0..40 {
            gone.extend(ended(pts));
            if pts % workers != k {
                continue;
            }
            let mut tick = node.tick(pts).ordinal(pts as u64).frame(0, pts, picture(1));
            if let Some(f) = current(pts) {
                tick = tick.feed(10, f);
            }
            for f in gone.drain(..) {
                tick = tick.ended(10, f);
            }
            out.push((pts, left(&node.process(&tick).unwrap()).is_none()));
        }
        out
    };
    let every: std::collections::HashMap<i64, bool> = shown(1, 0).into_iter().collect();
    let on: Vec<i64> = (0..40).filter(|pts| every[pts]).collect();
    assert_eq!(
        on,
        (8..18).chain(26..34).collect::<Vec<_>>(),
        "on from each first frame; off 0.1 s ahead of a return told ahead, and on the return otherwise"
    );
    for workers in [2, 3] {
        for k in 0..workers {
            for (pts, is_on) in shown(workers, k) {
                assert_eq!(is_on, every[&pts], "worker {k} of {workers} at {pts}");
            }
        }
    }
}

#[test]
fn renderer_gpu_with_no_adapter_is_refused_and_cpu_never_asks() {
    // The native build has no GPU to offer, so `auto` renders on the CPU,
    // `gpu` is refused by name and `cpu` does not ask.
    let html = format!(r#"{FULL}<div id="b" class="on"></div>"#);
    for renderer in ["auto", "cpu"] {
        let p = params(
            &html,
            &format!(r#""width":32,"height":18,"renderer":"{renderer}""#),
        );
        let mut node = Harness::<Compose>::new(&p, vec![]).unwrap();
        let emitted = node.process(&node.tick(0)).unwrap();
        let [Payload::Frame { data, .. }] = emitted.on("video").as_slice() else {
            panic!("a page makes a picture a tick")
        };
        assert_eq!(&data[(5 * 32 + 5) * 4..][..4], &[255, 0, 0, 255]);
    }
    let p = params(&html, r#""width":32,"height":18,"renderer":"gpu""#);
    let err = match Harness::<Compose>::new(&p, vec![]) {
        Ok(_) => panic!("renderer => 'gpu' with no adapter opened"),
        Err(e) => e,
    };
    assert!(err.contains("renderer => 'gpu'"), "{err}");
    assert!(shape(r#"{"renderer":"vulkan"}"#, &["v"]).is_err());
}

#[test]
fn the_video_inputs_take_yuv420p_on_every_renderer() {
    // rgba first, which the compiler puts on the wire.
    for renderer in ["cpu", "auto", "gpu"] {
        let s = shape(&format!(r#"{{"renderer":"{renderer}"}}"#), &["v", "inputs"]).unwrap();
        for port in ["v", "inputs"] {
            assert_eq!(
                s.find_input(port).unwrap().accepts.pixel_formats,
                ["rgba", "yuv420p"],
                "{port} with renderer => '{renderer}'"
            );
        }
        assert_eq!(s.outputs[0].name, "video");
    }
}

/// A `width` x `height` yuv420p picture whose samples run over every code:
/// luma across, Cb down and Cr along the diagonal.
fn yuv_picture(width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let mut data = vec![0u8; w * h + 2 * cw * ch];
    let (luma, chroma) = data.split_at_mut(w * h);
    for (i, y) in luma.iter_mut().enumerate() {
        *y = ((i % w) * 255 / (w - 1).max(1)) as u8 ^ ((i / w) as u8 & 7);
    }
    let (cb, cr) = chroma.split_at_mut(cw * ch);
    for (i, (u, v)) in cb.iter_mut().zip(cr.iter_mut()).enumerate() {
        let (x, y) = (i % cw, i / cw);
        *u = (y * 255 / (ch - 1).max(1)) as u8;
        *v = ((x + y) * 255 / (cw + ch - 2).max(1)) as u8;
    }
    data
}

/// A yuv420p stream of `w` x `h` on `port` in `space` and `range`, as the
/// host tells it.
fn yuv_stream(port: &str, id: u32, (w, h): (u32, u32), space: &str, range: &str) -> BoundStream {
    let mut stream = BoundStream::video(port, id, w, h, "yuv420p", TB);
    stream.format = Some(Format::Video(VideoFormat {
        width: w,
        height: h,
        pix_fmt: "yuv420p".to_owned(),
        color: Some(ffrwd_node::ColorInfo {
            range: range.to_owned(),
            primaries: "unknown".to_owned(),
            trc: "unknown".to_owned(),
            space: space.to_owned(),
        }),
    }));
    if port == "v" {
        stream = stream.rate(Rational::new(30, 1));
    }
    stream
}

/// What `yuv` is as straight rgba in `colour`, through the rows the GPU
/// path's conversion pass uses, each chroma sample over its two by two
/// pixels.
fn expected_rgba(yuv: &[u8], (w, h): (u32, u32), colour: Colour) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let rows = colour.rows();
    let mut out = vec![255u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let c = (y / 2) * cw + x / 2;
            let s = [yuv[y * w + x], yuv[w * h + c], yuv[w * h + cw * ch + c]]
                .map(|s| f64::from(s) / 255.0);
            for (k, row) in rows.iter().enumerate() {
                let v = f64::from(row[0]) * s[0]
                    + f64::from(row[1]) * s[1]
                    + f64::from(row[2]) * s[2]
                    + f64::from(row[3]);
                out[(y * w + x) * 4 + k] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
    out
}

/// The largest difference between two rgba pictures on any channel.
fn largest_difference(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0)
}

/// Colours a stream may say, and how each reads: an unsettled space is
/// BT.601 and an unsettled range limited, as on the GPU path.
const COLOURS: [(&str, &str, Matrix, bool); 6] = [
    ("bt709", "tv", Matrix::Bt709, false),
    ("bt709", "pc", Matrix::Bt709, true),
    ("smpte170m", "tv", Matrix::Bt601, false),
    ("bt470bg", "pc", Matrix::Bt601, true),
    ("unknown", "unknown", Matrix::Bt601, false),
    ("unknown", "pc", Matrix::Bt601, true),
];

#[test]
fn on_the_cpu_a_yuv420p_picture_is_converted_in_its_own_colour() {
    let yuv = yuv_picture(W, H);
    let drawn = format!(r#"{FULL}<img src="ffrwd:0">"#);
    for (space, range, matrix, full) in COLOURS {
        let colour = Colour { matrix, full };
        assert_eq!(Colour::of(space, range), colour, "{space} {range}");
        let want = expected_rgba(&yuv, (W, H), colour);
        // Handed back whole, and drawn by the document.
        for (bypass, near) in [("true", 1), ("false", 2)] {
            let p = params(&drawn, &format!(r#""bypass":{bypass}"#));
            let v = yuv_stream("v", 0, (W, H), space, range);
            let mut node = Harness::<Compose>::new(&p, vec![v])
                .unwrap_or_else(|e| panic!("{space} {range}: {e}"));
            let emitted = node
                .process(&node.tick(0).frame(0, 0, yuv.clone()))
                .unwrap();
            let [Payload::Frame { data, .. }] = emitted.on("video").as_slice() else {
                panic!("a yuv420p picture leaves as a new rgba frame on the CPU")
            };
            let off = largest_difference(data, &want);
            assert!(
                off <= near,
                "{space} {range}, bypass {bypass}: {off} levels off the GPU path's matrix"
            );
        }
    }
}

#[test]
fn a_yuv420p_picture_of_the_wrong_size_draws_nothing() {
    let p = params(
        &format!(r#"{FULL}<img src="ffrwd:0">"#),
        r#""bypass":false"#,
    );
    let v = yuv_stream("v", 0, (W, H), "bt709", "tv");
    let mut node = Harness::<Compose>::new(&p, vec![v]).unwrap();
    let emitted = node
        .process(&node.tick(0).frame(0, 0, vec![16; 100]))
        .unwrap();
    let [Payload::Frame { data, .. }] = emitted.on("video").as_slice() else {
        panic!("a frame leaves")
    };
    assert!(data.as_chunks::<4>().0.iter().all(|p| p == &[0, 0, 0, 255]));
}

#[test]
fn a_held_yuv420p_input_shows_converted_beside_an_rgba_programme() {
    // The held input on the left half, `v` on the right.
    let html = r#"<style>html, body { margin: 0; background: #000; }
img { position: absolute; top: 0; height: 100vh; width: 50vw; }
#a { left: 0; } #b { left: 50vw; }</style><img id="b" src="ffrwd:0"><img id="a" src="ffrwd:1">"#;
    let half = (W / 2, H);
    let yuv = yuv_picture(half.0, half.1);
    let mut bound = streams(0);
    bound.push(yuv_stream("inputs", 10, half, "bt709", "pc"));
    let mut node = Harness::<Compose>::new(&params(html, ""), bound).unwrap();
    let tick = node
        .tick(0)
        .frame(0, 0, picture(9))
        .frame(10, 0, yuv.clone());
    let emitted = node.process(&tick).unwrap();
    let [Payload::Frame { data, .. }] = emitted.on("video").as_slice() else {
        panic!("a rendered frame leaves")
    };
    let want = expected_rgba(&yuv, half, Colour::of("bt709", "pc"));
    let w = W as usize;
    for y in 0..H as usize {
        let row = &data[y * w * 4..][..w * 2];
        let off = largest_difference(row, &want[y * w * 2..][..w * 2]);
        assert!(off <= 2, "row {y}: {off} levels off");
        assert_eq!(data[(y * w + w / 2) * 4], 9, "the programme on the right");
    }
}

/// ffmpeg's yuv420p to rgba of `frame` (`w` x `h`) in `matrix` and
/// `range`, swscale given `flags`; None when there is no ffmpeg on the PATH.
fn ffmpeg_rgba(
    frame: &[u8],
    (w, h): (u32, u32),
    matrix: &str,
    range: &str,
    flags: &str,
) -> Option<Vec<u8>> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let size = format!("{w}x{h}");
    let filter =
        format!("scale=in_color_matrix={matrix}:in_range={range}:flags={flags},format=rgba");
    let mut child = Command::new("ffmpeg")
        .args(["-v", "error", "-f", "rawvideo", "-pix_fmt", "yuv420p"])
        .args([
            "-s", &size, "-i", "-", "-vf", &filter, "-f", "rawvideo", "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take().unwrap();
    let input = frame.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    assert!(
        out.status.success(),
        "ffmpeg: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Some(out.stdout)
}

#[test]
fn the_conversion_on_the_cpu_answers_what_ffmpeg_answers() {
    // swscale computed exactly (`accurate_rnd` and `full_chroma_int`, each
    // chroma sample over its two by two pixels) is within one level; its
    // default fast path rounds coarsely and is within three.
    let size = (320, 180);
    let luma = (size.0 * size.1) as usize;
    let chroma = luma / 4;
    for (space, range, matrix, full) in COLOURS {
        let mut yuv = yuv_picture(size.0, size.1);
        if !full {
            // swscale's exact path wraps a blue above 511.5 counts round to
            // black, which a limited-range luma near 255 with a Cb near 255
            // reaches. Holding Cb to 236 keeps every pixel clear of it.
            for u in &mut yuv[luma..luma + chroma] {
                *u = (*u).min(236);
            }
        }
        let named = match matrix {
            Matrix::Bt601 => "bt601",
            Matrix::Bt709 => "bt709",
        };
        let swscale_range = if full { "pc" } else { "tv" };
        let v = yuv_stream("v", 0, size, space, range);
        let mut node = Harness::<Compose>::new(&params("", ""), vec![v]).unwrap();
        let emitted = node
            .process(&node.tick(0).frame(0, 0, yuv.clone()))
            .unwrap();
        let [Payload::Frame { data, .. }] = emitted.on("video").as_slice() else {
            panic!("a yuv420p picture leaves converted")
        };
        for (flags, near) in [("neighbor+accurate_rnd+full_chroma_int", 1), ("bicubic", 3)] {
            let Some(theirs) = ffmpeg_rgba(&yuv, size, named, swscale_range, flags) else {
                eprintln!("SKIPPED: no ffmpeg on the PATH");
                return;
            };
            let off = largest_difference(data, &theirs);
            eprintln!("{space} {range} against swscale {flags}: largest difference {off}");
            assert!(
                off <= near,
                "{space} {range}: {off} levels off swscale {flags}"
            );
        }
    }
}
