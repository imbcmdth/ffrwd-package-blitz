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
    let mut bound = vec![BoundStream::video("v", 0, W, H, "rgba", TB)];
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
fn presence_runs_on_one_worker() {
    let p = r##"{"presence":"{\"input\":1,\"on\":{\"select\":\"#stage\",\"change\":\"~on\"}}"}"##;
    assert!(!shape(p, &["v", "inputs"]).unwrap().pure);
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
