//! Test only: a node that writes change rows at given times, to drive
//! `compose`'s `changes` from another node. `emit` is a JSON array of
//! `[seconds, row]` pairs, a row being one change object or an array of
//! them, each written as its own message on the first tick at or after its
//! time. Not shipped.

use ffrwd_node::{Bound, Init, Input, Node, Out, Output, Rational, Result, Shape, Tick};
use serde::Deserialize;
use serde_json::Value;

/// The change row record, as `compose`'s `changes` declares it.
const ROWS_SCHEMA: &str = r#"{"type":"object","properties":{"at":{"type":"number"},"select":{"type":"string"},"change":{"type":"string"},"text":{"type":"string"},"html":{"type":"string"}},"required":["select"]}"#;

#[derive(Deserialize)]
struct Params {
    emit: String,
}

struct EmitChanges {
    v: u32,
    base: Rational,
    /// (seconds, row), soonest first.
    pending: Vec<(f64, String)>,
}

fn rows(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| rows(item, out)),
        other => out.push(other.to_string()),
    }
}

fn parse(emit: &str) -> Result<Vec<(f64, String)>, String> {
    let list: Value = serde_json::from_str(emit).map_err(|e| format!("emit: {e}"))?;
    let mut out = Vec::new();
    for pair in list.as_array().ok_or("emit must be an array")? {
        let t = pair
            .get(0)
            .and_then(Value::as_f64)
            .ok_or("each entry is [seconds, row]")?;
        let mut written = Vec::new();
        rows(
            pair.get(1).ok_or("each entry is [seconds, row]")?,
            &mut written,
        );
        out.extend(written.into_iter().map(|row| (t, row)));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(out)
}

impl Node for EmitChanges {
    const NAME: &'static str = "emit_changes";
    const VERSION: &'static str = "0.2.0";
    const PARAMS_SCHEMA: &'static str = r#"{"type":"object","properties":{"emit":{"type":"string","default":"[]"}},"additionalProperties":false}"#;
    type Params = Params;

    fn shape(_: &Params, _: &Bound) -> Result<Shape> {
        Ok(Shape::new()
            .input(Input::video("v").clock())
            .output(Output::rows("changes").schema_json(ROWS_SCHEMA)))
    }

    fn init(params: Params, init: &Init) -> Result<EmitChanges> {
        let v = init.stream("v")?;
        Ok(EmitChanges {
            v: v.id,
            base: v.info.time_base,
            pending: parse(&params.emit)?,
        })
    }

    fn process(&mut self, tick: &Tick, out: &mut Out) -> Result<()> {
        let Some(frame) = tick.frame(self.v) else {
            return Ok(());
        };
        let t = frame.pts as f64 * (self.base.num as f64 / self.base.den as f64);
        let due = self.pending.partition_point(|(at, _)| *at <= t + 1e-9);
        for (_, row) in self.pending.drain(..due) {
            out.message("changes", frame.pts, row.into_bytes())?;
        }
        Ok(())
    }
}

ffrwd_node::export!(EmitChanges);
