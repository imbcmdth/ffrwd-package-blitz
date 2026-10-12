//! The GPU path on the native target: the same document and pictures
//! rendered by vello_cpu and by vello_hybrid on wgpu's own backends, frame
//! for frame. With no adapter (CI) every case skips, loudly, and passes.

use std::sync::Arc;

use compose_core::params::{Log, Params};
use compose_core::{Gpu, Output, RasterImageData, Row, Session};

/// How far the GPU may stray from the CPU: within `NEAR` levels on every
/// channel for all but `FAR_SHARE` of the pixels, and never beyond `MAX`.
const NEAR: u8 = 2;
const FAR_SHARE: f64 = 0.001;
const MAX: u8 = 8;

/// The share of pixels beyond `NEAR` allowed in a frame that draws a
/// picture scaled by a factor that is not a whole fraction of a pixel. There
/// vello_cpu's bilinear sampling reads past the picture's edge from its
/// opposite edge (the last column drawn in the first column's colour), and
/// the GPU's does not, so a row and a column of the picture differ by as
/// much as the picture's contrast, and its hard edges by a few levels.
const SCALING_FAR_SHARE: f64 = 0.01;

/// When the lbar document scales the programme: the squeeze from 2.0 s and
/// the return from 8.5 s, 0.75 s each.
const LBAR_SCALING: &[(f64, f64)] = &[(2.0, 2.75), (8.5, 9.25)];

fn gpu(size: (u32, u32)) -> Option<Gpu> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    match Gpu::new(&instance, size) {
        Ok(gpu) => {
            eprintln!("GPU: {}", gpu.adapter());
            Some(gpu)
        }
        Err(e) => {
            eprintln!("SKIPPED: {e}");
            None
        }
    }
}

/// The compositor document of `examples/lbar.sql`, with its rows.
const LBAR: &str = r##"<!DOCTYPE html><html><head><style>
  html, body { margin: 0; overflow: hidden; background: #000; font-family: sans-serif; }
  #ad, #prog { position: absolute; left: 0; top: 0; width: 1280px; height: 720px; }
  #prog { transform-origin: 100% 0; transition: transform 0.75s ease-in-out; }
  #stage.lbar #prog { transform: scale(0.75); }
  #lower { position: absolute; left: 330px; top: 572px; width: 920px; height: 104px;
           background: rgba(8, 18, 52, 0.92); border-left: 10px solid #fc4a1a; color: #fff;
           transform: translateX(1000px); transition: transform 0.6s ease-out; }
  #lower.on { transform: translateX(0); }
  #lower .name { position: absolute; left: 26px; top: 10px; font-size: 44px; font-weight: bold; }
  #lower .role { position: absolute; left: 26px; top: 64px; font-size: 26px; color: #cfd8ff; }
  #bug { position: absolute; left: 1130px; top: 24px; }
  .logo { width: 110px; height: 110px; border-radius: 55px; background: #1e90ff; color: #fff;
          font-size: 30px; font-weight: bold; line-height: 110px; text-align: center;
          animation: pulse 1.2s ease-in-out infinite; }
  @keyframes pulse { 0%, 100% { transform: scale(1) } 50% { transform: scale(1.15) } }
</style></head><body><div id="stage">
  <img id="ad" src="ffrwd:1"><img id="prog" src="ffrwd:0">
  <div id="lower"><div class="name"></div><div class="role">Correspondent, on location</div></div>
  <div id="bug"></div>
</div></body></html>"##;

const LBAR_ROWS: &str = r##"[
  {"at": 2.0, "select": "#stage", "change": "+lbar"},
  [{"at": 2.75, "select": "#lower .name", "text": "Jane Example"},
   {"at": 2.75, "select": "#lower", "change": "+on"}],
  {"at": 3.5, "select": "#bug", "html": "<div class=\"logo\">LIVE</div>"},
  {"at": 7.0, "select": "#lower", "change": "-on"},
  {"at": 8.0, "select": "#bug", "html": ""},
  {"at": 8.5, "select": "#stage", "change": "-lbar"}
]"##;

fn params(html: &str, rows: &str, css_width: Option<f64>) -> Params {
    Params {
        html: html.to_string(),
        rows: Row::parse_list(rows).unwrap(),
        css_width,
        bypass: false,
        log: Log::Off,
        ..Params::default()
    }
}

/// A picture with hard edges in it (bars over gradients), so sampling
/// differences show: different per input and per frame `n`.
fn picture(w: u32, h: u32, input: u32, n: usize) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let mut f = vec![255u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            f[i] = ((x * 255 / w) as u8).wrapping_add((n * 7) as u8);
            f[i + 1] = (y * 255 / h) as u8;
            f[i + 2] = if ((x + n * 3) * 8 / w).is_multiple_of(2) {
                40
            } else {
                200
            } ^ (input as u8 * 90);
        }
    }
    f
}

fn rendered(out: Output) -> Vec<u8> {
    match out {
        Output::New(b) => b,
        Output::Same(i) => panic!("input {i} handed back, with the bypass off"),
    }
}

