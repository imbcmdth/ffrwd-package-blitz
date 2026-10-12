//! The module under the ffrwd sidecar, on the GPU and on the CPU: the
//! README's lbar example (`examples/lbar.sql`'s document and rows over a
//! programme and an ad) run once with `renderer => 'gpu'` and once with
//! `renderer => 'cpu'`, the frames compared one for one.
//!
//! The sidecar is `FFRWD_WASM` when that is set, else the release build in
//! an ffrwd-cli checkout beside this repository; the module is the release
//! build in `target/wasm32-wasip2` (`cargo build --target wasm32-wasip2
//! --release` first). Without either the test skips, loudly, and passes.
//!
//! The pictures are made here and handed to the sidecar on its stdin as one
//! NUT of two rgba streams: the programme (colour bars, a gradient, a
//! moving box and a counter, as lavfi's testsrc2 has) and the ad (the
//! orange gradient and the moving ball of the example's page).
//!
//! `lbar_frames_a_second`, ignored by default, measures the example's
//! frames a second at 1080p and 4K on each path, for the README:
//! `cargo test -p compose --test sidecar -- --ignored --nocapture`.

use std::io::{BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use ffrwd_nut::{Demuxer, Media, Muxer, Stream, TimeBase};

/// How far the GPU may stray from the CPU: within `NEAR` levels on every
/// channel for all but `FAR_SHARE` of the pixels, and never beyond `MAX`,
/// as wgpu-wasi-webgpu's vello test asks.
const NEAR: u8 = 2;
const FAR_SHARE: f64 = 0.001;
const MAX: u8 = 8;

/// The share of pixels beyond `NEAR` allowed in a frame where the programme
/// is being scaled. blitz-paint draws pictures with `Extend::Repeat`, and
/// vello_cpu then shades the scaled programme's last row and column partly
/// from its first ones (the right-hand column white, from the first bar),
/// where vello_hybrid does not: that row and column differ by up to the
/// programme's contrast across it, and its hard edges by a few levels.
const SCALING_FAR_SHARE: f64 = 0.01;

/// When the document scales the programme: the squeeze from 2.0 s and the
/// return from 8.5 s, 0.75 s each.
const SCALING: &[(f64, f64)] = &[(2.0, 2.75), (8.5, 9.25)];

const W: u32 = 1920;
const H: u32 = 1080;
const FPS: u64 = 30;
/// Long enough for every row of the example: the programme is back by 9.25 s.
const FRAMES: u64 = 285;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The sidecar binary, or None to skip.
fn sidecar() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FFRWD_WASM") {
        let path = PathBuf::from(path);
        assert!(
            path.is_file(),
            "FFRWD_WASM names {}, which is not a file",
            path.display()
        );
        return Some(path);
    }
    let exe = if cfg!(windows) {
        "ffrwd-wasm.exe"
    } else {
        "ffrwd-wasm"
    };
    let sibling = root().join("../ffrwd-cli/sidecar/target/release").join(exe);
    if sibling.is_file() {
        return Some(sibling);
    }
    eprintln!(
        "SKIPPED: no ffrwd-wasm. Set FFRWD_WASM to the sidecar binary, or build \
         ../ffrwd-cli/sidecar with `cargo build --release`."
    );
    None
}

/// The module, or None to skip.
fn module() -> Option<PathBuf> {
    let path = root().join("target/wasm32-wasip2/release/compose.wasm");
    if path.is_file() {
        return Some(path);
    }
    eprintln!(
        "SKIPPED: no {}. Build it with `cargo build --target wasm32-wasip2 --release`.",
        path.display()
    );
    None
}

/// `examples/lbar.sql`'s document and rows.
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

/// The programme's and the ad's pictures, `PICTURES` of each, made once and
/// handed over in turn: the programme at the output's size, the ad at
/// 1920x1080 as the example's page is.
struct Pictures {
    size: (u32, u32),
    programme: Vec<Vec<u8>>,
    ad: Vec<Vec<u8>>,
}

const PICTURES: u64 = 60;
const AD: (u32, u32) = (1920, 1080);

