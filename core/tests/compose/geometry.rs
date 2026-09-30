//! Where the document lands: the design size painted at a device scale,
//! and canvases of other shapes fitted into the output.

use compose_core::Session;
use compose_core::geometry::Fit;
use compose_core::params::Params;

use crate::common::*;

/// A design `dw` x `dh` CSS px: a green page with a red 100x100 box at
/// (100, 100) and a blue box in the bottom-right corner.
fn marked(dw: u32, dh: u32) -> String {
    doc(
        &format!(
            "html, body {{ background: #00ff00; }}
             #r {{ position: absolute; left: 100px; top: 100px; width: 100px; height: 100px; background: #ff0000; }}
             #b {{ position: absolute; left: {}px; top: {}px; width: 50px; height: 50px; background: #0000ff; }}",
            dw - 50,
            dh - 50
        ),
        r#"<div id="r"></div><div id="b"></div>"#,
    )
}

fn frame(p: Params, out: (u32, u32), input: (u32, u32)) -> Vec<u8> {
    let mut s = Session::new(p, out, input, 1);
    let f = |_: u32| panic!("the document shows no video");
    let mut f = f;
    bytes(s.frame(0.0, &mut f))
}

fn rgb(f: &[u8], w: u32, x: u32, y: u32) -> [u8; 3] {
    let p = px(f, w, x, y);
    [p[0], p[1], p[2]]
}

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const BLACK: [u8; 3] = [0, 0, 0];

#[test]
fn css_px_are_frame_px_by_default() {
    let f = frame(params(&marked(640, 360)), (640, 360), (640, 360));
    assert_eq!(rgb(&f, 640, 99, 150), GREEN);
    assert_eq!(rgb(&f, 640, 100, 150), RED);
    assert_eq!(rgb(&f, 640, 199, 150), RED);
    assert_eq!(rgb(&f, 640, 200, 150), GREEN);
    assert_eq!(rgb(&f, 640, 639, 359), BLUE);
}

#[test]
fn one_design_serves_720p_1080p_and_4k() {
    for (w, h) in [(1280u32, 720u32), (1920, 1080), (3840, 2160)] {
        let mut p = params(&marked(1280, 720));
        p.css_width = Some(1280.0);
        let f = frame(p, (w, h), (w, h));
        let s = w / 1280; // 1, 1 (1.5 floors), 3
        let scale = w as f64 / 1280.0;
        let at = |css: f64| (css * scale) as u32;
        assert_eq!(rgb(&f, w, at(100.0), at(150.0)), RED, "{w}");
        assert_eq!(rgb(&f, w, at(199.0) + s - 1, at(150.0)), RED, "{w}");
        assert_eq!(rgb(&f, w, at(100.0) - 1, at(150.0)), GREEN, "{w}");
        assert_eq!(rgb(&f, w, at(200.0), at(150.0)), GREEN, "{w}");
        assert_eq!(rgb(&f, w, w - 1, h - 1), BLUE, "{w}");
    }
}

#[test]
fn a_16_9_canvas_fills_a_16_9_output() {
    let mut p = params(&marked(1920, 1080));
    p.width = Some(1920.0);
    p.height = Some(1080.0);
    let f = frame(p, (960, 540), (960, 540));
    assert_eq!(rgb(&f, 960, 0, 0), GREEN);
    assert_eq!(rgb(&f, 960, 60, 75), RED);
    assert_eq!(rgb(&f, 960, 959, 539), BLUE);
}

