//! Counts of in-game shader translations, shown by the on-screen progress panel.
//!
//! The title asks the runtime for a translation and waits for the host worker.
//! Each wait is one unit of work; cache hits return before they are counted.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
use std::time::Instant;

static START: LazyLock<Instant> = LazyLock::new(Instant::now);
static IN_FLIGHT: AtomicU32 = AtomicU32::new(0);
static TRANSLATED: AtomicU32 = AtomicU32::new(0);
static UNSUPPORTED: AtomicU32 = AtomicU32::new(0);
static TIMED_OUT: AtomicU32 = AtomicU32::new(0);
/// Milliseconds after `START` of the last change, plus one (zero means none yet).
static LAST_CHANGE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub in_flight: u32,
    pub translated: u32,
    pub unsupported: u32,
    pub timed_out: u32,
    /// Milliseconds since anything last changed, or `None` before any activity.
    pub idle_ms: Option<u64>,
}

impl Snapshot {
    pub(crate) fn finished(&self) -> u32 {
        self.translated + self.unsupported + self.timed_out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Translated,
    Unsupported,
    TimedOut,
}

fn now_ms() -> u64 {
    START.elapsed().as_millis() as u64
}

fn touch() {
    LAST_CHANGE.store(now_ms() + 1, Relaxed);
}

/// One translation the title is waiting on. Dropping it ends the wait.
pub(crate) struct Waiting {
    outcome: Option<Outcome>,
}

pub(crate) fn begin() -> Waiting {
    IN_FLIGHT.fetch_add(1, Relaxed);
    touch();
    Waiting { outcome: None }
}

impl Waiting {
    pub(crate) fn finish(mut self, outcome: Outcome) {
        self.outcome = Some(outcome);
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Relaxed);
        match self.outcome {
            Some(Outcome::Translated) => TRANSLATED.fetch_add(1, Relaxed),
            Some(Outcome::Unsupported) => UNSUPPORTED.fetch_add(1, Relaxed),
            Some(Outcome::TimedOut) => TIMED_OUT.fetch_add(1, Relaxed),
            None => 0,
        };
        touch();
    }
}

pub(crate) fn snapshot() -> Snapshot {
    let last = LAST_CHANGE.load(Relaxed);
    Snapshot {
        in_flight: IN_FLIGHT.load(Relaxed),
        translated: TRANSLATED.load(Relaxed),
        unsupported: UNSUPPORTED.load(Relaxed),
        timed_out: TIMED_OUT.load(Relaxed),
        idle_ms: (last != 0).then(|| now_ms().saturating_sub(last - 1)),
    }
}

/// `TUXBOX_PROGRESS_OVERLAY`: unset or `auto` shows the panel while work is in
/// flight and briefly afterwards, `always` keeps it on, `0` or `off` hides it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Auto,
    Always,
    Off,
}

impl Mode {
    pub(crate) fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("0" | "off" | "false" | "no") => Self::Off,
            Some("always" | "on" | "1") => Self::Always,
            _ => Self::Auto,
        }
    }

    pub(crate) fn from_env() -> Self {
        Self::parse(std::env::var("TUXBOX_PROGRESS_OVERLAY").ok().as_deref())
    }
}

/// How long the panel stays up after the last translation finishes.
pub(crate) const LINGER_MS: u64 = 4000;

pub(crate) fn visible(mode: Mode, snapshot: &Snapshot) -> bool {
    match mode {
        Mode::Off => false,
        Mode::Always => true,
        Mode::Auto => {
            snapshot.in_flight > 0
                || snapshot
                    .idle_ms
                    .is_some_and(|idle| idle < LINGER_MS && snapshot.finished() > 0)
        }
    }
}

/// Digits in groups of three: 12345 becomes "12,345".
pub(crate) fn grouped(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// The panel's three lines of text.
pub(crate) fn lines(snapshot: &Snapshot) -> [String; 3] {
    let state = if snapshot.in_flight > 0 {
        format!("translating {}", grouped(snapshot.in_flight))
    } else {
        "idle".to_string()
    };
    [
        "TuxBox  shader translation".to_string(),
        format!("{state}  |  {} done", grouped(snapshot.translated)),
        format!(
            "{} unsupported  |  {} timed out",
            grouped(snapshot.unsupported),
            grouped(snapshot.timed_out)
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(in_flight: u32, translated: u32, idle: Option<u64>) -> Snapshot {
        Snapshot { in_flight, translated, idle_ms: idle, ..Snapshot::default() }
    }

    #[test]
    fn mode_parses_the_documented_values() {
        assert_eq!(Mode::parse(None), Mode::Auto);
        assert_eq!(Mode::parse(Some("auto")), Mode::Auto);
        assert_eq!(Mode::parse(Some("always")), Mode::Always);
        assert_eq!(Mode::parse(Some("0")), Mode::Off);
        assert_eq!(Mode::parse(Some("off")), Mode::Off);
        assert_eq!(Mode::parse(Some("nonsense")), Mode::Auto);
    }

    #[test]
    fn auto_shows_while_working_and_lingers_then_hides() {
        assert!(!visible(Mode::Auto, &Snapshot::default()));
        assert!(visible(Mode::Auto, &snap(2, 0, Some(10))));
        assert!(visible(Mode::Auto, &snap(0, 5, Some(LINGER_MS - 1))));
        assert!(!visible(Mode::Auto, &snap(0, 5, Some(LINGER_MS))));
        assert!(!visible(Mode::Auto, &snap(0, 0, Some(10))));
    }

    #[test]
    fn always_and_off_ignore_activity() {
        assert!(visible(Mode::Always, &Snapshot::default()));
        assert!(!visible(Mode::Off, &snap(9, 9, Some(0))));
    }

    #[test]
    fn numbers_are_grouped_in_threes() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(1234567), "1,234,567");
    }

    #[test]
    fn lines_describe_the_counts() {
        let text = lines(&Snapshot { in_flight: 3, translated: 1284, unsupported: 12, timed_out: 1, idle_ms: Some(0) });
        assert_eq!(text[1], "translating 3  |  1,284 done");
        assert_eq!(text[2], "12 unsupported  |  1 timed out");
        assert!(lines(&Snapshot::default())[1].starts_with("idle"));
    }
}
