//! Change rows: the run-time input that drives a document.
//!
//! ```json
//! {"at": 3.01, "select": "div.foo", "change": "-foo +bar ~baz"}
//! {"at": 4.0,  "select": "#name",   "text": "Jane Example"}
//! {"at": 5.0,  "select": "#slot",   "html": "<div class=logo></div>"}
//! ```
//!
//! A row matches its selector first (Blitz's `query_selector_all`) and then
//! applies its change to every match, through Blitz's `DocumentMutator`.
//! There is no allow-list: any selector, any markup, from any source.
//!
//! `at` is seconds of stream time and is optional. A row takes effect at its
//! `at` when that is at or after the time it arrives at, and at its arrival
//! otherwise; a row without `at` takes effect at its arrival. A row arrives
//! at its message's time, and a row of the `rows` parameter at stream time
//! 0. Every instance of a frame-parallel lane that is handed a row with the
//! same arrival time therefore applies it at the same time.
//!
//! The [`Timeline`] applies rows in replay mode: each group of rows with the
//! same `at` is applied and the document resolved at that `at`, then the
//! frame is resolved at its own time. What a row starts (a CSS animation, a
//! transition) therefore starts at exactly `at`, whichever frames happen to
//! be rendered. Stylo's clock never runs backwards, which is why a row
//! cannot take effect before it arrives.
//!
//! The timeline keeps a log of every row applied since the last full-page
//! `html` replace (a row whose selector matched `html` or `body`, which is
//! the log's first entry), each with the time it took effect. A fresh
//! document plus [`Timeline::join`] of that log is the document an instance
//! that saw everything has.

use std::collections::HashSet;
use std::time::Instant;

use blitz_dom::{BaseDocument, NodeId, QualName, local_name, ns};
use serde_json::Value;

use crate::Compositor;

#[derive(Clone, Debug, PartialEq)]
pub enum ClassOp {
    Remove(String),
    Add(String),
    Toggle(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Classes(Vec<ClassOp>),
    Text(String),
    Html(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Seconds of stream time; `None` takes the time of whatever delivers it.
    pub at: Option<f64>,
    pub select: String,
    pub change: Change,
}

/// Where a row came from. Rows from the `rows` parameter are rebuilt from
/// the parameter; rows that arrived while the instance ran are kept in the
/// log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Param,
    Stream,
}

/// `"-foo +bar ~baz"`: remove, add, toggle.
pub fn parse_class_ops(s: &str) -> Result<Vec<ClassOp>, String> {
    s.split_ascii_whitespace()
        .map(|tok| {
            let mut chars = tok.chars();
            let op = chars.next().unwrap_or(' ');
            let name: String = chars.collect();
            if name.is_empty() {
                return Err(format!("empty class name in {tok:?}"));
            }
            match op {
                '-' => Ok(ClassOp::Remove(name)),
                '+' => Ok(ClassOp::Add(name)),
                '~' => Ok(ClassOp::Toggle(name)),
                _ => Err(format!("class change {tok:?} must start with -, + or ~")),
            }
        })
        .collect()
}

impl Row {
    pub fn classes(at: impl Into<Option<f64>>, select: &str, change: &str) -> Row {
        Row {
            at: at.into(),
            select: select.into(),
            change: Change::Classes(parse_class_ops(change).expect("class change")),
        }
    }

    pub fn text(at: impl Into<Option<f64>>, select: &str, text: &str) -> Row {
        Row {
            at: at.into(),
            select: select.into(),
            change: Change::Text(text.into()),
        }
    }

    pub fn html(at: impl Into<Option<f64>>, select: &str, html: &str) -> Row {
        Row {
            at: at.into(),
            select: select.into(),
            change: Change::Html(html.into()),
        }
    }

    /// Whether a JSON object is meant as a change row: it has `select`.
    /// Rows without one belong to somebody else and are left alone.
    pub fn is_change(v: &Value) -> bool {
        v.get("select").is_some()
    }

    pub fn from_value(v: &Value) -> Result<Row, String> {
        let at = match v.get("at") {
            None | Some(Value::Null) => None,
            Some(a) => Some(
                a.as_f64()
                    .filter(|a| a.is_finite())
                    .ok_or("\"at\" must be a number of seconds")?,
            ),
        };
        let select = v
            .get("select")
            .and_then(Value::as_str)
            .ok_or("row needs a string \"select\"")?
            .to_string();
        let s = |k: &str| v.get(k).and_then(Value::as_str);
        let change = match (s("change"), s("text"), s("html")) {
            (Some(c), None, None) => Change::Classes(parse_class_ops(c)?),
            (None, Some(t), None) => Change::Text(t.into()),
            (None, None, Some(h)) => Change::Html(h.into()),
            _ => {
                return Err(
                    "row needs exactly one of \"change\", \"text\" and \"html\", as a string"
                        .into(),
                );
            }
        };
        Ok(Row { at, select, change })
    }