fn put(f: &mut [u8], w: u32, x: u32, y: u32, rgb: [u8; 3]) {
    let i = ((y * w + x) * 4) as usize;
    f[i..i + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
}

impl Pictures {
    fn new(size: (u32, u32)) -> Pictures {
        let (pb, ab) = (programme_base(size), ad_base());
        Pictures {
            size,
            programme: (0..PICTURES).map(|n| programme(&pb, size, n)).collect(),
            ad: (0..PICTURES).map(|n| ad(&ab, n)).collect(),
        }
    }
}

/// The programme's still part: colour bars over the top two thirds, a grey
/// ramp under them.
fn programme_base((w, h): (u32, u32)) -> Vec<u8> {
    const BARS: [[u8; 3]; 8] = [
        [235, 235, 235],
        [235, 235, 16],
        [16, 235, 235],
        [16, 235, 16],
        [235, 16, 235],
        [235, 16, 16],
        [16, 16, 235],
        [16, 16, 16],
    ];
    let mut f = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let rgb = if y < h * 2 / 3 {
                BARS[(x * 8 / w) as usize]
            } else {
                let v = (x * 255 / (w - 1)) as u8;
                [v, v, v]
            };
            put(&mut f, w, x, y, rgb);
        }
    }
    f
}

/// Frame `n` of the programme: the bars, a box sweeping across them and a
/// row of blocks counting the frame in binary, laid out on 1080 lines and
/// scaled to `h`.
fn programme(base: &[u8], (w, h): (u32, u32), n: u64) -> Vec<u8> {
    let mut f = base.to_vec();
    let px = |v: u32| v * h / 1080;
    let side = px(200);
    let bx = (n * u64::from(px(23)) % u64::from(w - side)) as u32;
    for y in px(300)..px(300) + side {
        for x in bx..bx + side {
            let edge = !(bx + px(6)..bx + side - px(6)).contains(&x)
                || !(px(306)..px(300) + side - px(6)).contains(&y);
            put(
                &mut f,
                w,
                x,
                y,
                if edge { [0, 0, 0] } else { [255, 255, 255] },
            );
        }
    }
    for bit in 0..10u32 {
        let on = n >> bit & 1 == 1;
        for y in px(760)..px(800) {
            for x in px(100 + bit * 50)..px(140 + bit * 50) {
                put(
                    &mut f,
                    w,
                    x,
                    y,
                    if on { [255, 64, 0] } else { [0, 64, 255] },
                );
            }
        }
    }
    f
}

/// The ad's still part: the example page's orange gradient, corner to
/// corner.
fn ad_base() -> Vec<u8> {
    let (w, h) = AD;
    let (a, b) = ([0xf7u32, 0xb7, 0x33], [0xfcu32, 0x4a, 0x1a]);
    let mut f = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let t = (x + y) * 255 / (w + h - 2);
            let mix = |k: usize| ((a[k] * (255 - t) + b[k] * t) / 255) as u8;
            put(&mut f, w, x, y, [mix(0), mix(1), mix(2)]);
        }
    }
    f
}

