//! Captions drawn into the document: cue rows, as a transcriber writes them,
//! turned into change rows that fill an element.
//!
//! ```json
//! {"text": "he goes for the", "start_t": 0.32, "end_t": 0.72, "final": false}
//! {"text": "he goes for the flank", "start_t": 0.32, "end_t": 1.12, "final": true}
//! ```
//!
//! Each stream of cues has an element, named by a selector. A row whose
//! `start_t` is a cue's already on screen is that cue grown, and replaces it;
//! any other starts a new cue after it. The last `lines` cues show at once.
//! A cue goes `hold` seconds after its final row arrived, or `stale` seconds
//! after its last row did when none was final. Every time here is when a row
//! reached the programme; `start_t` is only the cue's name, on whatever clock
//! its producer keeps, and is never compared with programme time.
//!
//! The text is WebVTT cue text: its tags (`c`, `i`, `b`, `u`, `ruby`, `rt`,
//! `v`, `lang`) become elements, timestamps and unknown tags are dropped, and
//! everything else is text. No markup but those tags reaches the document,
//! so a cue from an outside source cannot add any.
//!
//! The element's content at a time is a function of the rows folded by then
//! and that time alone, so an instance that renders some ticks and folds the
//! rest draws what one that rendered every tick would.

use serde_json::Value;

use crate::changes::Row;

/// How cues come and go, from a call's params.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CueParams {
    /// Seconds a cue stays after its final row.
    pub hold: f64,
    /// Seconds a cue stays after its last row when none was final.
    pub stale: f64,
    /// How many cues show at once in one element.
    pub lines: usize,
}

impl Default for CueParams {
    fn default() -> CueParams {
        CueParams {
            hold: 3.0,
            stale: 6.0,
            lines: 2,
        }
    }
}

/// One cue row as a transcriber writes it. Fields other than these are left
/// alone.
struct CueRow {
    text: String,
    start_t: Option<f64>,
    done: bool,
    voice: Option<String>,
}

