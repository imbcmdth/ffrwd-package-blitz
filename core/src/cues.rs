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
//! A cue says who is speaking when its speaker is not the one before it, as
//! live captions do: `>> MANGO: and that is game`. A cue starts a new
//! speaker when nothing shows in its element as it arrives, or when the cue
//! before it is someone else's: another stream's, or another `voice`. The
//! name is the row's `voice`, else the text of the element that names its
//! stream's speaker, read from the document as it stands at the tick. With
//! no name, an element several streams share still marks the change with
//! `>>`.
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
    /// Whether it starts a new speaker, and so says who is speaking.
    marked: bool,
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

    /// Whether `other` is the same speaker: the same voice, or the same
    /// stream when neither has one.
    fn same_speaker(&self, other: &Cue) -> bool {
        match (&self.voice, &other.voice) {
            (Some(a), Some(b)) => a == b,
            (None, None) => self.stream == other.stream,
            _ => false,
        }
    }
}

struct Slot {
    select: String,
    /// How many streams are drawn into it.
    streams: usize,
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
    /// The element naming each stream's speaker, a selector.
    speakers: Vec<Option<String>>,
    slots: Vec<Slot>,
}

impl Cues {
    /// `streams` cue streams, the n-th (from 1) drawn into the element
    /// `template` names with `{n}` replaced by n. A template without `{n}`
    /// draws every stream into one element, in the order cues arrive.
    /// `speakers`, when not empty, names the n-th stream's speaker the same
    /// way: the text of the first element it matches that has any.
    pub fn new(template: &str, speakers: &str, streams: usize, params: CueParams) -> Cues {
        let mut slots: Vec<Slot> = Vec::new();
        let mut of_stream = Vec::with_capacity(streams);
        let mut named = Vec::with_capacity(streams);
        for n in 1..=streams {
            let select = template.replace("{n}", &n.to_string());
            let at = match slots.iter().position(|slot| slot.select == select) {
                Some(at) => at,
                None => {
                    slots.push(Slot {
                        select,
                        streams: 0,
                        cues: Vec::new(),
                        shown: None,
                    });
                    slots.len() - 1
                }
            };
            slots[at].streams += 1;
            of_stream.push(at);
            named.push((!speakers.is_empty()).then(|| speakers.replace("{n}", &n.to_string())));
        }
        Cues {
            params,
            of_stream,
            speakers: named,
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
            None => {
                let mut cue = Cue {
                    stream,
                    name: row.start_t,
                    text: row.text,
                    voice: row.voice,
                    first: t,
                    last: t,
                    done: row.done.then_some(t),
                    marked: true,
                };
                let before = slot.cues.iter().rev().find(|cue| cue.shows(t, &p));
                cue.marked = before.is_none_or(|before| !before.same_speaker(&cue));
                slot.cues.push(cue);
            }
        }
        Ok(())
    }

    /// The change rows that bring each element to what it shows at `t`: one
    /// for each element whose content this instance last drew differently.
    /// `read` gives the text of the first element a selector matches that
    /// has any, in the document as it stands at `t`; it is asked only for
    /// the speakers of cues that say who is speaking.
    pub fn tick(&mut self, t: f64, read: &mut dyn FnMut(&str) -> Option<String>) -> Vec<Row> {
        let p = self.params;
        let speakers = &self.speakers;
        let mut rows = Vec::new();
        for slot in &mut self.slots {
            let mut name_of = |cue: &Cue| {
                cue.voice
                    .clone()
                    .or_else(|| speakers[cue.stream].as_deref().and_then(&mut *read))
            };
            let html = render(&slot.cues, t, &p, slot.streams > 1, &mut name_of);
            if slot.shown.as_deref() != Some(html.as_str()) {
                rows.push(Row::html(t, &slot.select, &html));
                slot.shown = Some(html);
            }
        }
        rows
    }
}