    /// Rows as JSON text: one row object, an array, or one of those per
    /// line.
    pub fn parse_list(s: &str) -> Result<Vec<Row>, String> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(Vec::new());
        }
        if let Ok(v) = serde_json::from_str::<Value>(s) {
            return Row::from_json_value(&v);
        }
        let mut rows = Vec::new();
        for l in s.lines().filter(|l| !l.trim().is_empty()) {
            let v: Value = serde_json::from_str(l).map_err(|e| e.to_string())?;
            rows.extend(Row::from_json_value(&v)?);
        }
        Ok(rows)
    }

    /// One row object, or an array whose elements are rows or arrays of
    /// rows, in order.
    pub fn from_json_value(v: &Value) -> Result<Vec<Row>, String> {
        match v {
            Value::Array(a) => {
                let mut rows = Vec::new();
                for item in a {
                    rows.extend(Row::from_json_value(item)?);
                }
                Ok(rows)
            }
            Value::Object(_) => Ok(vec![Row::from_value(v)?]),
            _ => Err("rows must be objects, or arrays of objects".into()),
        }
    }

    /// The change rows among one upstream row line: an object with `select`
    /// is one, anything else is not ours. Returns the rows and the errors of
    /// the ones that meant to be change rows and are not well formed.
    pub fn from_upstream(line: &str) -> (Vec<Row>, Vec<String>) {
        let (mut rows, mut errors) = (Vec::new(), Vec::new());
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return (rows, errors);
        };
        fn flatten<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
            match v {
                Value::Array(a) => a.iter().for_each(|i| flatten(i, out)),
                other => out.push(other),
            }
        }
        let mut items = Vec::new();
        flatten(&v, &mut items);
        for item in items {
            if !Row::is_change(item) {
                continue;
            }
            match Row::from_value(item) {
                Ok(r) => rows.push(r),
                Err(e) => errors.push(format!("{e}: {item}")),
            }
        }
        (rows, errors)
    }

    pub fn to_value(&self) -> Value {
        let mut m = serde_json::Map::new();
        if let Some(at) = self.at {
            m.insert("at".into(), at.into());
        }
        m.insert("select".into(), self.select.clone().into());
        match &self.change {
            Change::Classes(ops) => {
                let s: Vec<String> = ops
                    .iter()
                    .map(|o| match o {
                        ClassOp::Remove(n) => format!("-{n}"),
                        ClassOp::Add(n) => format!("+{n}"),
                        ClassOp::Toggle(n) => format!("~{n}"),
                    })
                    .collect();
                m.insert("change".into(), s.join(" ").into());
            }
            Change::Text(t) => {
                m.insert("text".into(), t.clone().into());
            }
            Change::Html(h) => {
                m.insert("html".into(), h.clone().into());
            }
        }
        Value::Object(m)
    }
}

/// What one row did.
#[derive(Clone, Copy, Debug, Default)]
pub struct Applied {
    /// Elements the selector matched.
    pub matched: usize,
    /// An `html` row whose selector matched `html` or `body`.
    pub full_page: bool,
}

pub(crate) fn class_list_after(current: Option<&str>, ops: &[ClassOp]) -> String {
    let mut list: Vec<String> = current
        .unwrap_or("")
        .split_ascii_whitespace()
        .map(String::from)
        .collect();
    for op in ops {
        match op {
            ClassOp::Remove(n) => list.retain(|c| c != n),
            ClassOp::Add(n) => {
                if !list.contains(n) {
                    list.push(n.clone())
                }
            }
            ClassOp::Toggle(n) => {
                if list.contains(n) {
                    list.retain(|c| c != n)
                } else {
                    list.push(n.clone())
                }
            }
        }
    }
    list.join(" ")
}