impl CueRow {
    fn parse(json: &str) -> Result<CueRow, String> {
        let value: Value =
            serde_json::from_str(json).map_err(|e| format!("a cue row reads {json}: {e}"))?;
        let Some(text) = value.get("text").and_then(Value::as_str) else {
            return Err(format!("a cue row has a text, and {json} has none"));
        };
        Ok(CueRow {
            text: text.to_string(),
            start_t: value.get("start_t").and_then(Value::as_f64),
            done: value.get("final").and_then(Value::as_bool).unwrap_or(false),
            voice: value
                .get("voice")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }
}

#[derive(Clone, Debug)]
struct Cue {
    /// The stream it came on, from 0.
    stream: usize,
    /// The producer's name for the cue: its `start_t`.
    name: Option<f64>,
    text: String,
    voice: Option<String>,
    /// When its first row reached the programme.
    first: f64,
    /// When its latest row did.
    last: f64,
    /// When its final row did.
    done: Option<f64>,
}

impl Cue {
    /// Whether the cue shows at `t`.
    fn shows(&self, t: f64, p: &CueParams) -> bool {
        let gone = match self.done {
            Some(done) => done + p.hold,
            None => self.last + p.stale,
        };
        self.first <= t && t < gone
    }
}

struct Slot {
    select: String,
    cues: Vec<Cue>,
    /// What this instance last put into the element.
    shown: Option<String>,
}

/// The cues of every stream a call binds, each drawn into its element.
pub struct Cues {
    params: CueParams,
    /// One per stream, in the call's order; streams sharing an element
    /// share a slot.
    of_stream: Vec<usize>,
    slots: Vec<Slot>,
}

impl Cues {
    /// `streams` cue streams, the n-th (from 1) drawn into the element
    /// `template` names with `{n}` replaced by n. A template without `{n}`
    /// draws every stream into one element, in the order cues arrive.
    pub fn new(template: &str, streams: usize, params: CueParams) -> Cues {
        let mut slots: Vec<Slot> = Vec::new();
        let mut of_stream = Vec::with_capacity(streams);
        for n in 1..=streams {
            let select = template.replace("{n}", &n.to_string());
            let at = match slots.iter().position(|slot| slot.select == select) {
                Some(at) => at,
                None => {
                    slots.push(Slot {
                        select,
                        cues: Vec::new(),
                        shown: None,
                    });
                    slots.len() - 1
                }
            };
            of_stream.push(at);
        }
        Cues {
            params,
            of_stream,
            slots,
        }
    }

    /// The row a stream's cue arrived with, at programme time `t`. Rows are
    /// folded in time order. A row that is not a cue is refused, and changes
    /// nothing.
    pub fn fold(&mut self, stream: usize, t: f64, json: &str) -> Result<(), String> {
        let row = CueRow::parse(json)?;
        let Some(&at) = self.of_stream.get(stream) else {
            return Err(format!("no cue stream {stream}"));
        };
        let p = self.params;
        let slot = &mut self.slots[at];
        slot.cues.retain(|cue| cue.shows(t, &p) || cue.first > t);
        let grown = slot.cues.iter_mut().rev().find(|cue| {
            cue.stream == stream
                && cue.done.is_none()
                && row.start_t.is_some()
                && cue.name == row.start_t
        });
        match grown {
            Some(cue) => {
                cue.text = row.text;
                cue.last = t;
                if row.voice.is_some() {
                    cue.voice = row.voice;
                }
                if row.done {
                    cue.done = Some(t);
                }
            }
            None => slot.cues.push(Cue {
                stream,
                name: row.start_t,
                text: row.text,
                voice: row.voice,
                first: t,
                last: t,
                done: row.done.then_some(t),
            }),
        }
        Ok(())
    }

    /// The change rows that bring each element to what it shows at `t`: one
    /// for each element whose content this instance last drew differently.
    pub fn tick(&mut self, t: f64) -> Vec<Row> {
        let p = self.params;
        let mut rows = Vec::new();
        for slot in &mut self.slots {
            let html = render(&slot.cues, t, &p);
            if slot.shown.as_deref() != Some(html.as_str()) {
                rows.push(Row::html(t, &slot.select, &html));
                slot.shown = Some(html);
            }
        }
        rows
    }
}

/// An element's content at `t`: the last `lines` cues showing then, each a
/// span, oldest first.
fn render(cues: &[Cue], t: f64, p: &CueParams) -> String {
    let showing: Vec<&Cue> = cues.iter().filter(|cue| cue.shows(t, p)).collect();
    let from = showing.len().saturating_sub(p.lines.max(1));
    showing[from..]
        .iter()
        .map(|cue| {
            let voice = cue
                .voice
                .as_deref()
                .map(|v| format!(" data-voice=\"{}\"", attribute(v)))
                .unwrap_or_default();
            format!("<span class=\"cue\"{voice}>{}</span>", cue_html(&cue.text))
        })
        .collect()
}

/// WebVTT cue text as HTML: its tags as elements, everything else as text.
pub fn cue_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    // The open elements, as the tag that closes each.
    let mut open: Vec<(&'static str, &'static str)> = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(['<', '&', '>', '"']) {
        out.push_str(&rest[..i]);
        let c = rest.as_bytes()[i];
        rest = &rest[i..];
        match c {
            // A tag starts with its name, a `/` or a timestamp's digit; any
            // other `<` is text.
            b'<' => match rest
                .find('>')
                .filter(|_| rest[1..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '/'))
            {
                Some(end) => {
                    tag(&rest[1..end], &mut open, &mut out);
                    rest = &rest[end + 1..];
                }
                None => {
                    out.push_str("&lt;");
                    rest = &rest[1..];
                }
            },
            b'&' => {
                let reference = rest[1..].find(';').filter(|&n| {
                    n > 0
                        && rest[1..1 + n]
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '#')
                });
                match reference {
                    Some(n) => {
                        out.push_str(&rest[..n + 2]);
                        rest = &rest[n + 2..];
                    }
                    None => {
                        out.push_str("&amp;");
                        rest = &rest[1..];
                    }
                }
            }
            b'>' => {
                out.push_str("&gt;");
                rest = &rest[1..];
            }
            _ => {
                out.push_str("&quot;");
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    while let Some((_, close)) = open.pop() {
        out.push_str(close);
    }
    out
}

/// One WebVTT tag, `inner` being what lies between `<` and `>`: an element
/// opened or closed, or nothing for a timestamp or a tag WebVTT does not
/// have.
fn tag(inner: &str, open: &mut Vec<(&'static str, &'static str)>, out: &mut String) {
    if let Some(name) = inner.strip_prefix('/') {
        let name = name.trim();
        if let Some(at) = open.iter().rposition(|(opened, _)| *opened == name) {
            while open.len() > at {
                let (_, close) = open.pop().expect("an open element");
                out.push_str(close);
            }
        }
        return;
    }
    let (head, annotation) = match inner.find([' ', '\t']) {
        Some(at) => (&inner[..at], inner[at..].trim()),
        None => (inner, ""),
    };
    let mut parts = head.split('.');
    let name = parts.next().unwrap_or_default();
    let classes: Vec<&str> = parts.filter(|class| !class.is_empty()).collect();
    let (element, close, name): (&str, &'static str, &'static str) = match name {
        "c" => ("span", "</span>", "c"),
        "i" => ("i", "</i>", "i"),
        "b" => ("b", "</b>", "b"),
        "u" => ("u", "</u>", "u"),
        "ruby" => ("ruby", "</ruby>", "ruby"),
        "rt" => ("rt", "</rt>", "rt"),
        "v" => ("span", "</span>", "v"),
        "lang" => ("span", "</span>", "lang"),
        _ => return,
    };
    let mut classes: Vec<String> = classes.iter().map(|class| attribute(class)).collect();
    let mut attributes = String::new();
    match name {
        "v" => {
            classes.insert(0, "voice".to_string());
            attributes.push_str(&format!(" data-voice=\"{}\"", attribute(annotation)));
        }
        "lang" if !annotation.is_empty() => {
            attributes.push_str(&format!(" lang=\"{}\"", attribute(annotation)));
        }
        _ => {}
    }
    if !classes.is_empty() {
        attributes.insert_str(0, &format!(" class=\"{}\"", classes.join(" ")));
    }
    out.push_str(&format!("<{element}{attributes}>"));
    open.push((name, close));
}

/// Text as it may stand in a double-quoted attribute.
fn attribute(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changes::Change;

    fn row(text: &str, start: f64, done: bool) -> String {
        serde_json::json!({"text": text, "start_t": start, "end_t": start + 1.0, "final": done})
            .to_string()
    }

    fn html_of(rows: &[Row]) -> Vec<(String, String)> {
        rows.iter()
            .map(|row| match &row.change {
                Change::Html(html) => (row.select.clone(), html.clone()),
                other => panic!("an html change, not {other:?}"),
            })
            .collect()
    }

    #[test]
    fn a_cue_grows_in_place_and_goes_its_hold_after_its_final() {
        let mut cues = Cues::new("#cap", 1, CueParams::default());
        cues.fold(0, 10.0, &row("he goes", 0.32, false)).unwrap();
        assert_eq!(
            html_of(&cues.tick(10.0)),
            [(
                "#cap".to_string(),
                "<span class=\"cue\">he goes</span>".to_string()
            )]
        );
        cues.fold(0, 10.5, &row("he goes for the flank", 0.32, true))
            .unwrap();
        assert_eq!(
            html_of(&cues.tick(10.5))[0].1,
            "<span class=\"cue\">he goes for the flank</span>"
        );
        assert!(cues.tick(13.4).is_empty(), "nothing changes while it holds");
        assert_eq!(
            html_of(&cues.tick(13.5))[0].1,
            "",
            "gone 3 s after its final"
        );
    }

    #[test]
    fn a_cue_that_stops_growing_goes_its_stale_after_its_last_row() {
        let mut cues = Cues::new("#cap", 1, CueParams::default());
        cues.fold(0, 1.0, &row("and that", 5.0, false)).unwrap();
        cues.tick(1.0);
        assert!(cues.tick(6.9).is_empty());
        assert_eq!(html_of(&cues.tick(7.0))[0].1, "");
    }

    #[test]
    fn a_new_utterance_shows_after_the_one_still_held_and_lines_cap_them() {
        let p = CueParams {
            lines: 1,
            ..CueParams::default()
        };
        let mut two = Cues::new("#cap", 1, CueParams::default());
        let mut one = Cues::new("#cap", 1, p);
        for cues in [&mut two, &mut one] {
            cues.fold(0, 1.0, &row("first", 0.0, true)).unwrap();
            cues.fold(0, 2.0, &row("second", 2.0, false)).unwrap();
        }
        assert_eq!(
            html_of(&two.tick(2.0))[0].1,
            "<span class=\"cue\">first</span><span class=\"cue\">second</span>"
        );
        assert_eq!(
            html_of(&one.tick(2.0))[0].1,
            "<span class=\"cue\">second</span>"
        );
    }

    #[test]
    fn each_stream_has_its_element_and_a_template_without_n_shares_one() {
        let mut cues = Cues::new("[data-slot=\"cam{n}.caption\"]", 2, CueParams::default());
        cues.fold(1, 1.0, &row("from cam two", 0.0, false)).unwrap();
        let rows = html_of(&cues.tick(1.0));
        assert_eq!(
            rows,
            [
                ("[data-slot=\"cam1.caption\"]".to_string(), String::new()),
                (
                    "[data-slot=\"cam2.caption\"]".to_string(),
                    "<span class=\"cue\">from cam two</span>".to_string()
                ),
            ]
        );
        let mut shared = Cues::new("#all", 2, CueParams::default());
        shared.fold(0, 1.0, &row("one", 0.0, false)).unwrap();
        shared.fold(1, 1.5, &row("two", 0.0, false)).unwrap();
        assert_eq!(
            html_of(&shared.tick(1.5))[0].1,
            "<span class=\"cue\">one</span><span class=\"cue\">two</span>"
        );
    }

    #[test]
    fn an_instance_that_folded_ticks_it_did_not_render_draws_the_same() {
        let rows = [(1.0, row("a", 0.0, false)), (2.0, row("a b", 0.0, true))];
        let mut every = Cues::new("#cap", 1, CueParams::default());
        let mut some = Cues::new("#cap", 1, CueParams::default());
        let mut last_every = String::new();
        for (t, json) in &rows {
            every.fold(0, *t, json).unwrap();
            if let Some((_, html)) = html_of(&every.tick(*t)).pop() {
                last_every = html;
            }
            some.fold(0, *t, json).unwrap();
        }
        assert_eq!(html_of(&some.tick(2.0))[0].1, last_every);
    }

    #[test]
    fn a_voice_on_the_row_marks_its_cue() {
        let mut cues = Cues::new("#cap", 1, CueParams::default());
        let json = r#"{"text": "and that is game", "start_t": 1, "final": true, "voice": "Mango"}"#;
        cues.fold(0, 1.0, json).unwrap();
        assert_eq!(
            html_of(&cues.tick(1.0))[0].1,
            "<span class=\"cue\" data-voice=\"Mango\">and that is game</span>"
        );
    }

    #[test]
    fn webvtt_tags_become_elements_and_everything_else_is_text() {
        assert_eq!(
            cue_html("<v.loud Mango>AND THAT IS</v> <c.red>GAME</c>"),
            "<span class=\"voice loud\" data-voice=\"Mango\">AND THAT IS</span> \
             <span class=\"red\">GAME</span>"
        );
        assert_eq!(
            cue_html("<i>so</i> <b>bold</b> <u>under</u>"),
            "<i>so</i> <b>bold</b> <u>under</u>"
        );
        assert_eq!(
            cue_html("<lang en>hi</lang>"),
            "<span lang=\"en\">hi</span>"
        );
        assert_eq!(
            cue_html("a <00:00:01.500>b"),
            "a b",
            "a timestamp is dropped"
        );
        assert_eq!(
            cue_html("<img src=x onerror=y>hi<script>z</script>"),
            "hiz",
            "a tag WebVTT does not have is dropped, its text kept"
        );
        assert_eq!(
            cue_html("rock & roll &amp; more"),
            "rock &amp; roll &amp; more"
        );
        assert_eq!(cue_html("1 < 2 > 0 \"q\""), "1 &lt; 2 &gt; 0 &quot;q&quot;");
        assert_eq!(
            cue_html("<b>open"),
            "<b>open</b>",
            "an open element is closed"
        );
        assert_eq!(cue_html("</c>stray"), "stray", "a stray closer is dropped");
        assert_eq!(
            cue_html("<v Ma\"ngo>x"),
            "<span class=\"voice\" data-voice=\"Ma&quot;ngo\">x</span>"
        );
    }

    #[test]
    fn a_row_that_is_not_a_cue_is_refused_and_changes_nothing() {
        let mut cues = Cues::new("#cap", 1, CueParams::default());
        assert!(cues.fold(0, 1.0, r##"{"select": "#x"}"##).is_err());
        assert_eq!(html_of(&cues.tick(1.0))[0].1, "");
    }
}
