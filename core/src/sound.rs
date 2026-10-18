//! The sound of the held inputs. Each element that shows an input carries
//! a level, `--ffrwd-volume` (1 when unset), and may carry a lower level,
//! `--ffrwd-volume-duck`, that it drops to while an element of another
//! input is playing. A tick's audio is each element's input's samples
//! scaled by its gain, summed and clamped. A gain that changes from one
//! tick to the next is ramped across the second tick's samples.
//!
//! Each element also has a squelch: its input's level on a tick (the RMS of
//! the samples the input hands that tick, before any volume) opens the
//! element's gate when it is above `--ffrwd-squelch`, and the gate stays
//! open for `--ffrwd-squelch-release` seconds after the level was last
//! above it. A closed gate silences the element. An element plays, and
//! ducks the others, while its gate is open, its volume is above 0 and its
//! input carries sound.

use std::collections::VecDeque;

/// The output's sample rate and channels: 48 kHz stereo, interleaved f32.
pub const RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;

/// The custom properties an element's levels are read from, without the
/// leading `--`.
pub const VOLUME: &str = "ffrwd-volume";
pub const DUCK: &str = "ffrwd-volume-duck";
pub const SQUELCH: &str = "ffrwd-squelch";
pub const SQUELCH_RELEASE: &str = "ffrwd-squelch-release";

/// The level, in dBFS, a gate opens above by default: near silence.
pub const THRESHOLD: f32 = -50.0;
/// Seconds a gate stays open by default after its level was last above
/// the threshold.
pub const RELEASE: f32 = 0.5;
/// The longest release honoured, and how far back the levels are kept.
pub const MAX_RELEASE: f32 = 30.0;

/// One element that shows input `input`, with its levels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Voice {
    pub input: u32,
    /// `--ffrwd-volume`: 1 unset.
    pub volume: f32,
    /// `--ffrwd-volume-duck`: none unset, which never ducks.
    pub duck: Option<f32>,
    /// `--ffrwd-squelch`, dBFS; none (`none`) for no gate, always open.
    pub squelch: Option<f32>,
    /// `--ffrwd-squelch-release`, seconds.
    pub release: f32,
}

impl Voice {
    /// An element of `input` at `volume`, with no duck and the default
    /// squelch.
    pub fn new(input: u32, volume: f32) -> Voice {
        Voice {
            input,
            volume,
            duck: None,
            squelch: Some(THRESHOLD),
            release: RELEASE,
        }
    }
}

/// A number as CSS text; None for anything else.
pub fn number(css: &str) -> Option<f32> {
    let v: f32 = css.trim().parse().ok()?;
    v.is_finite().then_some(v)
}

/// A squelch as CSS text: `none` for no gate, else a number of dBFS.
/// Anything else is the default.
pub fn squelch(css: &str) -> Option<f32> {
    if css.trim().eq_ignore_ascii_case("none") {
        return None;
    }
    Some(number(css).unwrap_or(THRESHOLD))
}

/// A level as CSS text: a number, at least 0.
pub fn level(css: &str) -> Option<f32> {
    number(css).map(|v| v.max(0.0))
}

/// The RMS of samples, in dBFS: 0 for a square wave at full scale, minus
/// infinity for silence or no samples.
pub fn rms_dbfs(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return f32::NEG_INFINITY;
    }
    let power = samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / samples.len() as f64;
    (10.0 * power.log10()) as f32
}

/// Each input's level on the recent ticks, for the gates.
#[derive(Clone, Debug, Default)]
pub struct Loudness {
    /// Per input, oldest first: a tick's time, seconds, and its level, dBFS.
    ticks: Vec<VecDeque<(f64, f32)>>,
}

impl Loudness {
    /// Input `input`'s level on the tick at `t`.
    pub fn push(&mut self, input: u32, t: f64, dbfs: f32) {
        let k = input as usize;
        if self.ticks.len() <= k {
            self.ticks.resize_with(k + 1, VecDeque::new);
        }
        let ticks = &mut self.ticks[k];
        ticks.push_back((t, dbfs));
        while ticks
            .front()
            .is_some_and(|(at, _)| *at < t - f64::from(MAX_RELEASE))
        {
            ticks.pop_front();
        }
    }

    /// Whether input `input` was above `threshold` on a tick within
    /// `release` seconds before `t`, or at `t`.
    pub fn open(&self, input: u32, t: f64, threshold: f32, release: f32) -> bool {
        // A microsecond's slack, so a release of a whole number of ticks
        // reaches the tick it ends on.
        let since = t - f64::from(release.clamp(0.0, MAX_RELEASE)) - 1e-6;
        self.ticks.get(input as usize).is_some_and(|ticks| {
            ticks
                .iter()
                .rev()
                .take_while(|(at, _)| *at >= since)
                .any(|(at, dbfs)| *at <= t && *dbfs > threshold)
        })
    }
}