/// Frame `n` of the ad: a translucent white ball bobbing over the gradient.
fn ad(base: &[u8], n: u64) -> Vec<u8> {
    let w = AD.0;
    let mut f = base.to_vec();
    let phase = (n % PICTURES) as f64 / PICTURES as f64 * std::f64::consts::TAU;
    let (cx, cy, r) = (225.0, 570.0 + 60.0 * phase.sin(), 120.0);
    for y in (cy - r) as u32..=(cy + r) as u32 {
        for x in (cx - r) as u32..=(cx + r) as u32 {
            let d = ((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt();
            if d <= r {
                let i = ((y * w + x) * 4) as usize;
                for c in &mut f[i..i + 3] {
                    *c = ((u32::from(*c) * 165 + 255 * 90) / 255) as u8;
                }
            }
        }
    }
    f
}

/// One run of the example through the sidecar, its pictures fed on stdin
/// from a thread of their own and its frames read from stdout.
struct Run {
    child: Child,
    frames: Demuxer<BufReader<std::process::ChildStdout>>,
    feeder: Option<JoinHandle<()>>,
    stderr: Option<JoinHandle<String>>,
}

impl Run {
    /// `frames` frames of the example with `renderer`, on `jobs` workers
    /// (the sidecar's default when none).
    fn start(
        sidecar: &Path,
        module: &Path,
        renderer: &str,
        pictures: &Arc<Pictures>,
        frames: u64,
        jobs: Option<u32>,
    ) -> Run {
        let (w, h) = pictures.size;
        let params = serde_json::json!({
            "html": LBAR,
            "rows": LBAR_ROWS,
            "css_width": 1280,
            "lead": 0,
            "renderer": renderer,
            "log": std::env::var("LBAR_LOG").unwrap_or_else(|_| "summary".into()),
        });
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let params_file = dir.join(format!("lbar-{renderer}-{h}.json"));
        std::fs::write(&params_file, params.to_string()).unwrap();
        let module = module.to_str().unwrap();
        let mut command = Command::new(sidecar);
        command
            .args(["-gpu", module, "-f", "nut", "-i", "pipe:0"])
            .args(["-m", &format!("blitz={module}")])
            .args(["-filter_complex", "[v=0:v][inputs=0:v:1]blitz[video=out]"])
            .arg("-params-from")
            .arg(format!("blitz={}", params_file.display()));
        if let Some(jobs) = jobs {
            command.args(["-jobs", &jobs.to_string()]);
        }
        let mut child = command
            .args(["-map", "[out]", "-f", "nut", "pipe:1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn ffrwd-wasm");
        let stdin = child.stdin.take().unwrap();
        let pictures = pictures.clone();
        let feeder = std::thread::spawn(move || {
            let tb = TimeBase { num: 1, den: FPS };
            let mut streams = [
                Stream::video("rgba", w, h, tb).unwrap(),
                Stream::video("rgba", AD.0, AD.1, tb).unwrap(),
            ];
            for s in &mut streams {
                s.frame_rate = Some((FPS, 1));
            }
            let Ok(mut mux) = Muxer::with_streams(BufWriter::new(stdin), &streams) else {
                return;
            };
            for n in 0..frames {
                let (pts, k) = (n as i64, (n % PICTURES) as usize);
                // A sidecar that has stopped reading ends the feed; the
                // test reports why from its stderr.
                if mux.write_frame_to(0, pts, &pictures.programme[k]).is_err()
                    || mux.write_frame_to(1, pts, &pictures.ad[k]).is_err()
                {
                    return;
                }
            }
            let _ = mux.finish();
        });
        let mut err = child.stderr.take().unwrap();
        let stderr = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = err.read_to_string(&mut s);
            s
        });
        let stdout = child.stdout.take().unwrap();
        let frames = Demuxer::open(BufReader::with_capacity(1 << 22, stdout))
            .unwrap_or_else(|e| panic!("the {renderer} run wrote no NUT: {e}"));
        let media = frames.stream().media;
        assert!(
            matches!(media, Media::Video { width, height, .. } if (width, height) == (w, h)),
            "the {renderer} run's picture is {media:?}"
        );
        Run {
            child,
            frames,
            feeder: Some(feeder),
            stderr: Some(stderr),
        }
    }

    fn next(&mut self, data: &mut Vec<u8>) -> Option<i64> {
        self.frames.read_frame(data).expect("a NUT frame")
    }

    /// Waits for the sidecar and returns its stderr; panics if it failed.
    fn finish(mut self, renderer: &str) -> String {
        if let Some(f) = self.feeder.take() {
            f.join().unwrap();
        }
        let status = self.child.wait().unwrap();
        let stderr = self.stderr.take().unwrap().join().unwrap();
        assert!(
            status.success(),
            "the {renderer} run failed ({status}):\n{stderr}"
        );
        stderr
    }
}

#[test]
fn the_lbar_example_on_the_gpu_matches_the_cpu() {
    let (Some(sidecar), Some(module)) = (sidecar(), module()) else {
        return;
    };
    let pictures = Arc::new(Pictures::new((W, H)));
    let mut gpu = Run::start(&sidecar, &module, "gpu", &pictures, FRAMES, None);
    let mut cpu = Run::start(&sidecar, &module, "cpu", &pictures, FRAMES, None);
    let (mut g, mut c) = (Vec::new(), Vec::new());
    let pixels = (W * H) as usize;
    let (mut frames, mut far_all, mut max_all) = (0u64, 0usize, 0u8);
    let mut worst_share: (f64, i64) = (0.0, 0);
    let mut max_settled = 0u8;
    let mut over: Vec<(i64, usize, u8)> = Vec::new();
    let mut scaling: Vec<(i64, usize, u8)> = Vec::new();
    loop {
        let (pg, pc) = (gpu.next(&mut g), cpu.next(&mut c));
        let (pg, pc) = match (pg, pc) {
            (Some(a), Some(b)) => (a, b),
            (None, None) => break,
            (a, b) => panic!("one run ended first: gpu at {a:?}, cpu at {b:?}"),
        };
        assert_eq!(pg, pc, "the runs' frames are at different times");
        assert_eq!(g.len(), pixels * 4, "a gpu frame of {} bytes", g.len());
        assert_eq!(c.len(), pixels * 4, "a cpu frame of {} bytes", c.len());
        let mut far = 0usize;
        let mut max = 0u8;
        for (a, b) in g.as_chunks::<4>().0.iter().zip(c.as_chunks::<4>().0) {
            let worst = a.iter().zip(b).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
            max = max.max(worst);
            if worst > NEAR {
                far += 1;
            }
        }
        let share = far as f64 / pixels as f64;
        let t = pg as f64 / FPS as f64;
        if SCALING.iter().any(|(a, b)| *a < t && t < *b) {
            scaling.push((pg, far, max));
            if share > SCALING_FAR_SHARE {
                over.push((pg, far, max));
            }
        } else {
            if share > worst_share.0 {
                worst_share = (share, pg);
            }
            max_settled = max_settled.max(max);
            if share > FAR_SHARE || max > MAX {
                over.push((pg, far, max));
            }
        }
        frames += 1;
        far_all += far;
        max_all = max_all.max(max);
    }
    let gpu_log = gpu.finish("gpu");
    let cpu_log = cpu.finish("cpu");
    assert!(
        gpu_log.contains("on the GPU"),
        "the gpu run did not render on the GPU:\n{gpu_log}"
    );
    assert!(
        cpu_log.contains("on the CPU"),
        "the cpu run did not render on the CPU:\n{cpu_log}"
    );
    assert_eq!(frames, FRAMES, "frames out");
    let all = pixels as u64 * frames;
    eprintln!(
        "{frames} frames of {W}x{H}: {far_all} of {all} pixels ({:.4}%) differ by more than \
         {NEAR} levels, and the largest difference is {max_all}",
        far_all as f64 * 100.0 / all as f64,
    );
    eprintln!(
        "{} frames with nothing scaling: the worst is pts {} with {:.4}% beyond {NEAR}; the \
         largest difference is {max_settled}",
        frames as usize - scaling.len(),
        worst_share.1,
        worst_share.0 * 100.0
    );
    for (pts, far, max) in &scaling {
        eprintln!(
            "  scaling, pts {pts}: {far} pixels ({:.4}%) beyond {NEAR}, largest difference {max}",
            *far as f64 * 100.0 / pixels as f64
        );
    }
    assert!(
        over.is_empty(),
        "frames outside the tolerance (pts, pixels beyond {NEAR}, largest difference): {over:?}"
    );
    assert!(
        far_all as f64 <= FAR_SHARE * all as f64,
        "{far_all} pixels of the run differ by more than {NEAR} levels"
    );
}

/// Frames a second of the example through the sidecar, at 1080p and 4K, on
/// the GPU and on the CPU, at `-jobs 1` and at the sidecar's default: the
/// frames read back over the wall time from the first frame out to the
/// last, and the module's own mean time a rendered frame from its summary
/// line. `LBAR_SECONDS` sets the length of each run (12 s, as the
/// example's), `LBAR_JOBS` the worker counts (`1,default`), and
/// `LBAR_LOG=frame` has the module log each frame's timings.
#[test]
#[ignore]
fn lbar_frames_a_second() {
    let (Some(sidecar), Some(module)) = (sidecar(), module()) else {
        return;
    };
    let seconds: u64 = std::env::var("LBAR_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(12);
    let frames = seconds * FPS;
    for size in [(1920, 1080), (3840, 2160)] {
        let pictures = Arc::new(Pictures::new(size));
        let jobs_list: Vec<Option<u32>> = match std::env::var("LBAR_JOBS") {
            Ok(list) => list.split(',').map(|j| j.parse().ok()).collect(),
            Err(_) => vec![Some(1), None],
        };
        for &jobs in &jobs_list {
            for renderer in ["gpu", "cpu"] {
                let mut run = Run::start(&sidecar, &module, renderer, &pictures, frames, jobs);
                let mut data = Vec::new();
                let mut first = None;
                let mut n = 0u64;
                while run.next(&mut data).is_some() {
                    first.get_or_insert_with(Instant::now);
                    n += 1;
                }
                let wall = first.map_or(0.0, |t| t.elapsed().as_secs_f64());
                let log = run.finish(renderer);
                let rendered: Vec<f64> = log
                    .lines()
                    .filter_map(|l| {
                        let (_, rest) = l.split_once("ms a frame, ")?;
                        rest.split_once(" ms a rendered frame")?.0.parse().ok()
                    })
                    .collect();
                let mean = rendered.iter().sum::<f64>() / rendered.len().max(1) as f64;
                eprintln!(
                    "{}x{} {renderer} jobs={}: {n} frames, {:.1} fps (first frame out to last); \
                     {:.2} ms a rendered frame over {} instance(s)",
                    size.0,
                    size.1,
                    jobs.map_or("default".to_owned(), |j| j.to_string()),
                    (n - 1) as f64 / wall,
                    mean,
                    rendered.len(),
                );
            }
        }
    }
}
