//! Frame-parallel workers render what one instance renders, when every
//! worker knows every frame's rows.

use crate::common::*;
use crate::joiner::{H, W, page, rows_at};

const FRAMES: usize = 300;

/// One instance, every frame and its rows.
fn single() -> Vec<Vec<u8>> {
    let mut s = session(&page(), W, H, 1);
    (0..FRAMES)
        .map(|i| {
            s.fold(ft(i), &rows_at(i));
            bytes(render(&mut s, W, H, ft(i), i).0)
        })
        .collect()
}

/// `n` workers taking frames in turn. With `history`, a worker folds the
/// rows of every frame it skipped, each at that frame's time, before its
/// own frame's rows (what a host with row history hands it); without, it
/// sees its own frames' rows alone (what ffrwd:av 0.18 hands it).
fn workers(n: usize, history: bool) -> Vec<Vec<u8>> {
    let mut ws: Vec<_> = (0..n).map(|_| session(&page(), W, H, 1)).collect();
    let mut next_unseen = vec![0usize; n];
    (0..FRAMES)
        .map(|i| {
            let k = i % n;
            let s = &mut ws[k];
            if history {
                for j in next_unseen[k]..i {
                    let rows = rows_at(j);
                    if !rows.is_empty() {
                        s.fold(ft(j), &rows);
                    }
                }
            }
            next_unseen[k] = i + 1;
            s.fold(ft(i), &rows_at(i));
            bytes(render(s, W, H, ft(i), i).0)
        })
        .collect()
}

#[test]
fn workers_with_the_skipped_frames_rows_render_as_one_instance() {
    let one = single();
    for n in [2, 3, 4] {
        let many = workers(n, true);
        for (i, (a, b)) in one.iter().zip(&many).enumerate() {
            assert_eq!(diff(a, b), (0, 0), "{n} workers: frame {i} differs");
        }
    }
}

#[test]
fn workers_with_only_their_own_rows_do_not() {
    let one = single();
    let many = workers(3, false);
    let differing = one
        .iter()
        .zip(&many)
        .filter(|(a, b)| diff(a, b).1 > 0)
        .count();
    assert!(differing > 100, "{differing}");
}

#[test]
fn workers_driven_by_parameters_alone_render_as_one_instance() {
    let rows: Vec<String> = crate::joiner::stream_rows()
        .into_iter()
        .map(|(_, l)| l.to_string())
        .collect();
    let changes = format!(
        "[{}]",
        rows.iter()
            .map(|l| l.trim_start_matches('[').trim_end_matches(']'))
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut p = params(&page());
    p.changes = compose_core::Row::parse_list(&changes).unwrap();
    let mk = || compose_core::Session::new(p.clone(), (W, H), (W, H), 1);
    let mut one = mk();
    let mut ws: Vec<_> = (0..3).map(|_| mk()).collect();
    for i in 0..FRAMES {
        let a = bytes(render(&mut one, W, H, ft(i), i).0);
        let b = bytes(render(&mut ws[i % 3], W, H, ft(i), i).0);
        assert_eq!(diff(&a, &b), (0, 0), "frame {i} differs");
    }
}

/// A worker opened late, or one whose frames are far apart, resolves across
/// many iterations of a short loop at once; it renders what one instance
/// rendering every frame renders.
#[test]
fn a_worker_opened_late_renders_a_short_loop_as_one_instance() {
    let html = doc(
        "#b { position: absolute; left: 10px; top: 10px; width: 40px; height: 40px; background: #fff;
              animation: pulse 0.2s ease-in-out infinite; }
         #c { position: absolute; left: 60px; top: 10px; width: 40px; height: 40px; background: #f80;
              animation: slide 0.7s linear infinite alternate; }
         @keyframes pulse { 0%, 100% { opacity: 0.2 } 50% { opacity: 1 } }
         @keyframes slide { from { transform: translateX(0) } to { transform: translateX(30px) } }",
        r#"<div id="b"></div><div id="c"></div>"#,
    );
    let mut one = session(&html, 128, 64, 1);
    let mut late = session(&html, 128, 64, 1);
    let mut sparse = session(&html, 128, 64, 1);
    for i in 0..=183 {
        let a = bytes(render(&mut one, 128, 64, ft(i), i).0);
        if i == 183 {
            let b = bytes(render(&mut late, 128, 64, ft(i), i).0);
            assert_eq!(diff(&a, &b), (0, 0), "opened at frame {i}");
        }
        if i % 16 == 0 {
            let c = bytes(render(&mut sparse, 128, 64, ft(i), i).0);
            assert_eq!(diff(&a, &c), (0, 0), "every 16th frame, frame {i}");
        }
    }
}
