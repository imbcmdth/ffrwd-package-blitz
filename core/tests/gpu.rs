//! The GPU path on the native target: the same document and pictures
//! rendered by vello_cpu and by vello_hybrid on wgpu's own backends, frame
//! for frame. With no adapter (CI) every case skips, loudly, and passes.

use std::sync::Arc;

use compose_core::gpu::Matrix;
use compose_core::params::{Log, Params};
use compose_core::{Colour, Gpu, Output, Picture, RasterImageData, Row, Session};

/// How far the GPU may stray from the CPU: within `NEAR` levels on every
/// channel for all but `FAR_SHARE` of the pixels, and never beyond `MAX`.
const NEAR: u8 = 2;
const FAR_SHARE: f64 = 0.001;
const MAX: u8 = 8;

/// The share of pixels beyond `NEAR` allowed in a frame that draws a
/// picture while it is being scaled. blitz-paint draws pictures with
/// `Extend::Repeat`, and on vello_cpu the scaled picture's last row and
/// column take colour from its first ones, where on vello_hybrid they do
/// not: that row and column differ by up to the picture's contrast across
/// it, and its hard edges by a few levels.
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

/// The frame as bytes: the CPU's as it is, the GPU's read back from its
/// texture.
fn rendered(session: &mut Session, out: Output) -> Vec<u8> {
    match out {
        Output::New(b) => b,
        Output::Rendered(frame) => {
            let mut bytes = Vec::new();
            session
                .gpu_mut()
                .expect("a texture comes from the GPU renderer")
                .read_back(&frame, &mut bytes);
            bytes
        }
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

/// How the GPU path is handed its pictures; the CPU path always gets the
/// bytes they stand for.
#[derive(Clone, Copy, PartialEq)]
enum Form {
    /// As bytes, uploaded to the atlas.
    Bytes,
    /// As rgba textures of the renderer's device.
    Rgba,
    /// As the yuv420p planes of the picture, BT.601 limited range, which the
    /// CPU path gets converted back to rgba here.
    Yuv420p,
}

/// A texture of `gpu`'s device holding `bytes`, `w` x `h` in `format`,
/// readable as the host's frames are.
fn texture(
    gpu: &Gpu,
    (w, h): (u32, u32),
    format: wgpu::TextureFormat,
    bytes: &[u8],
) -> wgpu::Texture {
    let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("picture"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let texel = format.block_copy_size(None).unwrap();
    gpu.queue().write_texture(
        texture.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * texel),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    texture
}

const BT601: Colour = Colour {
    matrix: Matrix::Bt601,
    full: false,
};

/// An rgba picture as yuv420p planes, BT.601 limited range, each chroma
/// sample the mean of the two by two pixels it covers.
fn to_yuv420p(rgba: &[u8], (w, h): (u32, u32)) -> [Vec<u8>; 3] {
    let (w, h) = (w as usize, h as usize);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let px = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2]].map(|c| f64::from(c) / 255.0)
    };
    let (kr, kb) = (0.299, 0.114);
    let kg = 1.0 - kr - kb;
    let luma = |[r, g, b]: [f64; 3]| kr * r + kg * g + kb * b;
    let mut y = vec![0u8; w * h];
    for row in 0..h {
        for x in 0..w {
            y[row * w + x] = (16.0 + 219.0 * luma(px(x, row))).round() as u8;
        }
    }
    let (mut u, mut v) = (vec![0u8; cw * ch], vec![0u8; cw * ch]);
    for cy in 0..ch {
        for cx in 0..cw {
            let (mut cb, mut cr, mut n) = (0.0, 0.0, 0.0);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (x, row) = (cx * 2 + dx, cy * 2 + dy);
                if x < w && row < h {
                    let p = px(x, row);
                    let l = luma(p);
                    cb += (p[2] - l) / (2.0 * (1.0 - kb));
                    cr += (p[0] - l) / (2.0 * (1.0 - kr));
                    n += 1.0;
                }
            }
            u[cy * cw + cx] = (128.0 + 224.0 * cb / n).round() as u8;
            v[cy * cw + cx] = (128.0 + 224.0 * cr / n).round() as u8;
        }
    }
    [y, u, v]
}

