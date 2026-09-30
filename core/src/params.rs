//! The module's parameters, as the host hands them over: one JSON object.

use serde_json::Value;

use crate::changes::Row;
use crate::geometry::{Fit, Geometry};

/// The document used when `html` is empty: input 0 over the whole frame.
pub const DEFAULT_HTML: &str = "<!DOCTYPE html><html><head><style>\
html, body { margin: 0; padding: 0; overflow: hidden; background: #000; }\
#v0 { position: absolute; left: 0; top: 0; width: 100vw; height: 100vh; }\
</style></head><body><img id=\"v0\" src=\"ffrwd:0\"></body></html>";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Log {
    /// Nothing but errors.
    Off,
    /// One line when the instance opens and one when it ends.
    Summary,
    /// A line per frame.
    Frame,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    /// The document, as HTML text.
    pub html: String,
    /// Change rows from the `changes` parameter.
    pub changes: Vec<Row>,
    /// The canvas the document is composed for, px; `None` follows the
    /// output frame.
    pub width: Option<f64>,
    pub height: Option<f64>,
    /// The design size, CSS px: the document's viewport; `None` follows the
    /// canvas (a single side given takes the canvas's aspect for the other).
    pub css_width: Option<f64>,
    pub css_height: Option<f64>,
    /// How the design fills the canvas when their aspects differ.
    pub fit: Fit,
    /// Hand input 0 back untouched when the frame would be exactly it.
    pub bypass: bool,
    pub log: Log,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            html: DEFAULT_HTML.into(),
            changes: Vec::new(),
            width: None,
            height: None,
            css_width: None,
            css_height: None,
            fit: Fit::Contain,
            bypass: true,
            log: Log::Summary,
        }
    }
}

/// The JSON Schema describe publishes, which the compiler checks a call's
/// values against.
pub const SCHEMA: &str = r#"{"type":"object","properties":{"html":{"type":"string"},"changes":{"type":"string"},"width":{"type":"number"},"height":{"type":"number"},"css_width":{"type":"number"},"css_height":{"type":"number"},"fit":{"type":"string","enum":["contain","stretch"]},"bypass":{"type":"boolean"},"log":{"type":"string","enum":["off","summary","frame"]}},"additionalProperties":false}"#;

/// The JSON Schema of the change rows `compose` reads beside its stream.
pub const ROWS_SCHEMA: &str = r#"{"type":"object","properties":{"at":{"type":"number"},"select":{"type":"string"},"change":{"type":"string"},"text":{"type":"string"},"html":{"type":"string"}},"required":["select"]}"#;

impl Params {
    pub fn parse(params: &str) -> Result<Params, String> {
        let v: Value = if params.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str(params).map_err(|e| format!("params: {e}"))?
        };
        let Value::Object(m) = &v else {
            return Err("params must be a JSON object".into());
        };
        let mut p = Params::default();
        for (k, val) in m {
            if val.is_null() {
                continue;
            }
            match k.as_str() {
                "html" => {
                    let s = val.as_str().ok_or("html must be text")?;
                    if !s.trim().is_empty() {
                        p.html = s.to_string();
                    }
                }
                "changes" => {
                    p.changes = match val {
                        Value::String(s) => Row::parse_list(s),
                        other => Row::from_json_value(other),
                    }
                    .map_err(|e| format!("changes: {e}"))?;
                }
                "width" => p.width = Some(size(k, val)?),
                "height" => p.height = Some(size(k, val)?),
                "css_width" => p.css_width = Some(size(k, val)?),
                "css_height" => p.css_height = Some(size(k, val)?),
                "fit" => {
                    p.fit = match val.as_str() {
                        Some("contain") => Fit::Contain,
                        Some("stretch") => Fit::Stretch,
                        _ => return Err("fit must be 'contain' or 'stretch'".into()),
                    };
                }
                "bypass" => {
                    p.bypass = boolean(val).ok_or("bypass must be true or false")?;
                }
                "log" => {
                    p.log = match val.as_str() {
                        Some("off") => Log::Off,
                        Some("summary") => Log::Summary,
                        Some("frame") => Log::Frame,
                        _ => return Err("log must be 'off', 'summary' or 'frame'".into()),
                    };
                }
                other => return Err(format!("unknown parameter {other:?}")),
            }
        }
        Ok(p)
    }

    /// Whether the document has to be built again for `other`.
    pub fn same_document(&self, other: &Params) -> bool {
        self.html == other.html
            && self.changes == other.changes
            && self.width == other.width
            && self.height == other.height
            && self.css_width == other.css_width
            && self.css_height == other.css_height
            && self.fit == other.fit
    }

    /// Where the document lands in an output frame of `out`, over input
    /// frames of `input`.
    pub fn geometry(&self, out: (u32, u32), input: (u32, u32)) -> Geometry {
        Geometry::new(
            out,
            input,
            (self.width, self.height),
            (self.css_width, self.css_height),
            self.fit,
        )
    }
}

fn size(name: &str, v: &Value) -> Result<f64, String> {
    let n = number(v).ok_or(format!("{name} must be a number"))?;
    if !(n.is_finite() && n >= 1.0) {
        return Err(format!("{name} must be at least 1, got {n}"));
    }
    Ok(n)
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn boolean(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}
