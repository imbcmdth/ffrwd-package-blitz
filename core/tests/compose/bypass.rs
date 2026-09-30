//! Which inputs a frame fetches, and when it is input 0 untouched.

use compose_core::Output;

use crate::common::*;

const W: u32 = 320;
const H: u32 = 180;

fn full(extra_css: &str, extra_body: &str) -> String {
    doc(
        &format!(
            "#v0 {{ position: absolute; left: 0; top: 0; width: 100vw; height: 100vh; }}\n{extra_css}"
        ),
        &format!(r#"<img id="v0" src="ffrwd:0">{extra_body}"#),
    )
}

fn first(html: &str, inputs: u32) -> (Output, Vec<u32>) {
    let mut s = session(html, W, H, inputs);
    render(&mut s, W, H, 0.5, 0)
}

#[test]
fn the_video_alone_over_the_frame_is_handed_back_unfetched() {
    let (out, fetched) = first(&full("", ""), 1);
    assert!(matches!(out, Output::Same));
    assert!(fetched.is_empty());
    // The default document is the same thing.
    let mut s = compose_core::Session::new(
        compose_core::Params {
            log: compose_core::params::Log::Off,
            ..Default::default()
        },
        (W, H),
        (W, H),
        1,
    );
    let (out, fetched) = render(&mut s, W, H, 0.0, 0);
    assert!(matches!(out, Output::Same) && fetched.is_empty());
}

#[test]
fn rendering_the_bypassed_frame_gives_the_input_back_exactly() {
    let mut s = session(&full("", ""), W, H, 1);
    s.params.bypass = false;
    let (out, fetched) = render(&mut s, W, H, 0.5, 3);
    assert_eq!(fetched, vec![0]);
    assert_eq!(diff(&bytes(out), &picture(W, H, 0, 3)), (0, 0));
}

#[test]
fn anything_over_the_video_renders() {
    for (css, body) in [
        (
            "#o { position: absolute; left: 10px; top: 10px; width: 20px; height: 20px; background: red; }",
            r#"<div id="o"></div>"#,
        ),
        (
            "#o { position: absolute; left: 10px; top: 10px; color: white; }",
            r#"<div id="o">text</div>"#,
        ),
        (
            "#o { position: absolute; left: 10px; top: 10px; width: 20px; height: 20px; box-shadow: 0 0 8px #000; }",
            r#"<div id="o"></div>"#,
        ),
        ("#v0 { opacity: 0.5; }", ""),
        ("#v0 { transform: translateX(1px); }", ""),
        ("#v0 { width: 50vw; }", ""),
        ("#v0 { border-radius: 8px; }", ""),
    ] {
        let (out, fetched) = first(&full(css, body), 1);
        assert!(matches!(out, Output::New(_)), "{css}");
        assert_eq!(fetched, vec![0], "{css}");
    }
}

#[test]
fn what_cannot_show_does_not_stop_the_bypass() {
    for (css, body) in [
        (
            "#o { position: absolute; left: 10px; top: 10px; width: 20px; height: 20px; background: red; opacity: 0; }",
            r#"<div id="o"></div>"#,
        ),
        (
            "#o { position: absolute; left: 10px; top: 10px; width: 20px; height: 20px; background: red; display: none; }",
            r#"<div id="o"></div>"#,
        ),
        (
            "#o { position: absolute; left: 10px; top: 10px; width: 20px; height: 20px; background: red; visibility: hidden; }",
            r#"<div id="o"></div>"#,
        ),
        (
            "#o { position: absolute; left: 1000px; top: 10px; width: 20px; height: 20px; background: red; }",
            r#"<div id="o"></div>"#,
        ),
        (
            "#o { position: absolute; left: 10px; top: 10px; color: white; transform: translateX(-900px); }",
            r#"<div id="o">a lower third</div>"#,
        ),
        (
            "#o { position: absolute; left: 10px; top: 10px; width: 20px; height: 20px; background: transparent; }",
            r#"<div id="o"></div>"#,
        ),
        // Behind the video, fully covered.
        (
            "#o { position: absolute; left: 10px; top: 10px; width: 20px; height: 20px; background: red; }",
            r#"<div id="o"></div><img id="v0b" src="ffrwd:0" style="position:absolute;left:0;top:0;width:100vw;height:100vh">"#,
        ),
    ] {
        let (out, fetched) = first(&full(css, body), 1);
        assert!(matches!(out, Output::Same), "{css} {body}");
        assert!(fetched.is_empty(), "{css}");
    }
}

#[test]
fn only_the_inputs_drawn_are_fetched() {
    let two = |css: &str, body: &str| first(&doc(css, body), 2).1;
    let pos = "position: absolute; top: 0; width: 100px; height: 100px;";
    assert_eq!(
        two(
            &format!("img {{ {pos} }} #b {{ left: 150px; }}"),
            r#"<img src="ffrwd:0"><img id="b" src="ffrwd:1">"#
        ),
        vec![0, 1]
    );
    assert_eq!(
        two(&format!("img {{ {pos} }}"), r#"<img src="ffrwd:1">"#),
        vec![1]
    );
    assert_eq!(
        two(
            &format!("img {{ {pos} }} #a {{ display: none; }}"),
            r#"<img id="a" src="ffrwd:0"><img src="ffrwd:1">"#
        ),
        vec![1]
    );
    assert_eq!(
        two(
            &format!("img {{ {pos} }} #a {{ left: 2000px; }}"),
            r#"<img id="a" src="ffrwd:0"><img src="ffrwd:1">"#
        ),
        vec![1]
    );
    // An input this instance does not read draws nothing.
    assert_eq!(
        two(&format!("img {{ {pos} }}"), r#"<img src="ffrwd:5">"#),
        Vec::<u32>::new()
    );
    // Input 1 over the whole frame is not a bypass: `same` is input 0.
    let (out, fetched) = first(
        &doc(
            "img { position: absolute; left: 0; top: 0; width: 100vw; height: 100vh; }",
            r#"<img src="ffrwd:1">"#,
        ),
        2,
    );
    assert!(matches!(out, Output::New(_)));
    assert_eq!(fetched, vec![1]);
}

#[test]
fn a_design_width_keeps_the_bypass() {
    let html = doc(
        "#v0 { position: absolute; left: 0; top: 0; width: 1280px; height: 720px; }",
        r#"<img id="v0" src="ffrwd:0">"#,
    );
    let mut p = params(&html);
    p.css_width = Some(1280.0);
    let mut s = compose_core::Session::new(p, (1920, 1080), (1920, 1080), 1);
    let (out, fetched) = render(&mut s, 1920, 1080, 0.0, 0);
    assert!(matches!(out, Output::Same));
    assert!(fetched.is_empty());
}

#[test]
fn a_letterboxed_canvas_never_bypasses() {
    let mut p = params(&full("", ""));
    p.width = Some(1080.0);
    p.height = Some(1920.0);
    let mut s = compose_core::Session::new(p, (W, H), (W, H), 1);
    let (out, fetched) = render(&mut s, W, H, 0.0, 0);
    assert!(matches!(out, Output::New(_)));
    assert_eq!(fetched, vec![0]);
}

#[test]
fn a_background_image_of_the_video_is_frame_exact() {
    let html = doc(
        "#bg { position: absolute; left: 0; top: 0; width: 100vw; height: 100vh;
               background-image: url(ffrwd:0); background-size: 100% 100%; }
         #o { position: absolute; left: 0; top: 0; width: 4px; height: 4px; background: #fff; }",
        r#"<div id="bg"></div><div id="o"></div>"#,
    );
    let mut s = session(&html, W, H, 1);
    for n in 0..5 {
        let (out, fetched) = render(&mut s, W, H, ft(n), n);
        assert_eq!(fetched, vec![0], "frame {n}");
        let f = bytes(out);
        let want = picture(W, H, 0, n);
        for (x, y) in [(100, 50), (300, 170), (5, 170)] {
            assert_eq!(px(&f, W, x, y), px(&want, W, x, y), "frame {n} at {x},{y}");
        }
    }
}

#[test]
fn a_background_image_of_the_video_alone_is_a_bypass() {
    let html = doc(
        "#bg { position: absolute; left: 0; top: 0; width: 100vw; height: 100vh;
               background-image: url(ffrwd:0); background-size: 100% 100%; }",
        r#"<div id="bg"></div>"#,
    );
    let (out, fetched) = first(&html, 1);
    assert!(matches!(out, Output::Same));
    assert!(fetched.is_empty());
}

#[test]
fn a_video_inserted_by_a_row_shows_on_the_frame_it_lands() {
    let html = doc(
        "#slot img { position: absolute; left: 20px; top: 20px; width: 160px; height: 90px; }",
        r#"<div id="slot"></div>"#,
    );
    let mut s = session(&html, W, H, 1);
    s.fold(
        0.0,
        &lines(&[r##"{"at": 0.2, "select": "#slot", "html": "<img src=\"ffrwd:0\">"}"##]),
    );
    let (_, fetched) = render(&mut s, W, H, ft(5), 5);
    assert!(fetched.is_empty());
    let (out, fetched) = render(&mut s, W, H, ft(6), 6);
    assert_eq!(fetched, vec![0]);
    let f = bytes(out);
    // The picture is scaled into 160x90 at 20,20: its centre is the
    // picture's centre.
    let got = px(&f, W, 100, 65);
    let want = px(&picture(W, H, 0, 6), W, 160, 90);
    let d = got
        .iter()
        .zip(want)
        .map(|(a, b)| a.abs_diff(b))
        .max()
        .unwrap();
    assert!(d <= 8, "{got:?} {want:?}");
}
