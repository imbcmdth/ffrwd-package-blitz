//! A late joiner renders what an instance that saw everything renders.

use compose_core::Session;

use crate::common::*;

pub const PAGE_CSS: &str = "
html, body { background: #0b1d3a; font-family: sans-serif; }
#prog { position: absolute; left: 0; top: 0; width: 320px; height: 180px; }
#logo { position: absolute; left: 270px; top: 10px; width: 30px; height: 30px; border-radius: 15px;
        background: #1e90ff; animation: pulse 1.3s ease-in-out infinite; }
@keyframes pulse { 0%, 100% { transform: scale(1); opacity: 0.6 } 50% { transform: scale(1.25); opacity: 1 } }
#lower { position: absolute; left: 20px; top: 130px; width: 190px; height: 30px;
         background: rgba(10, 20, 60, 0.85); color: white; font-size: 16px;
         transform: translateX(-220px); opacity: 0;
         transition: transform 1.5s ease-in-out, opacity 1.5s linear; }
#lower.on { transform: translateX(0); opacity: 1; }
#slot .spin { position: absolute; left: 150px; top: 15px; width: 20px; height: 20px; background: #fc4a1a;
              animation: spin 2s linear infinite; }
@keyframes spin { to { transform: rotate(360deg) } }
.big { font-size: 22px; }";

pub const PAGE_BODY: &str = r#"<img id="prog" src="ffrwd:0"><div id="logo"></div><div id="lower"><span id="name">Jane Example</span></div><div id="slot"></div>"#;

/// Rows by the frame they arrive with: (frame, line).
pub fn stream_rows() -> Vec<(usize, &'static str)> {
    vec![
        (
            40,
            r##"{"at": 1.51, "select": "#slot", "html": "<div class=\"spin\"></div>"}"##,
        ),
        (55, r##"{"at": 2.02, "select": "#lower", "change": "+on"}"##),
        (90, r##"{"select": "#name", "text": "John Example"}"##),
        (
            120,
            r##"[{"at": 4.21, "select": "#lower", "change": "-on"}, {"at": 4.21, "select": "#name", "change": "+big"}]"##,
        ),
        (
            150,
            r##"{"at": 5.05, "select": "#slot", "html": "<div class=\"spin\"></div><div class=\"spin\" style=\"left: 190px; animation-duration: 0.7s\"></div>"}"##,
        ),
        (
            200,
            r##"{"at": 6.91, "select": "#lower", "change": "+on"}"##,
        ),
        (215, r##"{"at": 1.0, "select": "#name", "change": "-big"}"##),
        (216, r##"{"at": 7.31, "select": "#name", "text": "Live"}"##),
        (
            230,
            r##"{"at": 8.01, "select": "#lower", "change": "-on"}"##,
        ),
        (
            260,
            r##"{"select": "#slot", "html": "<div class=\"spin\" style=\"left: 100px\"></div>"}"##,
        ),
    ]
}

pub fn rows_at(frame: usize) -> Vec<String> {
    stream_rows()
        .into_iter()
        .filter(|(f, _)| *f == frame)
        .map(|(_, l)| l.to_string())
        .collect()
}

pub const W: u32 = 320;
pub const H: u32 = 180;

pub fn page() -> String {
    doc(PAGE_CSS, PAGE_BODY)
}

#[test]
fn a_late_joiner_built_from_the_log_renders_the_same_frames() {
    let join = 219; // 7.3 s
    let mut a = session(&page(), W, H, 1);
    for i in 0..join {
        a.fold(ft(i), &rows_at(i));
        render(&mut a, W, H, ft(i), i);
    }
    // What A has applied since its last full-page replace, and what it holds
    // for later.
    let log: Vec<_> = a
        .timeline()
        .log()
        .iter()
        .chain(a.timeline().pending_rows())
        .cloned()
        .collect();
    assert!(!log.is_empty());
    let mut b = Session::join(params(&page()), (W, H), (W, H), 1, &log);
    let mut moving = 0;
    let mut prev: Option<Vec<u8>> = None;
    for i in join..=join + 90 {
        let rows = rows_at(i);
        a.fold(ft(i), &rows);
        b.fold(ft(i), &rows);
        let fa = bytes(render(&mut a, W, H, ft(i), i).0);
        let fb = bytes(render(&mut b, W, H, ft(i), i).0);
        assert_eq!(diff(&fa, &fb), (0, 0), "frame {i} differs");
        if prev.as_ref().is_some_and(|p| diff(p, &fa).1 > 0) {
            moving += 1;
        }
        prev = Some(fa);
    }
    // The comparison can fail: the page moves on every frame.
    assert!(moving > 80, "{moving}");
}

#[test]
fn the_log_applied_at_the_join_instead_differs() {
    // The control: the same rows applied when B opens, not at their times.
    let join = 219;
    let mut a = session(&page(), W, H, 1);
    for i in 0..join {
        a.fold(ft(i), &rows_at(i));
        render(&mut a, W, H, ft(i), i);
    }
    let log = a.timeline().log().to_vec();
    let mut c = session(&page(), W, H, 1);
    let lines: Vec<String> = log
        .iter()
        .map(|t| {
            let mut v = t.row.to_value();
            v.as_object_mut().unwrap().remove("at");
            v.to_string()
        })
        .collect();
    c.fold(ft(join), &lines);
    let mut differing = 0;
    for i in join..=join + 30 {
        let rows = rows_at(i);
        a.fold(ft(i), &rows);
        if i > join {
            c.fold(ft(i), &rows);
        }
        let fa = bytes(render(&mut a, W, H, ft(i), i).0);
        let fc = bytes(render(&mut c, W, H, ft(i), i).0);
        differing += (diff(&fa, &fc).1 > 0) as usize;
    }
    assert!(differing > 20, "{differing}");
}