/// yuv420p planes back to straight rgba with alpha 255, by `colour`'s rows
/// in double precision: what the conversion pass is to make.
fn from_yuv420p(planes: &[Vec<u8>; 3], (w, h): (u32, u32), colour: Colour) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let cw = w.div_ceil(2);
    let rows = colour.rows();
    let mut out = vec![255u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let c = (y / 2) * cw + x / 2;
            let yuv =
                [planes[0][y * w + x], planes[1][c], planes[2][c]].map(|s| f64::from(s) / 255.0);
            for (k, row) in rows.iter().enumerate() {
                let v = f64::from(row[0]) * yuv[0]
                    + f64::from(row[1]) * yuv[1]
                    + f64::from(row[2]) * yuv[2]
                    + f64::from(row[3]);
                out[(y * w + x) * 4 + k] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
    out
}

/// The picture `form` hands the GPU path for `rgba`, and the bytes it
/// stands for, which the CPU path is handed.
fn handed(gpu: &Gpu, form: Form, (w, h): (u32, u32), rgba: Vec<u8>) -> (Picture, RasterImageData) {
    let stand_in = || RasterImageData::new(w, h, Arc::new(Vec::new()));
    match form {
        Form::Bytes => {
            let image = RasterImageData::new(w, h, Arc::new(rgba));
            (Picture::Bytes(image.clone()), image)
        }
        Form::Rgba => {
            let texture = texture(gpu, (w, h), wgpu::TextureFormat::Rgba8Unorm, &rgba);
            (
                Picture::Rgba {
                    image: stand_in(),
                    texture,
                },
                RasterImageData::new(w, h, Arc::new(rgba)),
            )
        }
        Form::Yuv420p => {
            let planes = to_yuv420p(&rgba, (w, h));
            let chroma = (w.div_ceil(2), h.div_ceil(2));
            let r8 = wgpu::TextureFormat::R8Unorm;
            let textures = [
                texture(gpu, (w, h), r8, &planes[0]),
                texture(gpu, chroma, r8, &planes[1]),
                texture(gpu, chroma, r8, &planes[2]),
            ];
            (
                Picture::Yuv420p {
                    image: stand_in(),
                    planes: textures,
                    colour: BT601,
                },
                RasterImageData::new(w, h, Arc::new(from_yuv420p(&planes, (w, h), BT601))),
            )
        }
    }
}

/// Renders `times` on both paths over the same pictures and checks every
/// frame against the tolerance, a frame inside one of `scaling` against
/// `SCALING_FAR_SHARE` alone. Input `k` repeats one picture for the whole
/// run when `held` has bit k, as a held input showing a still does. The GPU
/// path is handed its pictures as `form` says.
#[allow(clippy::too_many_arguments)]
fn both(
    name: &str,
    params: Params,
    out: (u32, u32),
    sizes: &[(u32, u32)],
    held: u64,
    times: &[f64],
    scaling: &[(f64, f64)],
    form: Form,
) -> Option<Gpu> {
    let gpu = gpu(out)?;
    let stills: Vec<(Picture, RasterImageData)> = sizes
        .iter()
        .enumerate()
        .map(|(k, &(w, h))| handed(&gpu, form, (w, h), picture(w, h, k as u32, 0)))
        .collect();
    let sizes: Vec<_> = sizes.iter().map(|s| Some(*s)).collect();
    let mut on_gpu = Session::with_sizes(params.clone(), out, sizes.clone());
    on_gpu.set_gpu(Some(gpu));
    let mut on_cpu = Session::with_sizes(params, out, sizes.clone());
    let pixels = (out.0 * out.1) as usize;
    for (n, &t) in times.iter().enumerate() {
        let pictures: Vec<(Picture, RasterImageData)> = sizes
            .iter()
            .enumerate()
            .map(|(k, s)| {
                if held & (1 << k) != 0 {
                    return stills[k].clone();
                }
                let (w, h) = s.unwrap();
                let gpu = on_gpu.gpu().unwrap();
                handed(gpu, form, (w, h), picture(w, h, k as u32, n))
            })
            .collect();
        let out = on_gpu.frame_with(t, 0, &mut |input, _| {
            Some(pictures[input as usize].0.clone())
        });
        let g = rendered(&mut on_gpu, out);
        let out = on_cpu.frame_pictures(t, 0, &mut |input, _| {
            Some(pictures[input as usize].1.clone())
        });
        let c = rendered(&mut on_cpu, out);
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
        Form::Bytes,
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
        Form::Bytes,
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
        Form::Bytes,
    );
    if let Some(gpu) = gpu {
        assert_eq!(
            gpu.uploads(),
            4 + 1,
            "one upload a frame for input 0, one in all for input 1"
        );
    }
}

#[test]
fn a_picture_with_transparency_matches_the_cpu() {
    // A picture whose alpha is not 255 everywhere is premultiplied on its
    // way to the atlas, as the CPU path premultiplies it.
    let html = r##"<style>html, body { margin: 0; background: #123456; }
      img { position: absolute; left: 0; top: 0; width: 100vw; height: 100vh; }
      </style><img src="ffrwd:0">"##;
    let (w, h) = (320, 180);
    let gpu = match gpu((w, h)) {
        Some(gpu) => gpu,
        None => return,
    };
    let mut p = picture(w, h, 0, 0);
    for (i, px) in p.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        px[3] = (i % 256) as u8;
    }
    let image = RasterImageData::new(w, h, Arc::new(p));
    let sizes = vec![Some((w, h))];
    let mut on_gpu = Session::with_sizes(params(html, "", None), (w, h), sizes.clone());
    on_gpu.set_gpu(Some(gpu));
    let mut on_cpu = Session::with_sizes(params(html, "", None), (w, h), sizes);
    let mut fetch = |_: u32, _: (u32, u32)| Some(image.clone());
    let out = on_gpu.frame_pictures(0.0, 0, &mut fetch);
    let g = rendered(&mut on_gpu, out);
    let out = on_cpu.frame_pictures(0.0, 0, &mut fetch);
    let c = rendered(&mut on_cpu, out);
    let (far, max) = compare(&g, &c);
    eprintln!("transparent picture: {far} pixels beyond {NEAR} levels, largest difference {max}");
    assert!(far as f64 <= FAR_SHARE * f64::from(w * h) && max <= MAX);
}

