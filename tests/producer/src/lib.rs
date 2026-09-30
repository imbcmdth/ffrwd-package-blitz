//! `emit_changes`, for the end-to-end test of change rows arriving with
//! frames: passes its frames through and emits the rows its `emit`
//! parameter lists, each on the first frame at or after its time. `emit` is
//! a JSON array of `[seconds, row]` pairs, where a row is one change object
//! or an array of them. Not shipped.

wit_bindgen::generate!({
    path: "wit",
    world: "window-module",
});

use std::cell::RefCell;

use exports::ffrwd::av::window_filter::{
    Format, FramePayload, Guest, InWindow, Meta, OutFrame, Processed, StreamInfo, WindowMeta,
};
use serde_json::Value;

/// The change row record, as `compose`'s `stream_changes` column declares
/// it (compose_core::params::ROWS_SCHEMA, copied so this module does not
/// link Blitz).
const ROWS_SCHEMA: &str = r#"{"type":"object","properties":{"at":{"type":"number"},"select":{"type":"string"},"change":{"type":"string"},"text":{"type":"string"},"html":{"type":"string"}},"required":["select"]}"#;

struct State {
    tick: f64,
    /// (seconds, row line), soonest first.
    pending: Vec<(f64, String)>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn parse(params: &str) -> Result<Vec<(f64, String)>, String> {
    let v: Value = serde_json::from_str(params).map_err(|e| e.to_string())?;
    let emit = v.get("emit").and_then(Value::as_str).unwrap_or("[]");
    let list: Value = serde_json::from_str(emit).map_err(|e| format!("emit: {e}"))?;
    let mut out = Vec::new();
    for pair in list.as_array().ok_or("emit must be an array")? {
        let t = pair
            .get(0)
            .and_then(Value::as_f64)
            .ok_or("each entry is [seconds, row]")?;
        let row = pair.get(1).ok_or("each entry is [seconds, row]")?;
        out.push((t, row.to_string()));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(out)
}

struct EmitChanges;

impl Guest for EmitChanges {
    fn describe() -> WindowMeta {
        WindowMeta {
            meta: Meta {
                name: "emit_changes".into(),
                version: "0.1.0".into(),
                params_schema: r#"{"type":"object","properties":{"emit":{"type":"string"}},"additionalProperties":false}"#.into(),
                rows_schema: ROWS_SCHEMA.into(),
                pixel_formats: vec!["rgba".into()],
                sample_formats: vec![],
                sample_rates: vec![],
                channel_counts: vec![],
                rows_language: vec![],
            },
            window: 1,
            stride: 1,
            pure: false,
            one_to_one: true,
            reads_rows: false,
            forwards_rows: false,
            inputs: 1,
            feeders: vec![],
        }
    }

    fn init(_format: Format, stream_info: StreamInfo, params: String) -> Result<(), String> {
        let tb = stream_info.time_base;
        let pending = parse(&params)?;
        STATE.with_borrow_mut(|s| {
            *s = Some(State {
                tick: tb.num as f64 / tb.den as f64,
                pending,
            })
        });
        Ok(())
    }

    fn set_params(params: String) -> Result<(), String> {
        let pending = parse(&params)?;
        STATE.with_borrow_mut(|s| s.as_mut().unwrap().pending = pending);
        Ok(())
    }

    fn process(window: &InWindow, _trailing: Vec<String>, _last: bool) -> Processed {
        STATE.with_borrow_mut(|s| {
            let s = s.as_mut().unwrap();
            let mut frames = Vec::new();
            for i in 0..window.len() {
                let pts = window.pts(i);
                let t = pts as f64 * s.tick;
                let due = s.pending.partition_point(|(at, _)| *at <= t + 1e-9);
                let rows = s.pending.drain(..due).map(|(_, r)| r).collect();
                frames.push(OutFrame {
                    pts,
                    frame: FramePayload::Same,
                    rows,
                });
            }
            Processed {
                frames,
                trailing: vec![],
            }
        })
    }
}

export!(EmitChanges);
