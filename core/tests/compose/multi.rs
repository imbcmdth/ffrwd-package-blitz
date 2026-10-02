//! Rows on a module reading several inputs: they arrive with pad 0's
//! frames and drive the document as they do on one input.

use crate::common::*;
use crate::joiner::{H, W, page, rows_at};

const FRAMES: usize = 300;

/// Every frame and its rows through one instance reading `inputs` inputs:
/// the frames, the inputs fetched per frame, and the log's (time, source)
/// entries.
fn run(html: &str, inputs: u32) -> (Vec<Vec<u8>>, Vec<Vec<u32>>, Vec<String>) {
    let mut s = session(html, W, H, inputs);
    let mut frames = Vec::new();
    let mut fetched = Vec::new();
    for i in 0..FRAMES {
        s.fold(ft(i), &rows_at(i));
        let (out, f) = render(&mut s, W, H, ft(i), i);
        frames.push(bytes(out));
        fetched.push(f);
    }
    assert!(s.take_errors().is_empty());
    let log = s
        .timeline()
        .log()
        .iter()
        .map(|t| format!("{:.6} {:?} {}", t.at, t.source, t.row.to_value()))
        .collect();
    (frames, fetched, log)
}

#[test]
fn rows_on_pad_0_drive_two_and_three_inputs_as_they_drive_one() {
    // The page draws input 0 alone, so the same rows must give the same
    // frames however many inputs the instance reads.
    let (one, one_fetched, one_log) = run(&page(), 1);
    assert!(!one_log.is_empty());
    for inputs in [2, 3] {
        let (many, fetched, log) = run(&page(), inputs);
        assert_eq!(log, one_log, "{inputs} inputs: the log differs");
        assert_eq!(fetched, one_fetched, "{inputs} inputs: fetches differ");
        for (i, (a, b)) in one.iter().zip(&many).enumerate() {
            assert_eq!(diff(a, b), (0, 0), "{inputs} inputs: frame {i} differs");
        }
    }
}

const TWO_CSS: &str = "
#a, #b { position: absolute; left: 0; top: 0; width: 64px; height: 64px; }
#b { display: none; }
#b.on { display: block; }";
const TWO_BODY: &str = r#"<img id="a" src="ffrwd:0"><img id="b" src="ffrwd:1">"#;

#[test]
fn a_row_on_pad_0_shows_the_second_input_at_its_time() {
    let html = doc(TWO_CSS, TWO_BODY);
    let mut s = session(&html, 64, 64, 2);
    let pad = |input: u32, n: usize| picture(64, 64, input, n);
    let frame = |s: &mut compose_core::Session, n: usize, rows: &[&str]| {
        s.fold(ft(n), &lines(rows));
        let mut fetched = Vec::new();
        let out = s.frame(ft(n), &mut |i| {
            fetched.push(i);
            pad(i, n)
        });
        (out, fetched)
    };
    // Nothing but input 0: the bypass, nothing fetched.
    let (out, fetched) = frame(&mut s, 0, &[]);
    assert_eq!(out, compose_core::Output::Same(0));
    assert!(fetched.is_empty());
    // A row arriving with frame 3, scheduled for frame 6's time: no change
    // until then.
    let at = ft(6);
    let row = format!(r##"{{"at": {at}, "select": "#b", "change": "+on"}}"##);
    let (out, _) = frame(&mut s, 3, &[&row]);
    assert_eq!(out, compose_core::Output::Same(0));
    let (out, _) = frame(&mut s, 5, &[]);
    assert_eq!(out, compose_core::Output::Same(0));
    // From its time on the frame is input 1, which covers input 0: handed
    // back as it is, and rendering it gives the same picture.
    let (out, fetched) = frame(&mut s, 6, &[]);
    assert_eq!(out, compose_core::Output::Same(1));
    assert!(fetched.is_empty(), "fetched {fetched:?}");
    s.params.bypass = false;
    let (out, fetched) = frame(&mut s, 7, &[]);
    assert!(fetched.contains(&1), "fetched {fetched:?}");
    assert_eq!(diff(&bytes(out), &pad(1, 7)), (0, 0));
    s.params.bypass = true;
    // An array in one line, with no `at`: the frame it arrives with.
    let (out, fetched) = frame(
        &mut s,
        9,
        &[r##"[{"select": "#b", "change": "-on"}, {"select": "#a", "change": "+seen"}]"##],
    );
    assert_eq!(out, compose_core::Output::Same(0));
    assert!(fetched.is_empty());
    assert_eq!(s.timeline().log().len(), 3);
}