/// An element's content at `t`: the last `lines` cues showing then, each a
/// span, oldest first. A cue that starts a new speaker leads with who it is,
/// `name_of` giving the name; without one, only an element several streams
/// share marks the change.
fn render(
    cues: &[Cue],
    t: f64,
    p: &CueParams,
    shared: bool,
    name_of: &mut dyn FnMut(&Cue) -> Option<String>,
) -> String {
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
            let speaker = match cue.marked.then(|| name_of(cue)) {
                Some(Some(name)) => {
                    format!(
                        "<span class=\"speaker\">&gt;&gt; {}:</span> ",
                        attribute(&name)
                    )
                }
                Some(None) if shared => "<span class=\"speaker\">&gt;&gt;</span> ".to_string(),
                _ => String::new(),
            };
            format!(
                "<span class=\"cue\"{voice}>{speaker}{}</span>",
                cue_html(&cue.text)
            )
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

    fn none(_: &str) -> Option<String> {
        None
    }

    /// The document of a show with two casters: Mango on cam 1, and on cam 2
    /// a caster with no name, known by a handle.
    fn casters(select: &str) -> Option<String> {
        match select {
            "#caster1" => Some("Mango".to_string()),
            "#caster2" => Some("@ludwig".to_string()),
            _ => None,
        }
    }

    #[test]
    fn a_cue_grows_in_place_and_goes_its_hold_after_its_final() {
        let mut cues = Cues::new("#cap", "", 1, CueParams::default());
        cues.fold(0, 10.0, &row("he goes", 0.32, false)).unwrap();
        assert_eq!(
            html_of(&cues.tick(10.0, &mut none)),
            [(
                "#cap".to_string(),
                "<span class=\"cue\">he goes</span>".to_string()
            )]
        );
        cues.fold(0, 10.5, &row("he goes for the flank", 0.32, true))
            .unwrap();
        assert_eq!(
            html_of(&cues.tick(10.5, &mut none))[0].1,
            "<span class=\"cue\">he goes for the flank</span>"
        );
        assert!(
            cues.tick(13.4, &mut none).is_empty(),
            "nothing changes while it holds"
        );
        assert_eq!(
            html_of(&cues.tick(13.5, &mut none))[0].1,
            "",
            "gone 3 s after its final"
        );
    }

    #[test]
    fn a_cue_that_stops_growing_goes_its_stale_after_its_last_row() {
        let mut cues = Cues::new("#cap", "", 1, CueParams::default());
        cues.fold(0, 1.0, &row("and that", 5.0, false)).unwrap();
        cues.tick(1.0, &mut none);
        assert!(cues.tick(6.9, &mut none).is_empty());
        assert_eq!(html_of(&cues.tick(7.0, &mut none))[0].1, "");
    }

    #[test]
    fn a_new_utterance_shows_after_the_one_still_held_and_lines_cap_them() {
        let p = CueParams {
            lines: 1,
            ..CueParams::default()
        };
        let mut two = Cues::new("#cap", "", 1, CueParams::default());
        let mut one = Cues::new("#cap", "", 1, p);
        for cues in [&mut two, &mut one] {
            cues.fold(0, 1.0, &row("first", 0.0, true)).unwrap();
            cues.fold(0, 2.0, &row("second", 2.0, false)).unwrap();
        }
        assert_eq!(
            html_of(&two.tick(2.0, &mut none))[0].1,
            "<span class=\"cue\">first</span><span class=\"cue\">second</span>"
        );
        assert_eq!(
            html_of(&one.tick(2.0, &mut none))[0].1,
            "<span class=\"cue\">second</span>"
        );
    }

    #[test]
    fn each_stream_has_its_element_and_a_template_without_n_shares_one() {
        let mut cues = Cues::new(
            "[data-slot=\"cam{n}.caption\"]",
            "",
            2,
            CueParams::default(),
        );
        cues.fold(1, 1.0, &row("from cam two", 0.0, false)).unwrap();
        let rows = html_of(&cues.tick(1.0, &mut none));
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
        let mut shared = Cues::new("#all", "", 2, CueParams::default());
        shared.fold(0, 1.0, &row("one", 0.0, false)).unwrap();
        shared.fold(1, 1.5, &row("two", 0.0, false)).unwrap();
        assert_eq!(
            html_of(&shared.tick(1.5, &mut none))[0].1,
            "<span class=\"cue\"><span class=\"speaker\">&gt;&gt;</span> one</span>\
             <span class=\"cue\"><span class=\"speaker\">&gt;&gt;</span> two</span>",
            "with no names, a shared element marks each change of speaker"
        );
    }

    #[test]
    fn an_instance_that_folded_ticks_it_did_not_render_draws_the_same() {
        let rows = [(1.0, row("a", 0.0, false)), (2.0, row("a b", 0.0, true))];
        let mut every = Cues::new("#cap", "", 1, CueParams::default());
        let mut some = Cues::new("#cap", "", 1, CueParams::default());
        let mut last_every = String::new();
        for (t, json) in &rows {
            every.fold(0, *t, json).unwrap();
            if let Some((_, html)) = html_of(&every.tick(*t, &mut none)).pop() {
                last_every = html;
            }
            some.fold(0, *t, json).unwrap();
        }
        assert_eq!(html_of(&some.tick(2.0, &mut none))[0].1, last_every);
    }

    #[test]
    fn a_voice_on_the_row_marks_its_cue_and_names_its_speaker() {
        let mut cues = Cues::new("#cap", "#caster{n}", 1, CueParams::default());
        let json = r#"{"text": "and that is game", "start_t": 1, "final": true, "voice": "Mango"}"#;
        cues.fold(0, 1.0, json).unwrap();
        assert_eq!(
            html_of(&cues.tick(1.0, &mut |_| Some("not asked".to_string())))[0].1,
            "<span class=\"cue\" data-voice=\"Mango\">\
             <span class=\"speaker\">&gt;&gt; Mango:</span> and that is game</span>"
        );
    }

    #[test]
    fn one_display_names_the_speaker_at_each_change() {
        let p = CueParams {
            lines: 4,
            ..CueParams::default()
        };
        let mut cues = Cues::new("#captions", "#caster{n}", 2, p);
        cues.fold(0, 1.0, &row("he goes for the", 0.0, false))
            .unwrap();
        cues.fold(1, 1.5, &row("what a read", 0.0, true)).unwrap();
        cues.fold(0, 2.0, &row("he goes for the flank", 0.0, true))
            .unwrap();
        cues.fold(0, 2.5, &row("and that is game", 3.0, false))
            .unwrap();
        cues.fold(0, 3.0, &row("game five", 5.0, false)).unwrap();
        assert_eq!(
            html_of(&cues.tick(3.0, &mut casters))[0].1,
            "<span class=\"cue\"><span class=\"speaker\">&gt;&gt; Mango:</span> he goes for the flank</span>\
             <span class=\"cue\"><span class=\"speaker\">&gt;&gt; @ludwig:</span> what a read</span>\
             <span class=\"cue\"><span class=\"speaker\">&gt;&gt; Mango:</span> and that is game</span>\
             <span class=\"cue\">game five</span>",
            "a cue grown in place keeps its place, and the same speaker again is not named twice"
        );
    }

    #[test]
    fn a_speaker_who_returns_to_an_empty_display_is_named_again() {
        let mut cues = Cues::new("#captions", "#caster{n}", 2, CueParams::default());
        cues.fold(0, 1.0, &row("first", 0.0, true)).unwrap();
        cues.tick(1.0, &mut casters);
        assert_eq!(html_of(&cues.tick(4.0, &mut casters))[0].1, "");
        cues.fold(0, 5.0, &row("back again", 5.0, false)).unwrap();
        assert_eq!(
            html_of(&cues.tick(5.0, &mut casters))[0].1,
            "<span class=\"cue\"><span class=\"speaker\">&gt;&gt; Mango:</span> back again</span>"
        );
    }

    #[test]
    fn the_name_is_the_documents_at_the_tick_and_is_text() {
        let mut cues = Cues::new("#captions", "#caster{n}", 2, CueParams::default());
        cues.fold(0, 1.0, &row("hello", 0.0, false)).unwrap();
        assert_eq!(
            html_of(&cues.tick(1.0, &mut |_| None))[0].1,
            "<span class=\"cue\"><span class=\"speaker\">&gt;&gt;</span> hello</span>",
            "a speaker whose element is empty is still marked in a shared display"
        );
        assert_eq!(
            html_of(&cues.tick(1.5, &mut |_| Some("M<b>&\"".to_string())))[0].1,
            "<span class=\"cue\"><span class=\"speaker\">&gt;&gt; M&lt;b&gt;&amp;&quot;:</span> hello</span>",
            "a name set since shows on the next tick, as text"
        );
    }

    #[test]
    fn an_element_of_its_own_names_its_speaker_only_when_named() {
        let mut named = Cues::new("#cam{n}", "#caster{n}", 2, CueParams::default());
        let mut plain = Cues::new("#cam{n}", "", 2, CueParams::default());
        for cues in [&mut named, &mut plain] {
            cues.fold(1, 1.0, &row("what a read", 0.0, false)).unwrap();
        }
        assert_eq!(
            html_of(&named.tick(1.0, &mut casters))[1].1,
            "<span class=\"cue\"><span class=\"speaker\">&gt;&gt; @ludwig:</span> what a read</span>"
        );
        assert_eq!(
            html_of(&plain.tick(1.0, &mut casters))[1].1,
            "<span class=\"cue\">what a read</span>"
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
        let mut cues = Cues::new("#cap", "", 1, CueParams::default());
        assert!(cues.fold(0, 1.0, r##"{"select": "#x"}"##).is_err());
        assert_eq!(html_of(&cues.tick(1.0, &mut none))[0].1, "");
    }
}
