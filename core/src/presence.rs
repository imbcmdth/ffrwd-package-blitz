//! Presence: the change rows a document is given at the edges of a held
//! input's feed, so it can move with an input that comes and goes.
//!
//! The host says, on every tick, what a held input's feed is: the clock time
//! its first frame shows at, and, once it can tell, the last tick it shows
//! on. From that this makes the rows `flex_input` used to send. `on` takes
//! effect when the feed's first frame shows, `off` (by default `on` again,
//! which is what a `~` toggle wants) `lead_out` seconds before the clock's
//! own picture is back, or as the feed is seen to have ended when that was
//! not foretold. A start known ahead of itself says `coming` at once and
//! again at the start, and gives the `countdown` element the whole seconds
//! left, one a second, each at its own time. A wait cut short says `coming`
//! where it was cut and nothing more.

use serde_json::Value;

use crate::changes::Row;

/// What to say about one held input's feeds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    /// The input, numbered as the document's `ffrwd:N`.
    pub input: u32,
    pub on: Vec<Row>,
    pub off: Vec<Row>,
    pub coming: Vec<Row>,
    /// The element that reads the whole seconds left until a start.
    pub countdown: Option<String>,
    /// Seconds before the clock's own picture is back that `off` is timed.
    pub lead_out: f64,
}

impl Entry {
    /// The `presence` parameter: JSON text holding one entry object or an
    /// array of them, each `{"input": N, "on": ..., "off": ..., "coming": ...,
    /// "countdown": "<selector>", "lead_out": <seconds>}`, a message being a
    /// change object or an array of them without `at`. Empty text is none.
    pub fn parse_list(text: &str) -> Result<Vec<Entry>, String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let value: Value =
            serde_json::from_str(text).map_err(|e| format!("presence is not JSON: {e}"))?;
        let items = match &value {
            Value::Array(items) => items.iter().collect(),
            other => vec![other],
        };
        let entries = items
            .into_iter()
            .map(Entry::from_value)
            .collect::<Result<Vec<_>, _>>()?;
        for (n, entry) in entries.iter().enumerate() {
            if entries[..n].iter().any(|e| e.input == entry.input) {
                return Err(format!("presence names input {} twice", entry.input));
            }
        }
        Ok(entries)
    }

    fn from_value(value: &Value) -> Result<Entry, String> {
        let Value::Object(fields) = value else {
            return Err(format!("presence: an entry is an object, and {value} is not"));
        };
        for key in fields.keys() {
            if !["input", "on", "off", "coming", "countdown", "lead_out"].contains(&key.as_str()) {
                return Err(format!(
                    "presence: an entry has \"input\", \"on\", \"off\", \"coming\", \
                     \"countdown\" and \"lead_out\", and not \"{key}\""
                ));
            }
        }
        let input = fields
            .get("input")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or("presence: an entry names its \"input\" by number, as ffrwd:N does")?;
        let message = |key: &str| -> Result<Option<Vec<Row>>, String> {
            let Some(value) = fields.get(key).filter(|v| !v.is_null()) else {
                return Ok(None);
            };
            let rows = Row::from_json_value(value).map_err(|e| format!("presence: {key}: {e}"))?;
            if rows.iter().any(|r| r.at.is_some()) {
                return Err(format!(
                    "presence: {key} gives \"at\", which the feed's own time fills in"
                ));
            }
            Ok(Some(rows))
        };
        let on = message("on")?.unwrap_or_default();
        let off = message("off")?.unwrap_or_else(|| on.clone());
        let coming = message("coming")?.unwrap_or_default();
        let countdown = match fields.get("countdown") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if !s.trim().is_empty() => Some(s.clone()),
            Some(Value::String(_)) => None,
            Some(other) => {
                return Err(format!(
                    "presence: countdown is a selector, and {other} is not"
                ));
            }
        };
        let lead_out = match fields.get("lead_out") {
            None | Some(Value::Null) => 0.0,
            Some(v) => v
                .as_f64()
                .filter(|s| (0.0..=60.0).contains(s))
                .ok_or("presence: lead_out is seconds, from 0 to 60")?,
        };
        Ok(Entry {
            input,
            on,
            off,
            coming,
            countdown,
            lead_out,
        })
    }
}

/// A held input's feed as one tick sees it, in the clock's pts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Feed {
    /// The clock time its first frame shows at.
    pub at: i64,
    /// Its source's first pts, which with `at` tells one feed from the next.
    pub first_pts: i64,
    /// The last tick it shows on, once the host can tell.
    pub ends: Option<i64>,
}

/// The clock's time base.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    pub num: i32,
    pub den: i32,
}

impl Clock {
    /// `pts` in seconds, worked out as a frame's time is.
    pub fn seconds(self, pts: i64) -> f64 {
        pts as f64 * (self.num as f64 / self.den as f64)
    }

    /// Ticks in a second, when that is a whole number of them.
    fn second(self) -> Option<i64> {
        (self.num > 0 && self.den % self.num == 0).then(|| (self.den / self.num) as i64)
    }

