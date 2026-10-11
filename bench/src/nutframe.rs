//! Pulls one rgba frame out of a NUT file (what `ffrwd-wasm -f nut` writes
//! of the module's picture) into a PNG, and compares it with another PNG:
//! how many pixels differ, by how much at most. For checking the wasm run's
//! picture against the native bench's dump of the same frame.
//!
//! ```text
//! nutframe <file.nut> <frame index> <out.png> [--compare <ref.png>]
//! ```

use std::fs::File;
use std::io::BufReader;

use ffrwd_nut::Demuxer;

fn usage() -> ! {
    eprintln!("usage: nutframe <file.nut> <frame index> <out.png> [--compare <ref.png>]");
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        usage();
    }
    let index: usize = args[1].parse().unwrap_or_else(|_| usage());
    let compare = args.iter().position(|a| a == "--compare").and_then(|i| args.get(i + 1));

    let file = File::open(&args[0]).expect("open nut");
    let mut demuxer = Demuxer::open(BufReader::new(file)).expect("nut headers");
    let stream = demuxer.stream().clone();
    let (w, h) = match stream.media {
        ffrwd_nut::Media::Video { width, height, .. } => (width, height),
        _ => panic!("not a video stream"),
    };
    let mut data = Vec::new();
    let mut n = 0usize;
    loop {
        match demuxer.read_packet(&mut data).expect("nut packet") {
            Some(packet) => {
                if n == index {
                    eprintln!(
                        "nutframe: frame {index} pts {} {}x{} {} bytes",
                        packet.pts,
                        w,
                        h,
                        data.len()
                    );
                    break;
                }
                n += 1;
            }
            None => {
                eprintln!("nutframe: only {n} frames in the file");
                std::process::exit(1);
            }
        }
    }
    assert_eq!(data.len(), (w * h * 4) as usize, "a packed rgba frame");
    image::save_buffer(&args[2], &data, w, h, image::ColorType::Rgba8).expect("png");

    if let Some(reference) = compare {
        let other = image::open(reference).expect("reference png").into_rgba8();
        assert_eq!((other.width(), other.height()), (w, h), "same size");
        let mut differing = 0u64;
        let mut over_8 = 0u64;
        let mut max = 0u8;
        let mut sum = 0u64;
        for (a, b) in data.chunks_exact(4).zip(other.as_raw().chunks_exact(4)) {
            let d = (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap();
            if d > 0 {
                differing += 1;
                sum += d as u64;
            }
            if d > 8 {
                over_8 += 1;
            }
            max = max.max(d);
        }
        let total = (w * h) as u64;
        println!(
            "compare: {} of {} pixels differ ({:.3}%), {} by more than 8 ({:.4}%), max difference {}, mean over differing {:.2}",
            differing,
            total,
            differing as f64 * 100.0 / total as f64,
            over_8,
            over_8 as f64 * 100.0 / total as f64,
            max,
            if differing > 0 { sum as f64 / differing as f64 } else { 0.0 }
        );
    }
}
