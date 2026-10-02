//! Presence: the change rows a document is given at the edges of a held
//! input's feed, so it can move with an input that comes and goes.
//!
//! The host records each feed: the tick its start was fixed on, the clock
//! time its first frame shows at and, once it can tell, the last tick it
//! shows on; and it lists, on each call, the feeds that ended since the
//! instance's previous one. From that record alone this makes the rows
//! `flex_input` used to send, so they come out the same on any worker. `on`
//! takes effect when the feed's first frame shows, `off` (by default `on`
//! again, which is what a `~` toggle wants) `lead_out` seconds before the
//! clock's own picture is back, or on that picture when the end was not
//! told ahead. A start known ahead of itself says `coming` on the tick it
//! was fixed and again at the start, and gives the `countdown` element the
//! whole seconds left, one a second, each at its own time. A wait cut short
//! says `coming` where it was cut and nothing more.

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
            return Err(format!(
                "presence: an entry is an object, and {value} is not"
            ));
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

/// A held input's feed as the host records it, in the clock's pts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Feed {
    /// The tick its start was fixed on.
    pub known: i64,
    /// The clock time its first frame shows at.
    pub at: i64,
    /// Its source's first pts, which with `known` and `at` tells one feed
    /// from the next.
    pub first_pts: i64,
    /// The last tick it shows on, once the host can tell.
    pub ends: Option<i64>,
}

impl Feed {
    fn key(&self) -> (i64, i64, i64) {
        (self.known, self.at, self.first_pts)
    }
}

/// What one tick says of one held input: its current feed, and the feeds
/// that ended since the instance's previous call, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    pub current: Option<Feed>,
    pub ended: Vec<Feed>,
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

type Key = (i64, i64, i64);

#[derive(Clone, Copy, Debug)]
struct Open {
    key: Key,
    /// When `on` takes effect.
    on_at: f64,
}

#[derive(Clone, Debug)]
struct Said {
    row: Row,
    feed: Key,
}

#[derive(Clone, Debug, Default)]
struct State {
    open: Vec<Open>,
    /// Feeds whose end was said while they were current, until the host
    /// lists them as ended.
    done: Vec<Key>,
    /// Rows said with a time still to come, in the order they were said.
    queue: Vec<Said>,
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
    /// is known, and what the tick says of each entry's input, in entry
    /// order. Answers the rows due by `now`, each with its `at`.
    ///
    /// Every row's time comes from the feed's record alone, so an instance
    /// handed any subset of the ticks says the same rows at the same times.
    pub fn tick(&mut self, now: i64, step: Option<i64>, clock: Clock, seen: &[Seen]) -> Vec<Row> {
        let mut due = Vec::new();
        let now_s = clock.seconds(now);
        for (n, entry) in self.entries.iter().enumerate() {
            let state = &mut self.states[n];
            if let Some(seen) = seen.get(n) {
                for feed in &seen.ended {
                    learn(entry, state, step, clock, feed, true);
                }
                if let Some(feed) = &seen.current {
                    learn(entry, state, step, clock, feed, false);
                }
            }
            let (ready, later): (Vec<Said>, Vec<Said>) = std::mem::take(&mut state.queue)
                .into_iter()
                .partition(|s| s.row.at.is_some_and(|at| at <= now_s));
            state.queue = later;
            due.extend(ready.into_iter().map(|s| s.row));
        }
        due
    }
}

fn say(rows: &[Row], at: f64, feed: Key, queue: &mut Vec<Said>) {
    queue.extend(rows.iter().map(|r| Said {
        row: Row {
            at: Some(at),
            ..r.clone()
        },
        feed,
    }));
}

/// What a feed's record says: its start once, and its end once it has one.
/// `ended` is a feed the host lists as gone, whose end this instance may
/// not have been told ahead of it.
fn learn(
    entry: &Entry,
    state: &mut State,
    step: Option<i64>,
    clock: Clock,
    feed: &Feed,
    ended: bool,
) {
    let key = feed.key();
    if let Some(n) = state.done.iter().position(|k| *k == key) {
        if ended {
            state.done.remove(n);
        }
        return;
    }
    let open = match state.open.iter().find(|o| o.key == key) {
        Some(open) => *open,
        None => {
            let open = start(entry, &mut state.queue, clock, feed);
            state.open.push(open);
            open
        }
    };
    let Some(ends) = feed.ends else {
        return;
    };
    let back = clock.seconds(ends + step.unwrap_or(0));
    if feed.at > ends {
        state
            .queue
            .retain(|s| s.feed != key || s.row.at.is_some_and(|at| at < back));
        say(&entry.coming, back, key, &mut state.queue);
    } else {
        let off = if ended { back } else { back - entry.lead_out };
        say(&entry.off, off.max(open.on_at), key, &mut state.queue);
    }
    state.open.retain(|o| o.key != key);
    if !ended {
        state.done.push(key);
    }
}

