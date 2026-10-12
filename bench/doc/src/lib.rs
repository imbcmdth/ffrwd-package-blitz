//! The document, the inputs and the rows of the 4K60 compositing experiment
//! (`plans/experiment-compose-4k60.md` in ffrwd-cli). Step A renders it
//! natively, step C under `ffrwd-wasm`; both write the same rows.

use serde::{Deserialize, Serialize};

/// The design width every document here is laid out at, CSS px: at
/// 3840x2160 Blitz paints it at a device scale of 3.
pub const CSS_WIDTH: u32 = 1280;

/// The output frame.
pub const OUT: (u32, u32) = (3840, 2160);

/// The frames rendered in one run, at t = n / 60.
pub const FRAMES: u32 = 600;

/// The experiment's variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Variant {
    /// Textures uploaded once (the same picture blob every frame), render only.
    Resident = 1,
    /// A new picture blob every frame, uploaded every frame: today's wire.
    Upload = 2,
    /// `Resident`, plus a readback of the 4K frame each tick: today's output path.
    Readback = 3,
    /// `Resident` with the animations idle: a document that changes nothing.
    Idle = 4,
}

impl Variant {
    pub fn from_number(n: u8) -> Option<Variant> {
        Some(match n {
            1 => Variant::Resident,
            2 => Variant::Upload,
            3 => Variant::Readback,
            4 => Variant::Idle,
            _ => return None,
        })
    }

    pub fn number(self) -> u8 {
        self as u8
    }

    pub fn name(self) -> &'static str {
        match self {
            Variant::Resident => "resident",
            Variant::Upload => "upload",
            Variant::Readback => "readback",
            Variant::Idle => "idle",
        }
    }

    pub fn animated(self) -> bool {
        self != Variant::Idle
    }

    pub fn uploads_every_frame(self) -> bool {
        self == Variant::Upload
    }

    pub fn reads_back(self) -> bool {
        self == Variant::Readback
    }
}

/// Input `i`'s picture size: input 0 at 4K, the rest at 1080p unless every
/// input is 4K.
pub fn input_size(i: u32, all_4k: bool) -> (u32, u32) {
    if i == 0 || all_4k { OUT } else { (1920, 1080) }
}

/// The lbar document generalised to `n` inputs, `n` from 1 to 64: input 0
/// over the whole frame with the L-bar cycle, inputs 1 on in a
/// picture-in-picture grid over it with `transform` animations, a lower
/// third sliding in and out, a logo pulsing. With `animate` off the same
/// document stands still at its first keyframe.
pub fn document(n: u32, animate: bool) -> String {
    let anim = |a: &str| {
        if animate {
            format!("animation: {a};")
        } else {
            String::new()
        }
    };
    let mut css = String::new();
    css.push_str(concat!(
        "html, body { margin: 0; overflow: hidden; background: #000; font-family: sans-serif; }\n",
        "#stage { position: absolute; left: 0; top: 0; width: 1280px; height: 720px;\n",
        "         background: linear-gradient(135deg, #f7b733, #fc4a1a); }\n",
        "#prog { position: absolute; left: 0; top: 0; width: 1280px; height: 720px; transform-origin: 100% 0; ",
    ));
    css.push_str(&anim("lbar 8s ease-in-out infinite"));
    css.push_str(concat!(
        " }\n",
        "@keyframes lbar { 0%, 30% { transform: scale(1) } 40%, 85% { transform: scale(0.75) } 95%, 100% { transform: scale(1) } }\n",
        ".pip { position: absolute; width: 288px; height: 162px; border: 3px solid #fff; background: #222; ",
    ));
    css.push_str(&anim("drift 2.4s ease-in-out infinite alternate"));
    css.push_str(concat!(
        " }\n",
        "@keyframes drift { from { transform: translate(0, 0) scale(1) } to { transform: translate(14px, 10px) scale(1.05) } }\n",
        "#lower { position: absolute; left: 330px; top: 572px; width: 920px; height: 104px;\n",
        "         background: rgba(8, 18, 52, 0.92); border-left: 10px solid #fc4a1a; color: #fff; ",
    ));
    css.push_str(&anim("slide 6s ease-in-out infinite"));
    css.push_str(concat!(
        " }\n",
        "@keyframes slide { 0%, 8% { transform: translateX(1000px) } 20%, 78% { transform: translateX(0) } 90%, 100% { transform: translateX(1000px) } }\n",
        "#lower .name { position: absolute; left: 26px; top: 10px; font-size: 44px; font-weight: bold; }\n",
        "#lower .role { position: absolute; left: 26px; top: 64px; font-size: 26px; color: #cfd8ff; }\n",
        "#bug { position: absolute; left: 1130px; top: 24px; }\n",
        ".logo { width: 110px; height: 110px; border-radius: 55px; background: #1e90ff; color: #fff;\n",
        "        font-size: 30px; font-weight: bold; line-height: 110px; text-align: center; ",
    ));
    css.push_str(&anim("pulse 1.2s ease-in-out infinite"));
    css.push_str(concat!(
        " }\n",
        "@keyframes pulse { 0%, 100% { transform: scale(1) } 50% { transform: scale(1.15) } }\n",
    ));
    let mut pips = String::new();
    for k in 1..n {
        let idx = k - 1;
        let (col, row) = (idx % 3, idx / 3);
        let (left, top) = (16 + col * 304, 16 + row * 178);
        let delay = if animate {
            format!(" animation-delay: -{:.2}s;", idx as f64 * 0.37)
        } else {
            String::new()
        };
        pips.push_str(&format!(
            "<img class=\"pip\" id=\"pip{k}\" src=\"ffrwd:{k}\" style=\"left: {left}px; top: {top}px;{delay}\">"
        ));
    }
    format!(
        concat!(
            "<!DOCTYPE html><html><head><style>\n{css}</style></head><body><div id=\"stage\">",
            "<img id=\"prog\" src=\"ffrwd:0\">{pips}",
            "<div id=\"lower\"><div class=\"name\">Jane Example</div><div class=\"role\">Correspondent, on location</div></div>",
            "<div id=\"bug\"><div class=\"logo\">LIVE</div></div>",
            "</div></body></html>"
        ),
        css = css,
        pips = pips
    )
}

