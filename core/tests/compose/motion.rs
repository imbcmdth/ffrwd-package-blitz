//! Does animated `left` move in whole pixels where animated `transform`
//! moves smoothly? Measured from the rendered pixels: the coverage-weighted
//! position of a 20 px box moving 10 px over one second, 30 frames.

use crate::common::*;

const W: u32 = 200;
const H: u32 = 40;

fn positions(prop_css: &str) -> Vec<f64> {
    let html = doc(
        &format!(
            "html, body {{ background: #000; }}
             #b {{ position: absolute; left: 50px; top: 10px; width: 20px; height: 20px; background: #fff;
                   animation: move 1s linear forwards; }}
             @keyframes move {{ {prop_css} }}"
        ),
        r#"<div id="b"></div>"#,
    );
    let mut s = session(&html, W, H, 1);
    (0..=30)
        .map(|i| {
            let f = bytes(render(&mut s, W, H, ft(i), i).0);
            // Coverage-weighted left edge along the row y = 20.
            let (mut sum, mut weight) = (0.0, 0.0);
            for x in 0..W {
                let v = px(&f, W, x, 20)[0] as f64 / 255.0;
                sum += v * (x as f64 + 0.5);
                weight += v;
            }
            sum / weight - weight / 2.0
        })
        .collect()
}

fn whole(v: f64) -> bool {
    (v - v.round()).abs() < 0.01
}

#[test]
fn transform_moves_in_fractions_of_a_pixel_and_left_does_not() {
    let by_left = positions("from { left: 50px } to { left: 60px }");
    let by_transform =
        positions("from { transform: translateX(0px) } to { transform: translateX(10px) }");
    eprintln!(
        "left:      {:?}",
        by_left
            .iter()
            .map(|v| format!("{v:.3}"))
            .collect::<Vec<_>>()
    );
    eprintln!(
        "transform: {:?}",
        by_transform
            .iter()
            .map(|v| format!("{v:.3}"))
            .collect::<Vec<_>>()
    );
    // Both end 10 px along.
    assert!((by_left[30] - 60.0).abs() < 0.01, "{}", by_left[30]);
    assert!(
        (by_transform[30] - 60.0).abs() < 0.01,
        "{}",
        by_transform[30]
    );
    // `left` lands on whole pixels every frame: steps of 1 px, some frames
    // repeating the one before.
    assert!(by_left.iter().all(|v| whole(*v)), "{by_left:?}");
    // `transform` follows the time: 10/30 px a frame, fractions included.
    let fractional = by_transform.iter().filter(|v| !whole(**v)).count();
    assert!(fractional >= 15, "{by_transform:?}");
    for (i, v) in by_transform.iter().enumerate() {
        let want = 50.0 + 10.0 * i as f64 / 30.0;
        assert!((v - want).abs() < 0.02, "frame {i}: {v} against {want}");
    }
}