    /// `k` whole seconds before `pts`, in seconds.
    fn before(self, pts: i64, k: i64) -> f64 {
        match self.second() {
            Some(second) => self.seconds(pts - k * second),
            None => self.seconds(pts) - k as f64,
        }
    }

    /// The whole seconds from `now` to `at`.
    fn whole(self, now: i64, at: i64) -> i64 {
        match self.second() {
            Some(second) => (at - now).div_euclid(second),
            None => (self.seconds(at) - self.seconds(now)).floor() as i64,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Open {
    at: i64,
    first_pts: i64,
    /// When `on` takes effect.
    on_at: f64,
    off_said: bool,
}

#[derive(Clone, Debug, Default)]
struct State {
    open: Option<Open>,
    /// Rows said with a time still to come, in the order they were said.
    queue: Vec<Row>,
}

pub struct Presence {
    entries: Vec<Entry>,
    states: Vec<State>,
}

impl Presence {
    pub fn new(entries: Vec<Entry>) -> Presence {
        let states = vec![State::default(); entries.len()];
        Presence { entries, states }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// New messages, from the next edge on; a feed already open stays open.
    pub fn set(&mut self, entries: Vec<Entry>) {
        let states = entries
            .iter()
            .map(|entry| {
                self.entries
                    .iter()
                    .position(|old| old.input == entry.input)
                    .map(|n| self.states[n].clone())
                    .unwrap_or_default()
            })
            .collect();
        self.entries = entries;
        self.states = states;
    }

    /// One tick at `now` on `clock`, `step` the clock's frame length when it
    /// is known, and each entry's feed as the tick sees it, in entry order.
    /// Answers the rows due by `now`, each with its `at`.
    pub fn tick(
        &mut self,
        now: i64,
        step: Option<i64>,
        clock: Clock,
        feeds: &[Option<Feed>],
    ) -> Vec<Row> {
        let mut due = Vec::new();
        for (n, entry) in self.entries.iter().enumerate() {
            let feed = feeds.get(n).copied().flatten();
            let state = &mut self.states[n];
            edges(entry, state, now, step, clock, feed);
            let now_s = clock.seconds(now);
            let (ready, later): (Vec<Row>, Vec<Row>) = std::mem::take(&mut state.queue)
                .into_iter()
                .partition(|r| r.at.is_some_and(|at| at <= now_s));
            state.queue = later;
            due.extend(ready);
        }
        due
    }
}

fn say(rows: &[Row], at: f64, queue: &mut Vec<Row>) {
    queue.extend(rows.iter().map(|r| Row {
        at: Some(at),
        ..r.clone()
    }));
}

fn edges(
    entry: &Entry,
    state: &mut State,
    now: i64,
    step: Option<i64>,
    clock: Clock,
    feed: Option<Feed>,
) {
    let now_s = clock.seconds(now);
    if let Some(open) = state.open {
        let same = feed.is_some_and(|f| (f.at, f.first_pts) == (open.at, open.first_pts));
        if !same {
            if open.at > now {
                state.queue.clear();
                say(&entry.coming, now_s, &mut state.queue);
            } else if !open.off_said {
                say(&entry.off, now_s.max(open.on_at), &mut state.queue);
            }
            state.open = None;
        }
    }
    let Some(feed) = feed else {
        return;
    };
    if state.open.is_none() {
        let on_at = if feed.at > now {
            say(&entry.coming, now_s, &mut state.queue);
            if let Some(select) = &entry.countdown {
                let whole = clock.whole(now, feed.at);
                let text = |k: i64| Row::text(None, select, &k.to_string());
                if whole >= 1 && clock.before(feed.at, whole) > now_s {
                    say(&[text(whole)], now_s, &mut state.queue);
                }
                for k in (1..=whole).rev() {
                    say(&[text(k)], clock.before(feed.at, k), &mut state.queue);
                }
            }
            let at = clock.seconds(feed.at);
            say(&entry.on, at, &mut state.queue);
            say(&entry.coming, at, &mut state.queue);
            at
        } else {
            say(&entry.on, now_s, &mut state.queue);
            now_s
        };
        state.open = Some(Open {
            at: feed.at,
            first_pts: feed.first_pts,
            on_at,
            off_said: false,
        });
    }
    if let (Some(open), Some(ends)) = (state.open.as_mut(), feed.ends)
        && !open.off_said
    {
        let back = clock.seconds(ends + step.unwrap_or(0));
        let at = (back - entry.lead_out).max(now_s).max(open.on_at);
        say(&entry.off, at, &mut state.queue);
        open.off_said = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THIRTIETHS: Clock = Clock { num: 1, den: 30 };

    fn entry(json: &str) -> Entry {
        Entry::parse_list(json).unwrap().remove(0)
    }

    /// Each row as (pts of its `at` on the clock, selector, what it does).
    fn said(rows: &[Row]) -> Vec<(i64, String, String)> {
        rows.iter()
            .map(|r| {
                let at = r.at.unwrap();
                let pts = (0..1000)
                    .find(|p| THIRTIETHS.seconds(*p) == at)
                    .unwrap_or(-1);
                let v = r.to_value();
                let what = v
                    .get("change")
                    .or(v.get("text"))
                    .and_then(Value::as_str)
                    .unwrap()
                    .to_string();
                (pts, r.select.clone(), what)
            })
            .collect()
    }

    fn row(pts: i64, select: &str, what: &str) -> (i64, String, String) {
        (pts, select.to_string(), what.to_string())
    }

    const FLEX: &str = r##"{"input": 1, "on": {"select": "#s", "change": "~on"},
        "coming": {"select": "#s", "change": "~coming"}, "countdown": "#n", "lead_out": 0.5}"##;

    #[test]
    fn an_entry_reads_like_flex_input_s_params() {
        let e = entry(FLEX);
        assert_eq!(e.input, 1);
        assert_eq!(e.off, e.on);
        assert_eq!(e.countdown.as_deref(), Some("#n"));
        assert_eq!(e.lead_out, 0.5);
        for bad in [
            r##"{"on": {"select": "a", "text": "x"}}"##,
            r##"{"input": 1, "on": {"at": 2, "select": "a", "text": "x"}}"##,
            r##"{"input": 1, "colour": "red"}"##,
            r##"{"input": 1, "lead_out": -1}"##,
            r##"[{"input": 1}, {"input": 1}]"##,
        ] {
            assert!(Entry::parse_list(bad).is_err(), "{bad}");
        }
        assert!(Entry::parse_list(" ").unwrap().is_empty());
    }

    #[test]
    fn a_start_known_ahead_counts_down_and_the_end_leads_out() {
        let mut p = Presence::new(vec![entry(FLEX)]);
        let mut all = p.tick(10, Some(1), THIRTIETHS, &[None]);
        for now in 25..=200 {
            let feed = (now <= 160).then_some(Feed {
                at: 100,
                first_pts: 0,
                ends: (now >= 120).then_some(160),
            });
            all.extend(p.tick(now, Some(1), THIRTIETHS, &[feed]));
        }
        let back = THIRTIETHS.seconds(161) - 0.5;
        assert_eq!(all.last().unwrap().at, Some(back));
        assert_eq!(
            said(&all[..all.len() - 1]),
            [
                row(25, "#s", "~coming"),
                row(25, "#n", "2"),
                row(40, "#n", "2"),
                row(70, "#n", "1"),
                row(100, "#s", "~on"),
                row(100, "#s", "~coming"),
            ]
        );
        assert_eq!(all.len(), 7);
    }

    #[test]
    fn a_wait_cut_short_says_coming_and_nothing_more() {
        let mut p = Presence::new(vec![entry(FLEX)]);
        let mut all = Vec::new();
        for now in 25..=120 {
            let feed = (now < 60).then_some(Feed {
                at: 100,
                first_pts: 0,
                ends: None,
            });
            all.extend(p.tick(now, Some(1), THIRTIETHS, &[feed]));
        }
        assert_eq!(
            said(&all),
            [
                row(25, "#s", "~coming"),
                row(25, "#n", "2"),
                row(40, "#n", "2"),
                row(60, "#s", "~coming"),
            ]
        );
    }

    #[test]
    fn a_feed_seen_running_is_on_at_once_and_off_when_it_goes() {
        let e = entry(
            r##"{"input": 2, "on": {"select": "#s", "change": "+in"},
                "off": {"select": "#s", "change": "-in"}, "lead_out": 1}"##,
        );
        let mut p = Presence::new(vec![e]);
        let mut all = Vec::new();
        for now in 0..=60 {
            let feed = (10..30).contains(&now).then_some(Feed {
                at: 5,
                first_pts: 0,
                ends: None,
            });
            all.extend(p.tick(now, Some(1), THIRTIETHS, &[feed]));
        }
        assert_eq!(said(&all), [row(10, "#s", "+in"), row(30, "#s", "-in")]);
    }

    #[test]
    fn the_next_feed_is_a_new_edge() {
        let mut p = Presence::new(vec![entry(
            r##"{"input": 1, "on": {"select": "#s", "change": "+in"},
                "off": {"select": "#s", "change": "-in"}}"##,
        )]);
        let mut all = Vec::new();
        for now in 0..=40 {
            let feed = match now {
                5..=14 => Some(Feed {
                    at: 5,
                    first_pts: 0,
                    ends: None,
                }),
                15..=20 => Some(Feed {
                    at: 15,
                    first_pts: 900,
                    ends: Some(20),
                }),
                _ => None,
            };
            all.extend(p.tick(now, Some(1), THIRTIETHS, &[feed]));
        }
        assert_eq!(
            said(&all),
            [
                row(5, "#s", "+in"),
                row(15, "#s", "-in"),
                row(15, "#s", "+in"),
                row(21, "#s", "-in"),
            ]
        );
    }
}
