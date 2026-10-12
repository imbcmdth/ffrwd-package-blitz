//! Step A of the 4K60 compositing experiment: the harness on native wgpu,
//! 600 frames at t = n / 60, one JSON row a frame on stdout. No wasm, no
//! sidecar: this is the floor the wasm run is measured against.
//!
//! ```text
//! bench --n 8 --variant 1 [--all-4k] [--frames 600] [--backend dx12|vulkan|gl|metal]
//!       [--dump <frame> <file.png>] > rows.ndjson
//! ```

use std::io::Write;
use std::time::Instant;

use blitz_bench_doc::{self as doc, End, Variant};
use blitz_bench_harness::{Config, Harness};

struct Args {
    config: Config,
    backends: wgpu::Backends,
    dump: Option<(u32, String)>,
}

fn usage() -> ! {
    eprintln!(
        "usage: bench --n <inputs> --variant <1|2|3|4> [--all-4k] [--frames <count>] \
         [--backend dx12|vulkan|gl|metal] [--dump <frame> <file.png>]"
    );
    std::process::exit(2)
}

fn parse_args() -> Args {
    let mut a = Args {
        config: Config {
            n: 2,
            variant: Variant::Resident,
            all_4k: false,
            frames: doc::FRAMES,
            step: "A".into(),
        },
        backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
        dump: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        match k.as_str() {
            "--n" => {
                a.config.n = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| usage())
            }
            "--variant" => {
                a.config.variant = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .and_then(Variant::from_number)
                    .unwrap_or_else(|| usage())
            }
            "--all-4k" => a.config.all_4k = true,
            "--frames" => {
                a.config.frames = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| usage())
            }
            "--backend" => {
                a.backends = match it.next().as_deref() {
                    Some("dx12") => wgpu::Backends::DX12,
                    Some("vulkan") => wgpu::Backends::VULKAN,
                    Some("gl") => wgpu::Backends::GL,
                    Some("metal") => wgpu::Backends::METAL,
                    _ => usage(),
                }
            }
            "--dump" => {
                let frame = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| usage());
                let path = it.next().unwrap_or_else(|| usage());
                a.dump = Some((frame, path));
            }
            _ => usage(),
        }
    }
    if !(1..=64).contains(&a.config.n) {
        usage();
    }
    a
}

#[cfg(windows)]
fn peak_rss() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: a zeroed counters struct with its size set is what the call asks for.
    unsafe {
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        (K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) != 0)
            .then_some(c.PeakWorkingSetSize as u64)
    }
}

#[cfg(target_os = "linux")]
fn peak_rss() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = s.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn peak_rss() -> Option<u64> {
    None
}

fn main() {
    let args = parse_args();
    let (w, h) = doc::OUT;
    let out = std::io::stdout();
    let mut out = std::io::BufWriter::new(out.lock());

    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    desc.backends = args.backends;
    let instance = wgpu::Instance::new(desc);
    let frames = args.config.frames;
    let mut harness = Harness::new(&instance, args.config.clone(), None);
    let meta = harness.meta().clone();
    eprintln!(
        "bench: {} on {} ({}); n={} variant={} ({}) all_4k={} frames={}; gpu timing: {:?}; {}",
        meta.adapter,
        meta.backend,
        meta.driver_info.replace('\n', " "),
        meta.n,
        meta.variant,
        args.config.variant.name(),
        meta.all_4k,
        frames,
        meta.gpu_timing,
        meta.notes.join("; ")
    );
    writeln!(out, "{}", serde_json::to_string(&meta).unwrap()).unwrap();

    let run = Instant::now();
    let mut dumped = false;
    for frame in 0..frames {
        let dump_now = matches!(&args.dump, Some((f, _)) if *f == frame) && !dumped;
        let (row, pixels) = harness.frame(frame, dump_now);
        if dump_now {
            let path = &args.dump.as_ref().unwrap().1;
            image::save_buffer(path, &pixels.unwrap(), w, h, image::ColorType::Rgba8).expect("png");
            eprintln!("bench: frame {frame} written to {path}");
            dumped = true;
        }
        writeln!(out, "{}", serde_json::to_string(&row).unwrap()).unwrap();
        if frame % 100 == 99 {
            eprintln!(
                "bench: frame {frame}: cpu {:.2} ms, gpu {:.2} ms, frame {:.2} ms",
                row.cpu_us() / 1e3,
                row.gpu_us / 1e3,
                row.frame_us / 1e3
            );
        }
    }
    let end = End {
        kind: "end".into(),
        frames,
        wall_us: run.elapsed().as_secs_f64() * 1e6,
        peak_rss_bytes: peak_rss(),
    };
    writeln!(out, "{}", serde_json::to_string(&end).unwrap()).unwrap();
    out.flush().unwrap();
    eprintln!(
        "bench: {} frames in {:.2} s ({:.1} fps), peak rss {} MB",
        frames,
        end.wall_us / 1e6,
        frames as f64 / (end.wall_us / 1e6),
        end.peak_rss_bytes
            .map(|b| (b / (1 << 20)).to_string())
            .unwrap_or("?".into())
    );
}