fn start(entry: &Entry, queue: &mut Vec<Said>, clock: Clock, feed: &Feed) -> Open {
    let key = feed.key();
    let known = clock.seconds(feed.known);
    if feed.at <= feed.known {
        say(&entry.on, known, key, queue);
        return Open { key, on_at: known };
    }
    say(&entry.coming, known, key, queue);
    if let Some(select) = &entry.countdown {
        let whole = clock.whole(feed.known, feed.at);
        let text = |k: i64| Row::text(None, select, &k.to_string());
        if whole >= 1 && clock.before(feed.at, whole) > known {
            say(&[text(whole)], known, key, queue);
        }
        for k in (1..=whole).rev() {
            say(&[text(k)], clock.before(feed.at, k), key, queue);
        }
    }
    let at = clock.seconds(feed.at);
    say(&entry.on, at, key, queue);
    say(&entry.coming, at, key, queue);
    Open { key, on_at: at }
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

    /// The host for one input over `ticks`: `feed(t)` is the feed current on
    /// tick `t`, and a feed that stops being current ends with `ends` at the
    /// last tick it was. Answers what an instance handed only `mine` sees on
    /// each of them.
    fn host(
        ticks: std::ops::RangeInclusive<i64>,
        feed: impl Fn(i64) -> Option<Feed>,
        mine: impl Fn(i64) -> bool,
    ) -> Vec<(i64, Seen)> {
        let mut ended: Vec<(i64, Feed)> = Vec::new();
        let mut last: Option<(i64, Feed)> = None;
        let mut told = i64::MIN;
        let mut seen = Vec::new();
        for t in ticks {
            let current = feed(t);
            if let Some((when, was)) = last
                && current.map(|f| f.key()) != Some(was.key())
            {
                ended.push((
                    t,
                    Feed {
                        ends: Some(when),
                        ..was
                    },
                ));
            }
            last = current.map(|f| (t, f));
            if mine(t) {
                let gone = ended
                    .iter()
                    .filter(|(at, _)| *at > told && *at <= t)
                    .map(|(_, f)| *f)
                    .collect();
                seen.push((
                    t,
                    Seen {
                        current,
                        ended: gone,
                    },
                ));
                told = t;
            }
        }
        seen
    }

    fn run(p: &mut Presence, seen: &[(i64, Seen)]) -> Vec<Row> {
        let mut all = Vec::new();
        for (t, s) in seen {
            all.extend(p.tick(*t, Some(1), THIRTIETHS, std::slice::from_ref(s)));
        }
        all
    }

    fn timed(known: i64, at: i64, first_pts: i64, ends: Option<i64>) -> Feed {
        Feed {
            known,
            at,
            first_pts,
            ends,
        }
    }

    #[test]
    fn a_start_known_ahead_counts_down_from_known_and_the_end_leads_out() {
        let mut p = Presence::new(vec![entry(FLEX)]);
        let seen = host(
            10..=200,
            |t| {
                (25..=160)
                    .contains(&t)
                    .then(|| timed(25, 100, 0, (t >= 120).then_some(160)))
            },
            |_| true,
        );
        let all = run(&mut p, &seen);
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
        let seen = host(
            25..=120,
            |t| (t < 60).then(|| timed(25, 100, 0, None)),
            |_| true,
        );
        assert_eq!(
            said(&run(&mut p, &seen)),
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
        let seen = host(
            0..=60,
            |t| (10..30).contains(&t).then(|| timed(10, 5, 0, None)),
            |_| true,
        );
        assert_eq!(
            said(&run(&mut p, &seen)),
            [row(10, "#s", "+in"), row(30, "#s", "-in")]
        );
    }

    #[test]
    fn the_next_feed_is_a_new_edge() {
        let mut p = Presence::new(vec![entry(
            r##"{"input": 1, "on": {"select": "#s", "change": "+in"},
                "off": {"select": "#s", "change": "-in"}}"##,
        )]);
        let seen = host(
            0..=40,
            |t| match t {
                5..=14 => Some(timed(5, 5, 0, None)),
                15..=20 => Some(timed(15, 15, 900, Some(20))),
                _ => None,
            },
            |_| true,
        );
        assert_eq!(
            said(&run(&mut p, &seen)),
            [
                row(5, "#s", "+in"),
                row(15, "#s", "-in"),
                row(15, "#s", "+in"),
                row(21, "#s", "-in"),
            ]
        );
    }

    type Told = (i64, String, String);

    /// The rows an instance has said by each tick it was handed, as
    /// (tick, pts of each row due by then).
    fn due_by(seen: &[(i64, Seen)]) -> Vec<(i64, Vec<Told>)> {
        let mut p = Presence::new(vec![entry(FLEX)]);
        let mut so_far = Vec::new();
        seen.iter()
            .map(|(t, s)| {
                so_far.extend(p.tick(*t, Some(1), THIRTIETHS, std::slice::from_ref(s)));
                let mut rows = said(&so_far);
                rows.sort();
                (*t, rows)
            })
            .collect()
    }

    #[test]
    fn every_worker_says_the_same_rows_at_the_same_times() {
        let feed = |t: i64| match t {
            25..=160 => Some(timed(25, 100, 0, (t >= 120).then_some(160))),
            200..=229 => Some(timed(200, 260, 7, None)),
            300..=340 => Some(timed(300, 300, 9, None)),
            _ => None,
        };
        let one = due_by(&host(0..=400, feed, |_| true));
        let by_tick: std::collections::HashMap<i64, Vec<Told>> = one.into_iter().collect();
        for workers in [2, 4, 7] {
            for k in 0..workers {
                for (t, rows) in due_by(&host(0..=400, feed, |t| t % workers == k)) {
                    assert_eq!(rows, by_tick[&t], "worker {k} of {workers} at tick {t}");
                }
            }
        }
    }
}