/// Each input's gain, `inputs` long: the sum over the elements showing it
/// of each one's gain. `sounding` says which inputs are present and carry
/// sound this tick, and `open` whether a level above a threshold was heard
/// on an input within a release. An element whose gate is closed is at 0;
/// one with no squelch has a gate always open.
/// One with a duck level is at that level while an element of another
/// input plays: its gate open, at a volume above 0, its input sounding.
/// Whether an element plays never depends on its own duck, so two ducking
/// elements duck each other.
pub fn gains(
    voices: &[Voice],
    sounding: &dyn Fn(u32) -> bool,
    open: &dyn Fn(u32, f32, f32) -> bool,
    inputs: usize,
) -> Vec<f32> {
    let gate = |v: &Voice| v.squelch.is_none_or(|th| open(v.input, th, v.release));
    let playing = |v: &Voice| v.volume > 0.0 && sounding(v.input) && gate(v);
    let mut gains = vec![0.0; inputs];
    for voice in voices {
        let Some(gain) = gains.get_mut(voice.input as usize) else {
            continue;
        };
        if !gate(voice) {
            continue;
        }
        let ducked = voice.duck.filter(|_| {
            voices
                .iter()
                .any(|other| other.input != voice.input && playing(other))
        });
        *gain += ducked.unwrap_or(voice.volume);
    }
    gains
}

/// A time as an exact fraction of a second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Time {
    pub num: i128,
    pub den: i128,
}

impl Time {
    pub const ZERO: Time = Time { num: 0, den: 1 };

    /// `pts` in a time base of `num / den` seconds.
    pub fn of(pts: i64, num: i32, den: i32) -> Time {
        Time {
            num: pts as i128 * num as i128,
            den: (den as i128).max(1),
        }
    }

    fn minus(self, other: Time) -> Time {
        Time {
            num: self.num * other.den - other.num * self.den,
            den: self.den * other.den,
        }
    }

    /// The whole units of `1 / rate` in it, rounded down.
    pub fn floor_at(self, rate: i128) -> i128 {
        (self.num * rate).div_euclid(self.den)
    }
}

/// The output samples a tick from `start` to `end` holds, as the first
/// sample and a count: each time rounded down to a whole sample, so ticks
/// that meet share no sample and leave none out.
pub fn window(start: Time, end: Time) -> (i64, usize) {
    let s0 = start.floor_at(RATE as i128);
    let s1 = end.floor_at(RATE as i128).max(s0);
    (s0 as i64, (s1 - s0) as usize)
}

/// How an input's source time maps onto the clock: the clock time a source
/// time stands at, less that source time (the host's offset for the feed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offset(pub Time);

impl Offset {
    /// The offset that puts `first` at `at`.
    pub fn between(first: Time, at: Time) -> Offset {
        Offset(at.minus(first))
    }
}

/// Where a run of an input's samples lands in a tick's window, as the host
/// cut it. The host hands the samples from the tick's start less the
/// offset, rounded down to the source's time base, to its end less the
/// offset, so a run that starts on time starts on the window's first
/// sample, and one that starts late (a source joining mid-tick) later.
///
/// `base` is the source's time base, `run_pts` the run's first sample in
/// it and `count` its samples. Returns the window index of the run's first
/// sample and how many samples of the window it fills. Where the host's cut
/// is a sample longer than the window the last is left out, and where it is
/// a sample shorter the run's last is held for one more, so whole runs play
/// across the ticks' edges with no gap and no sample twice.
#[allow(clippy::too_many_arguments)]
pub fn place(
    start: Time,
    end: Time,
    offset: Offset,
    base: (i32, i32),
    run_pts: i64,
    count: usize,
    len: usize,
) -> Option<(usize, usize)> {
    let (num, den) = (base.0 as i128, (base.1 as i128).max(1));
    let rate = RATE as i128;
    // The source time the clock's `t` stands at, in `base`, rounded down.
    let source = |t: Time| {
        let t = t.minus(offset.0);
        (t.num * den).div_euclid(t.den * num.max(1))
    };
    // The sample a time in `base` falls on, rounded to the nearest.
    let sample = |pts: i128| (2 * pts * num * rate + den).div_euclid(2 * den);
    let w0 = sample(source(start));
    let w1 = sample(source(end));
    let first = sample(run_pts as i128).max(w0);
    let at = usize::try_from(first - w0).ok()?;
    if at >= len {
        return None;
    }
    let whole = first + count as i128 >= w1;
    let fills = if whole { len - at } else { count.min(len - at) };
    Some((at, fills))
}