/// Applies one row to the document: match, then change every match.
pub fn apply(doc: &mut BaseDocument, row: &Row) -> Result<Applied, String> {
    let ids: Vec<NodeId> = doc
        .query_selector_all(&row.select)
        .map_err(|e| format!("bad selector {:?}: {e:?}", row.select))?
        .into_vec();
    let mut out = Applied {
        matched: ids.len(),
        full_page: false,
    };
    if ids.is_empty() {
        return Ok(out);
    }
    match &row.change {
        Change::Classes(ops) => {
            // Read every match's class list first, then write: the match set
            // is fixed before anything changes.
            let updates: Vec<(NodeId, String)> = ids
                .iter()
                .map(|id| {
                    let cur = doc.get_node(*id).and_then(|n| n.attr(local_name!("class")));
                    (*id, class_list_after(cur, ops))
                })
                .collect();
            let mut m = doc.mutate();
            let class = QualName::new(None, ns!(), local_name!("class"));
            for (id, value) in updates {
                m.set_attribute(id, class.clone(), &value);
            }
        }
        Change::Text(text) => {
            let ids = outermost(doc, &ids);
            let mut m = doc.mutate();
            for id in ids {
                m.remove_and_drop_all_children(id);
                if !text.is_empty() {
                    let t = m.create_text_node(text);
                    m.append_children(id, &[t]);
                }
            }
        }
        Change::Html(html) => {
            out.full_page = ids.iter().any(|id| {
                doc.get_node(*id)
                    .and_then(|n| n.element_data())
                    .is_some_and(|e| matches!(e.name.local.as_ref(), "html" | "body"))
            });
            let ids = outermost(doc, &ids);
            let mut m = doc.mutate();
            for id in ids {
                m.set_inner_html(id, html);
            }
        }
    }
    Ok(out)
}

/// The matches that have no matched ancestor. Replacing an element's
/// children drops its descendants, and a dropped node's id can be reused by
/// the nodes the same row creates, so the inner matches are skipped: in a
/// browser they would be detached by then and change nothing visible.
fn outermost(doc: &BaseDocument, ids: &[NodeId]) -> Vec<NodeId> {
    let set: HashSet<NodeId> = ids.iter().copied().collect();
    ids.iter()
        .copied()
        .filter(|id| {
            let mut p = doc.get_node(*id).and_then(|n| n.parent);
            while let Some(pid) = p {
                if set.contains(&pid) {
                    return false;
                }
                p = doc.get_node(pid).and_then(|n| n.parent);
            }
            true
        })
        .collect()
}

/// The smallest step between the resolves that carry an animation across a
/// gap, and the most of them one resolve makes: an animation shorter than
/// two milliseconds, or a gap of over a thousand steps, is carried in
/// bigger ones.
const MIN_STEP: f64 = 0.001;
const MAX_STEPS: usize = 1000;

/// A row with the time it applies at, settled.
#[derive(Clone, Debug, PartialEq)]
pub struct Timed {
    pub at: f64,
    pub row: Row,
    pub source: Source,
}

/// What one `advance` did, in microseconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct Step {
    pub rows: usize,
    pub full_page: bool,
    /// Matching plus mutation (includes HTML parsing and, for `<img>` with a
    /// `data:` URI, the image decode, which happens inside `fetch`).
    pub apply_us: f64,
    /// Resolves at the rows' own times.
    pub replay_resolve_us: f64,
    /// The frame's resolve.
    pub resolve_us: f64,
}

pub struct Timeline {
    pending: Vec<Timed>,
    log: Vec<Timed>,
    /// The latest stream time the document was resolved at.
    clock: f64,
    /// The shortest animation in the document as last resolved, seconds.
    shortest: Option<f64>,
    pub errors: Vec<String>,
}

impl Default for Timeline {
    fn default() -> Self {
        Timeline::new()
    }
}

impl Timeline {
    pub fn new() -> Timeline {
        Timeline {
            pending: Vec::new(),
            log: Vec::new(),
            clock: f64::NEG_INFINITY,
            shortest: None,
            errors: Vec::new(),
        }
    }

    /// Queues a row that arrived at `arrival` seconds, after every queued
    /// row with the same or an earlier time, so rows sharing a time keep
    /// the order they arrived in. It takes effect at its `at`, or at
    /// `arrival` when it has none or its `at` is earlier.
    pub fn push(&mut self, row: Row, arrival: f64, source: Source) {
        let at = row.at.map_or(arrival, |a| a.max(arrival));
        let i = self.pending.partition_point(|r| r.at <= at);
        self.pending.insert(i, Timed { at, row, source });
    }

    pub fn extend(&mut self, rows: impl IntoIterator<Item = Row>, arrival: f64, source: Source) {
        for r in rows {
            self.push(r, arrival, source);
        }
    }

    fn push_timed(&mut self, t: Timed) {
        let i = self.pending.partition_point(|r| r.at <= t.at);
        self.pending.insert(i, t);
    }