#[test]
fn the_lbar_document_over_rgba_textures_matches_the_cpu() {
    let times = [0.5, 2.0, 2.4, 2.9, 3.3, 3.8, 5.0, 7.3, 8.6, 8.9, 9.5];
    let gpu = both(
        "lbar 1080p, rgba textures",
        params(LBAR, LBAR_ROWS, Some(1280.0)),
        (1920, 1080),
        &[(1920, 1080), (1920, 1080)],
        0,
        &times,
        LBAR_SCALING,
        Form::Rgba,
    );
    if let Some(gpu) = gpu {
        assert_eq!(gpu.uploads(), 0, "a texture is drawn from where it is");
        assert_eq!(gpu.conversions(), 0);
    }
}

#[test]
fn the_lbar_document_over_yuv420p_planes_matches_the_cpu() {
    let times = [0.5, 2.0, 2.4, 2.9, 3.3, 3.8, 5.0, 7.3, 8.6, 8.9, 9.5];
    both(
        "lbar 1080p, yuv420p planes",
        params(LBAR, LBAR_ROWS, Some(1280.0)),
        (1920, 1080),
        &[(1920, 1080), (1920, 1080)],
        0,
        &times,
        LBAR_SCALING,
        Form::Yuv420p,
    );
}

#[test]
fn a_held_yuv420p_picture_is_converted_once() {
    let gpu = both(
        "held yuv420p picture",
        params(LBAR, LBAR_ROWS, Some(1280.0)),
        (1280, 720),
        &[(1280, 720), (1280, 720)],
        0b10,
        &[0.5, 0.6, 0.7, 0.8],
        &[],
        Form::Yuv420p,
    );
    if let Some(gpu) = gpu {
        assert_eq!(gpu.uploads(), 0);
        assert_eq!(
            gpu.conversions(),
            4 + 1,
            "one conversion a frame for input 0, one in all for input 1"
        );
    }
}

#[test]
fn a_yuv420p_picture_alone_is_its_conversion() {
    // What a frame that is nothing but a yuv input's picture hands on: the
    // picture converted, in a texture of its own.
    let size = (322, 182);
    let Some(mut gpu) = gpu(size) else {
        return;
    };
    let rgba = picture(size.0, size.1, 0, 3);
    let (picture, expected) = handed(&gpu, Form::Yuv420p, size, rgba);
    let first = gpu.frame_of(0, &picture).expect("a texture picture");
    let again = gpu.frame_of(0, &picture).expect("a texture picture");
    assert_eq!(gpu.conversions(), 1, "the same picture is converted once");
    for frame in [first, again] {
        let mut bytes = Vec::new();
        gpu.read_back(&frame, &mut bytes);
        let (far, max) = compare(&bytes, expected.data.data());
        assert!(
            far == 0 && max <= 1,
            "{far} pixels beyond {NEAR}, largest difference {max}"
        );
    }
}