/// One input's contribution to a tick: its samples (interleaved stereo),
/// where they start in the window and how much of it they fill, and its
/// gain at the previous tick and at this one.
pub struct Part<'a> {
    pub samples: &'a [f32],
    pub at: usize,
    pub fills: usize,
    pub from: f32,
    pub to: f32,
}

/// A tick of `len` samples: each part scaled by its gain, ramped from
/// `from` at the tick's start to `to` at its last sample, summed and
/// clamped to the sample range. Silence where no part sounds.
pub fn mix(len: usize, parts: &[Part]) -> Vec<f32> {
    let mut out = vec![0.0f32; len * CHANNELS];
    for part in parts {
        let frames = part.samples.len() / CHANNELS;
        if frames == 0 || (part.from == 0.0 && part.to == 0.0) {
            continue;
        }
        for i in part.at..(part.at + part.fills).min(len) {
            let gain = part.from + (part.to - part.from) * (i + 1) as f32 / len as f32;
            let j = (i - part.at).min(frames - 1);
            for c in 0..CHANNELS {
                out[i * CHANNELS + c] += gain * part.samples[j * CHANNELS + c];
            }
        }
    }
    for s in &mut out {
        *s = s.clamp(-1.0, 1.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(input: u32, volume: f32, duck: Option<f32>) -> Voice {
        Voice {
            duck,
            ..Voice::new(input, volume)
        }
    }

    #[test]
    fn a_level_is_a_number_at_least_zero() {
        assert_eq!(level(" 0.5 "), Some(0.5));
        assert_eq!(level("2"), Some(2.0));
        assert_eq!(level("-1"), Some(0.0));
        assert_eq!(number("-40"), Some(-40.0));
        assert_eq!(level("loud"), None);
        assert_eq!(level("inf"), None);
        assert_eq!(squelch(" none "), None);
        assert_eq!(squelch("-30"), Some(-30.0));
        assert_eq!(squelch("quiet"), Some(THRESHOLD));
    }

    #[test]
    fn the_rms_of_full_scale_is_0_dbfs() {
        assert_eq!(rms_dbfs(&[1.0, -1.0, 1.0, -1.0]), 0.0);
        assert!((rms_dbfs(&[0.01; 8]) + 40.0).abs() < 1e-4);
        assert_eq!(rms_dbfs(&[0.0; 8]), f32::NEG_INFINITY);
        assert_eq!(rms_dbfs(&[]), f32::NEG_INFINITY);
    }

    #[test]
    fn two_elements_of_one_input_add_and_a_duck_follows_another_input() {
        let all = |_: u32| true;
        let open = |_: u32, _: f32, _: f32| true;
        let voices = [
            voice(1, 1.0, None),
            voice(1, 0.25, None),
            voice(2, 0.5, None),
        ];
        assert_eq!(gains(&voices, &all, &open, 3), vec![0.0, 1.25, 0.5]);

        let main = voice(1, 1.0, Some(0.5));
        assert_eq!(gains(&[main, voice(2, 1.0, None)], &all, &open, 3)[1], 0.5);
        assert_eq!(
            gains(&[main, voice(2, 0.0, None)], &all, &open, 3)[1],
            1.0,
            "a cam at 0 ducks nothing"
        );
        let only_main = |input: u32| input == 1;
        assert_eq!(
            gains(&[main, voice(2, 1.0, None)], &only_main, &open, 3)[1],
            1.0,
            "a cam with no sound ducks nothing"
        );
        let main_alone = |input: u32, _: f32, _: f32| input == 1;
        assert_eq!(
            gains(&[main, voice(2, 1.0, None)], &all, &main_alone, 3),
            vec![0.0, 1.0, 0.0],
            "a cam below its squelch is silent and ducks nothing"
        );
        assert_eq!(
            gains(&[main, voice(1, 1.0, None)], &all, &open, 3)[1],
            2.0,
            "an element of the same input ducks nothing"
        );
        let game = Voice {
            squelch: None,
            ..voice(2, 1.0, None)
        };
        assert_eq!(
            gains(&[main, game], &all, &main_alone, 3),
            vec![0.0, 0.5, 1.0],
            "a track with no squelch is heard, quiet or not, and ducks on presence"
        );
        let both = gains(
            &[voice(1, 1.0, Some(0.5)), voice(2, 0.8, Some(0.2))],
            &all,
            &open,
            3,
        );
        assert_eq!(
            both,
            vec![0.0, 0.5, 0.2],
            "two ducking elements duck each other"
        );
    }

    #[test]
    fn a_gate_stays_open_for_its_release() {
        let mut levels = Loudness::default();
        let tick = |k: i64| k as f64 / 60.0;
        for k in 0..10 {
            levels.push(2, tick(k), -20.0);
        }
        for k in 10..100 {
            levels.push(2, tick(k), f32::NEG_INFINITY);
        }
        assert!(levels.open(2, tick(9), THRESHOLD, RELEASE));
        assert!(levels.open(2, tick(39), THRESHOLD, RELEASE), "0.5 s after");
        assert!(!levels.open(2, tick(40), THRESHOLD, RELEASE), "past 0.5 s");
        assert!(
            !levels.open(2, tick(9), -10.0, RELEASE),
            "under a raised squelch"
        );
        assert!(!levels.open(2, tick(12), THRESHOLD, 0.0), "no release");
        assert!(
            !levels.open(3, tick(5), THRESHOLD, RELEASE),
            "an input never heard"
        );
    }

    const KHZ: (i32, i32) = (1, RATE as i32);

    fn sixtieth(k: i64) -> Time {
        Time::of(k, 1, 60)
    }

    #[test]
    fn ticks_at_60_fps_cut_800_samples_each_and_meet() {
        let mut next = 0;
        for k in 0..120 {
            let (start, len) = window(sixtieth(k), sixtieth(k + 1));
            assert_eq!((start, len), (k * 800, 800));
            assert_eq!(start, next);
            next = start + len as i64;
        }
        // At 59.94 the ticks are 800 or 801 samples, and still meet.
        let tick = |k: i64| Time::of(k * 1001, 1, 60_000);
        let mut next = window(tick(0), tick(1)).0;
        let mut total = 0;
        for k in 0..60_000 {
            let (start, len) = window(tick(k), tick(k + 1));
            assert!(len == 800 || len == 801);
            assert_eq!(start, next);
            next = start + len as i64;
            total += len;
        }
        assert_eq!(total, 48_048_000, "1001 s of sound");
    }

    #[test]
    fn a_run_is_placed_where_its_offset_puts_it() {
        // A feed whose first sample, at source 0, stands at clock 2.0 s: the
        // host cuts the tick at 2.5 s from source 0.5 s, so the run's first
        // sample is the window's.
        let offset = Offset::between(Time::ZERO, sixtieth(120));
        let (start, end) = (sixtieth(150), sixtieth(151));
        assert_eq!(
            place(start, end, offset, KHZ, 24_000, 800, 800),
            Some((0, 800))
        );
        // Starting 100 samples late, it fills from 100 on.
        assert_eq!(
            place(start, end, offset, KHZ, 24_100, 700, 800),
            Some((100, 700))
        );
        // A source that stopped short leaves the rest silent.
        assert_eq!(
            place(start, end, offset, KHZ, 24_000, 300, 800),
            Some((0, 300))
        );
        // An offset of a third of a sample: the host's cut starts a sample
        // early, which lands on the window's first.
        let third = Offset(Time {
            num: 2 * 3 * RATE as i128 + 1,
            den: 3 * RATE as i128,
        });
        assert_eq!(
            place(start, end, third, KHZ, 23_999, 800, 800),
            Some((0, 800))
        );
    }

    #[test]
    fn the_mix_ramps_sums_and_clamps() {
        let ones = vec![1.0f32; 8 * CHANNELS];
        let half = vec![0.5f32; 8 * CHANNELS];
        let out = mix(
            8,
            &[
                Part {
                    samples: &ones,
                    at: 0,
                    fills: 8,
                    from: 1.0,
                    to: 0.0,
                },
                Part {
                    samples: &half,
                    at: 4,
                    fills: 4,
                    from: 4.0,
                    to: 4.0,
                },
            ],
        );
        assert_eq!(out[0], 0.875, "a ramp starts one step in");
        assert_eq!(out[3 * CHANNELS], 0.5);
        assert_eq!(out[4 * CHANNELS], 1.0, "1.0 + 0.375 + 2.0, clamped");
        assert_eq!(out[7 * CHANNELS + 1], 1.0);
        let held = mix(
            4,
            &[Part {
                samples: &[0.1, 0.1, 0.2, 0.2, 0.3, 0.3],
                at: 0,
                fills: 4,
                from: 1.0,
                to: 1.0,
            }],
        );
        assert_eq!(held, vec![0.1, 0.1, 0.2, 0.2, 0.3, 0.3, 0.3, 0.3]);
    }
}