    /// Rows applied since the last full-page replace (which is the first
    /// entry, when there was one), each with the time it took effect.
    pub fn log(&self) -> &[Timed] {
        &self.log
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// The last time the document was resolved at.
    pub fn now(&self) -> f64 {
        self.clock
    }

    fn apply_one(&mut self, c: &mut Compositor, t: Timed, at: f64) -> bool {
        match c.apply(&t.row) {
            Ok(a) => {
                if a.full_page {
                    self.log.clear();
                }
                self.log.push(Timed { at, ..t });
                a.full_page
            }
            Err(e) => {
                self.errors.push(e);
                false
            }
        }
    }

    /// Resolves at `t` (never before the last resolve).
    fn resolve(&mut self, c: &mut Compositor, t: f64) {
        let t = t.max(self.clock);
        c.resolve(t);
        self.clock = t;
        self.shortest = c.shortest_animation();
    }

    /// Resolves at steps from the last resolve up to, not including, `t`,
    /// when the gap is longer than half the shortest animation: Blitz moves
    /// a CSS animation on by at most one iteration per resolve (it calls
    /// Stylo's `iterate_if_necessary` once per resolve), so a resolve that
    /// jumps two iterations samples the animation clamped to the end of the
    /// one it is in. With the steps, an instance that skips frames (a
    /// frame-parallel worker, or one opened late) renders what an instance
    /// that saw every frame renders. It runs before anything due at `t` is
    /// applied, so no change is seen early.
    fn catch_up(&mut self, c: &mut Compositor, t: f64) {
        if !self.clock.is_finite() {
            return;
        }
        let mut n = 0;
        while let Some(d) = self.shortest {
            let s = self.clock + (d * 0.5).max(MIN_STEP);
            if s >= t || n == MAX_STEPS {
                break;
            }
            self.resolve(c, s);
            n += 1;
        }
    }

    /// Resolves the document once at `epoch`, the time every instance's
    /// timeline is anchored at (the base document's own animations start
    /// then). Call it on a fresh compositor before its first frame.
    pub fn start(&mut self, c: &mut Compositor, epoch: f64) {
        self.resolve(c, epoch);
    }

    /// Applies and resolves each group of rows due by `t` at its own time,
    /// then resolves the frame at `t`.
    pub fn advance(&mut self, c: &mut Compositor, t: f64) -> Step {
        let mut step = self.replay_due(c, t);
        let s = Instant::now();
        self.catch_up(c, t);
        self.resolve(c, t);
        step.resolve_us = s.elapsed().as_secs_f64() * 1e6;
        step
    }

    /// The first half of `advance`: each group of rows due by `t` (same
    /// `at`) is applied and resolved at its own time.
    pub fn replay_due(&mut self, c: &mut Compositor, t: f64) -> Step {
        let due = self.pending.partition_point(|r| r.at <= t);
        let rows: Vec<Timed> = self.pending.drain(..due).collect();
        let mut step = Step {
            rows: rows.len(),
            ..Default::default()
        };
        let mut rows = rows.into_iter().peekable();
        while let Some(first) = rows.next() {
            // A row that arrives after its time takes effect at the last
            // resolve: Stylo's clock must not run backwards.
            let group_at = first.at;
            let at = group_at.max(self.clock);
            self.catch_up(c, at);
            let s = Instant::now();
            step.full_page |= self.apply_one(c, first, at);
            while rows.peek().is_some_and(|r| r.at == group_at) {
                let r = rows.next().unwrap();
                step.full_page |= self.apply_one(c, r, at);
            }
            step.apply_us += s.elapsed().as_secs_f64() * 1e6;
            let s = Instant::now();
            self.resolve(c, at);
            step.replay_resolve_us += s.elapsed().as_secs_f64() * 1e6;
        }
        step
    }

    /// A late joiner: starts `c` (a fresh compositor on the base document) at
    /// `epoch`, then applies and resolves each logged row at its own time.
    /// The returned timeline carries the log on; the caller's next `advance`
    /// renders its first frame.
    pub fn join(c: &mut Compositor, epoch: f64, log: &[Timed]) -> Timeline {
        let mut tl = Timeline::new();
        tl.start(c, epoch);
        for t in log {
            tl.push_timed(t.clone());
        }
        tl.replay_due(c, f64::INFINITY);
        tl
    }

    /// Rows still waiting for their time.
    pub fn pending_rows(&self) -> &[Timed] {
        &self.pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_ops() {
        let ops = parse_class_ops("-foo +bar ~baz").unwrap();
        assert_eq!(class_list_after(Some("foo baz qux"), &ops), "qux bar");
        assert!(parse_class_ops("foo").is_err());
        assert!(parse_class_ops("+").is_err());
    }
}