/// The parameters the wasm module takes for the same document: the
/// `page` shape plus the experiment's own.
pub fn params_json(n: u32, variant: Variant, all_4k: bool, frames: u32) -> String {
    serde_json::json!({
        "html": document(n, variant.animated()),
        "css_width": CSS_WIDTH,
        "width": OUT.0,
        "height": OUT.1,
        "fps": 60,
        "inputs": n,
        "variant": variant.number(),
        "all_4k": all_4k,
        "frames": frames,
    })
    .to_string()
}

/// Input `i`'s picture, straight RGBA8: a gradient whose blue is the
/// input's, so a wrong input is visible.
pub fn gradient(w: u32, h: u32, i: u32) -> Vec<u8> {
    let mut v = vec![0u8; (w as usize) * (h as usize) * 4];
    let b = ((i * 73 + 40) % 256) as u8;
    let (wu, hu) = (w as usize, h as usize);
    for y in 0..hu {
        let g = ((y * 255) / hu.max(1)) as u8;
        let row = &mut v[y * wu * 4..(y + 1) * wu * 4];
        for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            px[0] = ((x * 255) / wu.max(1)) as u8;
            px[1] = g;
            px[2] = b;
            px[3] = 255;
        }
    }
    v
}

/// 5x7 digit glyphs, one bit a pixel, rows top to bottom.
const DIGITS: [[u8; 7]; 10] = [
    [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
    [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
    [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
    [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
    [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
    [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
    [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
    [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
    [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
    [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
];

/// Burns `frame` into `rgba` (a `w` x `h` picture) as white digits on a
/// dark box near the top left, each glyph cell `w / 128` px, so the frame
/// shown can be read off a dump.
pub fn stamp(rgba: &mut [u8], w: u32, h: u32, frame: u32) {
    let cell = (w / 128).max(2) as usize;
    let (wu, hu) = (w as usize, h as usize);
    let digits: Vec<usize> = format!("{frame:04}")
        .bytes()
        .map(|b| (b - b'0') as usize)
        .collect();
    let (x0, y0) = (wu / 32, hu / 32);
    let box_w = digits.len() * 6 * cell + 2 * cell;
    let box_h = 9 * cell;
    for y in y0..(y0 + box_h).min(hu) {
        for x in x0..(x0 + box_w).min(wu) {
            let p = (y * wu + x) * 4;
            rgba[p..p + 3].copy_from_slice(&[16, 16, 24]);
        }
    }
    for (k, d) in digits.iter().enumerate() {
        let gx = x0 + cell + k * 6 * cell;
        let gy = y0 + cell;
        for (row, bits) in DIGITS[*d].iter().enumerate() {
            for col in 0..5 {
                if bits & (0x10 >> col) == 0 {
                    continue;
                }
                for dy in 0..cell {
                    let y = gy + row * cell + dy;
                    if y >= hu {
                        continue;
                    }
                    for dx in 0..cell {
                        let x = gx + col * cell + dx;
                        if x >= wu {
                            continue;
                        }
                        let p = (y * wu + x) * 4;
                        rgba[p..p + 3].copy_from_slice(&[255, 255, 255]);
                    }
                }
            }
        }
    }
}

/// How GPU time was measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuTiming {
    /// wgpu timestamp queries around the frame's encoder.
    Timestamp,
    /// Wall clock around submit and the wait for the queue.
    Wall,
}

/// The first line of a run: what ran, and on what.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Meta {
    pub kind: String,
    /// "A" native, "C" wasm.
    pub step: String,
    pub variant: u8,
    pub n: u32,
    pub all_4k: bool,
    pub frames: u32,
    pub width: u32,
    pub height: u32,
    pub adapter: String,
    pub backend: String,
    pub driver: String,
    pub driver_info: String,
    pub device_type: String,
    pub os: String,
    pub arch: String,
    pub gpu_timing: GpuTiming,
    pub notes: Vec<String>,
}

impl Meta {
    pub fn new(step: &str, variant: Variant, n: u32, all_4k: bool, frames: u32) -> Meta {
        Meta {
            kind: "meta".into(),
            step: step.into(),
            variant: variant.number(),
            n,
            all_4k,
            frames,
            width: OUT.0,
            height: OUT.1,
            adapter: String::new(),
            backend: String::new(),
            driver: String::new(),
            driver_info: String::new(),
            device_type: String::new(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            gpu_timing: GpuTiming::Wall,
            notes: Vec::new(),
        }
    }
}

/// One rendered frame. Times in microseconds; a phase that did not run is 0.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Row {
    pub frame: u32,
    pub t: f64,
    /// Making the frame's input pictures (variant 2 copies them anew), not
    /// part of the compositor's cost.
    pub synth_us: f64,
    pub resolve_us: f64,
    pub plan_us: f64,
    /// Painting the document into the scene: path generation and, with it,
    /// the uploads of pictures the renderer has not got.
    pub paint_cmds_us: f64,
    /// The renderer's preparation and command encoding.
    pub encode_us: f64,
    pub submit_us: f64,
    /// Waiting for the queue after submit.
    pub wait_us: f64,
    /// GPU time, by `Meta::gpu_timing`.
    pub gpu_us: f64,
    pub readback_us: f64,
    /// Picture bytes uploaded to the GPU this frame.
    pub upload_bytes: u64,
    /// Bytes that crossed the wit this frame (step C only).
    pub wit_bytes: Option<u64>,
    /// The whole tick, wall clock.
    pub frame_us: f64,
    /// FNV-1a of the frame's pixels, on the frames that are read back and
    /// checked (frame 123): the same document on two renderers should agree.
    pub pixel_hash: Option<u64>,
}

/// The frame whose pixels are hashed when they are read back.
pub const HASHED_FRAME: u32 = 123;

/// FNV-1a over `bytes`.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

impl Row {
    /// The compositor's CPU time: what the guest spends a frame.
    pub fn cpu_us(&self) -> f64 {
        self.resolve_us + self.plan_us + self.paint_cmds_us + self.encode_us
    }
}

/// The last line of a run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct End {
    pub kind: String,
    pub frames: u32,
    pub wall_us: f64,
    pub peak_rss_bytes: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_document_names_every_input_once() {
        for n in [1, 2, 4, 8] {
            let d = document(n, true);
            for k in 0..n {
                assert_eq!(
                    d.matches(&format!("src=\"ffrwd:{k}\"")).count(),
                    1,
                    "n={n} k={k}"
                );
            }
            assert_eq!(d.matches("src=\"ffrwd:").count(), n as usize);
        }
        assert!(!document(4, false).contains("animation:"));
        assert!(document(4, true).contains("animation: lbar"));
    }

    #[test]
    fn the_stamp_is_inside_the_picture() {
        let (w, h) = (1920, 1080);
        let mut v = gradient(w, h, 1);
        stamp(&mut v, w, h, 599);
        assert_eq!(v.len(), (w * h * 4) as usize);
        let mut g = gradient(w, h, 1);
        stamp(&mut g, w, h, 598);
        assert_ne!(v, g);
    }
}
