//! Turns the rows the bench (or the wasm run) writes into a table: per run,
//! median and p99 of each phase, frames a second, missed frames, peak
//! memory, adapter and backend.
//!
//! ```text
//! summarize [--skip <frames>] <rows.ndjson | dir>...
//! ```
//!
//! A directory stands for every `*.ndjson` in it. `--skip` leaves the first
//! frames out of the statistics (pipeline compilation lands in frame 0);
//! the default keeps every frame, as the experiment asks.

use std::path::{Path, PathBuf};

use blitz_bench_doc::{End, Meta, Row};

struct Run {
    file: String,
    meta: Meta,
    rows: Vec<Row>,
    end: Option<End>,
}

fn read_run(path: &Path) -> Option<Run> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let meta: Meta = serde_json::from_str(lines.next()?).ok()?;
    let mut rows = Vec::new();
    let mut end = None;
    for line in lines {
        if line.contains("\"kind\":\"end\"") {
            end = serde_json::from_str(line).ok();
        } else if let Ok(row) = serde_json::from_str::<Row>(line) {
            rows.push(row);
        }
    }
    Some(Run { file: path.display().to_string(), meta, rows, end })
}

fn percentile(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let i = ((v.len() as f64 - 1.0) * p).round() as usize;
    v[i.min(v.len() - 1)]
}

fn med_p99(rows: &[Row], f: impl Fn(&Row) -> f64) -> (f64, f64) {
    let mut v: Vec<f64> = rows.iter().map(f).collect();
    (percentile(&mut v, 0.5), percentile(&mut v, 0.99))
}

fn ms(us: f64) -> String {
    format!("{:.2}", us / 1e3)
}

fn pair(rows: &[Row], f: impl Fn(&Row) -> f64) -> String {
    let (m, p) = med_p99(rows, f);
    format!("{} / {}", ms(m), ms(p))
}

fn main() {
    let mut skip = 0usize;
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if a == "--skip" {
            skip = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        } else {
            let p = PathBuf::from(a);
            if p.is_dir() {
                let mut found: Vec<PathBuf> = std::fs::read_dir(&p)
                    .map(|d| {
                        d.flatten()
                            .map(|e| e.path())
                            .filter(|q| q.extension().is_some_and(|e| e == "ndjson"))
                            .collect()
                    })
                    .unwrap_or_default();
                found.sort();
                paths.extend(found);
            } else {
                paths.push(p);
            }
        }
    }
    if paths.is_empty() {
        eprintln!("usage: summarize [--skip <frames>] <rows.ndjson | dir>...");
        std::process::exit(2);
    }
    let mut runs: Vec<Run> = paths.iter().filter_map(|p| read_run(p)).collect();
    if runs.is_empty() {
        eprintln!("summarize: no runs read");
        std::process::exit(1);
    }
    runs.sort_by_key(|r| (r.meta.step.clone(), r.meta.all_4k, r.meta.n, r.meta.variant));

    let m = &runs[0].meta;
    println!(
        "Machine: {} ({}), driver {} {}, {} {}; GPU time by {:?}.",
        m.adapter, m.backend, m.driver, m.driver_info, m.os, m.arch, m.gpu_timing
    );
    for note in &m.notes {
        println!("Note: {note}");
    }
    if skip > 0 {
        println!("The first {skip} frame(s) of each run are left out of the statistics.");
    }
    println!();
    println!("Times in ms, median / p99. cpu = resolve + plan + paint_cmds + encode. submit+wait is the wall clock from submit to the queue idle. upload and wit are MB a frame (mean). miss = frames over 16.67 ms. fps = frames over the run's wall time.");
    println!();
    println!("| step | N | variant | inputs | frames | resolve | plan | paint_cmds | encode | cpu | gpu | submit+wait | readback | frame | fps | miss | upload | wit | peak MB |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for run in &runs {
        let rows: &[Row] = if run.rows.len() > skip { &run.rows[skip..] } else { &run.rows };
        if rows.is_empty() {
            eprintln!("summarize: {} has no rows", run.file);
            continue;
        }
        let frames = rows.len();
        let wall: f64 = rows.iter().map(|r| r.frame_us).sum();
        let fps = frames as f64 * 1e6 / wall.max(1.0);
        let miss = rows.iter().filter(|r| r.frame_us > 1e6 / 60.0).count();
        let upload = rows.iter().map(|r| r.upload_bytes as f64).sum::<f64>() / frames as f64 / 1e6;
        let wit: Option<f64> = {
            let v: Vec<f64> = rows.iter().filter_map(|r| r.wit_bytes.map(|b| b as f64)).collect();
            (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64 / 1e6)
        };
        let peak = run
            .end
            .as_ref()
            .and_then(|e| e.peak_rss_bytes)
            .map(|b| format!("{}", b / (1 << 20)))
            .unwrap_or("?".into());
        let inputs = if run.meta.all_4k { "all 4K" } else { "4K + 1080p" };
        let variant_name = blitz_bench_doc::Variant::from_number(run.meta.variant)
            .map(|v| v.name())
            .unwrap_or("?");
        println!(
            "| {} | {} | {} {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {:.1} | {} | {:.1} | {} | {} |",
            run.meta.step,
            run.meta.n,
            run.meta.variant,
            variant_name,
            inputs,
            frames,
            pair(rows, |r| r.resolve_us),
            pair(rows, |r| r.plan_us),
            pair(rows, |r| r.paint_cmds_us),
            pair(rows, |r| r.encode_us),
            pair(rows, |r| r.cpu_us()),
            pair(rows, |r| r.gpu_us),
            pair(rows, |r| r.submit_us + r.wait_us),
            pair(rows, |r| r.readback_us),
            pair(rows, |r| r.frame_us),
            fps,
            miss,
            upload,
            wit.map(|w| format!("{w:.1}")).unwrap_or("-".into()),
            peak,
        );
    }
    print_hashes(&runs);
}

/// The pixel hashes of the frames that were read back and hashed, so a run
/// on one renderer can be checked against another.
fn print_hashes(runs: &[Run]) {
    let mut lines = Vec::new();
    for run in runs {
        for row in &run.rows {
            if let Some(h) = row.pixel_hash {
                lines.push(format!(
                    "| {} | {} | {} | {} | {} | {:016x} |",
                    run.meta.step,
                    run.meta.n,
                    run.meta.variant,
                    if run.meta.all_4k { "all 4K" } else { "4K + 1080p" },
                    row.frame,
                    h
                ));
            }
        }
    }
    if lines.is_empty() {
        return;
    }
    println!();
    println!("Pixel hashes (FNV-1a of the frame read back), for checking one renderer against another:");
    println!();
    println!("| step | N | variant | inputs | frame | hash |");
    println!("|---|---|---|---|---|---|");
    for l in lines {
        println!("{l}");
    }
}