/// (pixels beyond `NEAR`, the largest difference).
fn compare(gpu: &[u8], cpu: &[u8]) -> (usize, u8) {
    assert_eq!(gpu.len(), cpu.len());
    let mut far = 0;
    let mut max = 0;
    for (g, c) in gpu.as_chunks::<4>().0.iter().zip(cpu.as_chunks::<4>().0) {
        let worst = g.iter().zip(c).map(|(g, c)| g.abs_diff(*c)).max().unwrap();
        max = max.max(worst);
        if worst > NEAR {
            far += 1;
        }
    }
    (far, max)
}

/// Renders `times` on both paths over the same pictures and checks every
/// frame against the tolerance, a frame inside one of `scaling` against
/// `SCALING_FAR_SHARE` alone. Input `k` repeats one picture for the whole
/// run when `held` has bit k, as a held input showing a still does.
fn both(
    name: &str,
    params: Params,
    out: (u32, u32),
    sizes: &[(u32, u32)],
    held: u64,
    times: &[f64],
    scaling: &[(f64, f64)],
) -> Option<Gpu> {
    let gpu = gpu(out)?;
    let sizes: Vec<_> = sizes.iter().map(|s| Some(*s)).collect();
    let mut on_gpu = Session::with_sizes(params.clone(), out, sizes.clone());
    on_gpu.set_gpu(Some(gpu));
    let mut on_cpu = Session::with_sizes(params, out, sizes.clone());
    let stills: Vec<RasterImageData> = sizes
        .iter()
        .enumerate()
        .map(|(k, s)| {
            let (w, h) = s.unwrap();
            RasterImageData::new(w, h, Arc::new(picture(w, h, k as u32, 0)))
        })
        .collect();
    let pixels = (out.0 * out.1) as usize;
    for (n, &t) in times.iter().enumerate() {
        let mut fetch = |input: u32, (w, h): (u32, u32)| {
            Some(if held & (1 << input) != 0 {
                stills[input as usize].clone()
            } else {
                RasterImageData::new(w, h, Arc::new(picture(w, h, input, n)))
            })
        };
        let g = rendered(on_gpu.frame_pictures(t, 0, &mut fetch));
        let c = rendered(on_cpu.frame_pictures(t, 0, &mut fetch));
        let (far, max) = compare(&g, &c);
        let scaled = scaling.iter().any(|(a, b)| *a < t && t < *b);
        eprintln!(
            "{name} at {t:.2} s{}: {far} of {pixels} pixels ({:.4}%) beyond {NEAR} levels, largest difference {max}",
            if scaled { " (scaling)" } else { "" },
            far as f64 * 100.0 / pixels as f64
        );
        if scaled {
            assert!(
                far as f64 <= SCALING_FAR_SHARE * pixels as f64,
                "{name} at {t} s: {far} of {pixels} pixels differ by more than {NEAR} levels"
            );
        } else {
            assert!(
                far as f64 <= FAR_SHARE * pixels as f64,
                "{name} at {t} s: {far} of {pixels} pixels differ by more than {NEAR} levels"
            );
            assert!(
                max <= MAX,
                "{name} at {t} s: a pixel differs by {max} levels"
            );
        }
    }
    let mut on_gpu = on_gpu;
    on_gpu.compositor().take_gpu()
}

#[test]
fn the_lbar_document_matches_the_cpu_at_1080p() {
    let times = [0.5, 2.0, 2.4, 2.9, 3.3, 3.8, 5.0, 7.3, 8.6, 8.9, 9.5];
    both(
        "lbar 1080p",
        params(LBAR, LBAR_ROWS, Some(1280.0)),
        (1920, 1080),
        &[(1920, 1080), (1920, 1080)],
        0,
        &times,
        LBAR_SCALING,
    );
}

#[test]
fn a_translucent_page_matches_the_cpu_premultiplied() {
    // No background: the frame is transparent but for a translucent box and
    // text, so the bytes are premultiplied on both paths.
    let html = r##"<style>html, body { margin: 0; }
      #a { position: absolute; left: 10px; top: 10px; width: 200px; height: 90px;
           background: rgba(255, 128, 0, 0.5); border-radius: 20px; }
      #t { position: absolute; left: 20px; top: 120px; font-size: 40px; color: rgba(0, 200, 255, 0.7); }
      </style><div id="a"></div><div id="t">Translucent</div>"##;
    both(
        "translucent page",
        params(html, "", None),
        (330, 180),
        &[],
        0,
        &[0.0],
        &[],
    );
}

#[test]
fn a_held_picture_is_uploaded_once() {
    // Input 1 repeats one picture, as a held input does while its source is
    // late; input 0 is new every frame.
    let gpu = both(
        "held picture",
        params(LBAR, LBAR_ROWS, Some(1280.0)),
        (1280, 720),
        &[(1280, 720), (1280, 720)],
        0b10,
        &[0.5, 0.6, 0.7, 0.8],
        &[],
    );
    if let Some(gpu) = gpu {
        assert_eq!(
            gpu.uploads(),
            4 + 1,
            "one upload a frame for input 0, one in all for input 1"
        );
    }
}