#[test]
fn a_9_16_canvas_is_pillarboxed_in_a_16_9_output() {
    let mut p = params(&marked(1080, 1920));
    p.width = Some(1080.0);
    p.height = Some(1920.0);
    let f = frame(p, (1920, 1080), (1920, 1080));
    // The canvas scales by 1080/1920 to 607.5 px wide, centred: 656..1264.
    assert_eq!(rgb(&f, 1920, 655, 540), BLACK);
    assert_eq!(rgb(&f, 1920, 657, 540), GREEN);
    assert_eq!(rgb(&f, 1920, 1262, 540), GREEN);
    assert_eq!(rgb(&f, 1920, 1265, 540), BLACK);
    assert_eq!(rgb(&f, 1920, 656 + 84, 84), RED);
    assert_eq!(rgb(&f, 1920, 1262, 1078), BLUE);
    let scale = 1080.0 / 1920.0;
    let mut s = Session::new(
        {
            let mut p = params(&marked(1080, 1920));
            p.width = Some(1080.0);
            p.height = Some(1920.0);
            p
        },
        (1920, 1080),
        (1920, 1080),
        1,
    );
    assert_eq!(s.compositor().scale(), scale);
}

#[test]
fn a_square_canvas_is_pillarboxed_in_a_16_9_output() {
    let mut p = params(&marked(1080, 1080));
    p.width = Some(1080.0);
    p.height = Some(1080.0);
    let f = frame(p, (1920, 1080), (1920, 1080));
    assert_eq!(rgb(&f, 1920, 419, 540), BLACK);
    assert_eq!(rgb(&f, 1920, 421, 540), GREEN);
    assert_eq!(rgb(&f, 1920, 420 + 150, 150), RED);
    assert_eq!(rgb(&f, 1920, 1498, 1078), BLUE);
    assert_eq!(rgb(&f, 1920, 1501, 540), BLACK);
}

#[test]
fn a_16_9_design_in_a_square_canvas_is_letterboxed_then_pillarboxed() {
    let mut p = params(&marked(1280, 720));
    p.width = Some(1080.0);
    p.height = Some(1080.0);
    p.css_width = Some(1280.0);
    p.css_height = Some(720.0);
    let f = frame(p, (1920, 1080), (1920, 1080));
    // The square is 420..1500; the design fills its width, 607.5 tall, centred.
    assert_eq!(rgb(&f, 1920, 960, 230), BLACK);
    assert_eq!(rgb(&f, 1920, 960, 240), GREEN);
    assert_eq!(rgb(&f, 1920, 960, 850), BLACK);
    assert_eq!(rgb(&f, 1920, 419, 540), BLACK);
}

#[test]
fn stretch_fills_the_canvas_on_each_axis() {
    let mut p = params(&marked(1000, 1000));
    p.css_width = Some(1000.0);
    p.css_height = Some(1000.0);
    p.fit = Fit::Stretch;
    let f = frame(p, (1920, 1080), (1920, 1080));
    assert_eq!(rgb(&f, 1920, 0, 0), GREEN);
    // The red box: x 192..384, y 108..216.
    assert_eq!(rgb(&f, 1920, 193, 110), RED);
    assert_eq!(rgb(&f, 1920, 382, 214), RED);
    assert_eq!(rgb(&f, 1920, 190, 150), GREEN);
    assert_eq!(rgb(&f, 1920, 1919, 1079), BLUE);
}

/// What a host that let the output differ from the inputs would get: a
/// 9:16 output built from a 16:9 input placed as an image.
#[test]
fn a_portrait_output_from_a_landscape_input() {
    let html = doc(
        "html, body { background: #000; }
         #v { position: absolute; left: 0; top: 656px; width: 1080px; height: 608px; }",
        r#"<img id="v" src="ffrwd:0">"#,
    );
    let mut s = Session::new(params(&html), (1080, 1920), (1920, 1080), 1);
    let mut fetch = |_: u32| picture(1920, 1080, 0, 0);
    let f = bytes(s.frame(0.0, &mut fetch));
    assert_eq!(f.len(), 1080 * 1920 * 4);
    assert_eq!(rgb(&f, 1080, 540, 100), BLACK);
    let src = picture(1920, 1080, 0, 0);
    let got = px(&f, 1080, 540, 960);
    let want = px(&src, 1920, 960, 540);
    assert!(
        got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 8),
        "{got:?} {want:?}"
    );
}
