//! Memory-test history and time estimates.
//!
//! Each test is recorded per device (started, finished, outcome), so you can see
//! how many passes ran, which Synology support usually asks for. Progress
//! samples give an estimate of the time remaining.

use crate::protocol::NasInfo;
use crate::store;
use serde::{Deserialize, Serialize};

pub const FILE: &str = "memtests.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    NoErrors,
    Errors,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub mac: String,
    pub name: String,
    /// When the test was started from this app (unix seconds).
    #[serde(default)]
    pub started: Option<u64>,
    /// First time this app saw the test running, with the progress then (hundredths of %).
    pub first_seen: (u64, u32),
    /// Most recent progress sample.
    pub last_seen: (u64, u32),
    #[serde(default)]
    pub finished: Option<u64>,
    #[serde(default)]
    pub outcome: Option<Outcome>,
}

impl Record {
    pub fn is_running(&self) -> bool {
        self.finished.is_none()
    }

    /// Estimated seconds left, from the progress rate observed so far.
    pub fn remaining(&self) -> Option<u64> {
        let (t0, p0) = self.started.map(|s| (s, 0)).unwrap_or(self.first_seen);
        let (t1, p1) = self.last_seen;
        let dt = t1.checked_sub(t0)?;
        let dp = p1.checked_sub(p0)?;
        if dt < 120 || dp < 20 || p1 >= 10_000 {
            return None; // not enough data for a meaningful estimate
        }
        Some(((10_000 - p1) as f64 * dt as f64 / dp as f64) as u64)
    }

    /// Seconds since the test started, and whether that's exact (started here) or a lower bound.
    pub fn elapsed(&self, now: u64) -> (u64, bool) {
        let end = self.finished.unwrap_or(now);
        match self.started {
            Some(s) => (end.saturating_sub(s), true),
            None => (end.saturating_sub(self.first_seen.0), false),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct History {
    pub records: Vec<Record>,
}

/// What changed when a device report was applied.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    None,
    Finished(Outcome),
}

impl History {
    pub fn load() -> Self {
        store::load(FILE)
    }

    pub fn save(&self) -> std::io::Result<()> {
        store::save(FILE, self)
    }

    pub fn running(&self, mac: &str) -> Option<&Record> {
        self.records.iter().rev().find(|r| r.mac == mac && r.is_running())
    }

    fn running_mut(&mut self, mac: &str) -> Option<&mut Record> {
        self.records.iter_mut().rev().find(|r| r.mac == mac && r.is_running())
    }

    pub fn for_device(&self, mac: &str) -> impl Iterator<Item = &Record> {
        self.records.iter().filter(move |r| r.mac == mac)
    }

    /// The app successfully asked `mac` to start a test.
    pub fn started(&mut self, mac: &str, name: &str, now: u64) {
        if let Some(r) = self.running_mut(mac) {
            // A test we only observed before is superseded.
            r.finished = Some(now);
        }
        self.records.push(Record {
            mac: mac.into(),
            name: name.into(),
            started: Some(now),
            first_seen: (now, 0),
            last_seen: (now, 0),
            finished: None,
            outcome: None,
        });
    }

    /// Feed a discovery reply. Returns `Event::Finished` when a running test ended.
    pub fn observe(&mut self, n: &NasInfo, now: u64) -> Event {
        if n.is_memtest_running() {
            let p = n.progress.unwrap_or(0);
            if let Some(r) = self.running_mut(&n.mac) {
                // Progress going backwards means a new pass began without us noticing.
                if p + 100 < r.last_seen.1 {
                    r.finished = Some(now);
                } else {
                    r.last_seen = (now, p);
                    return Event::None;
                }
            }
            self.records.push(Record {
                mac: n.mac.clone(),
                name: n.name.clone(),
                started: None,
                first_seen: (now, p),
                last_seen: (now, p),
                finished: None,
                outcome: None,
            });
            return Event::None;
        }
        // Rebooting into the test can report "Booting" first: keep waiting for those.
        let in_transition = matches!(n.status_code, Some(5 | 7));
        if in_transition {
            return Event::None;
        }
        let Some(r) = self.running_mut(&n.mac) else {
            return Event::None;
        };
        // Started here but the device never reported the test yet (still rebooting).
        if r.started.is_some() && r.last_seen.1 == 0 && now.saturating_sub(r.started.unwrap()) < 15 * 60 {
            return Event::None;
        }
        // Same rule as the original app: field 0x79 non-zero after the test = errors found.
        let outcome = if n.progress.unwrap_or(0) != 0 {
            Outcome::Errors
        } else {
            Outcome::NoErrors
        };
        r.finished = Some(now);
        r.outcome = Some(outcome);
        Event::Finished(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nas(code: u32, progress: u32) -> NasInfo {
        NasInfo {
            mac: "m".into(),
            name: "nas".into(),
            status_code: Some(code),
            progress: Some(progress),
            ..Default::default()
        }
    }

    #[test]
    fn observed_test_gets_estimate_and_result() {
        let mut h = History::default();
        assert_eq!(h.observe(&nas(9, 3855), 1000), Event::None);
        assert_eq!(h.running("m").unwrap().remaining(), None); // one sample only
        h.observe(&nas(9, 7536), 1000 + 12_600); // +36.81% in 3.5 h
        let left = h.running("m").unwrap().remaining().unwrap();
        assert!((8_000..9_000).contains(&left), "{left}");
        assert_eq!(h.observe(&nas(1, 0), 30_000), Event::Finished(Outcome::NoErrors));
        let r = &h.records[0];
        assert_eq!(r.outcome, Some(Outcome::NoErrors));
        assert_eq!(r.elapsed(99_999), (29_000, false));
    }

    #[test]
    fn started_here_waits_through_reboot_and_reports_errors() {
        let mut h = History::default();
        h.started("m", "nas", 0);
        assert_eq!(h.observe(&nas(1, 0), 60), Event::None); // still "Ready" right after the request
        assert_eq!(h.observe(&nas(5, 0), 120), Event::None); // booting
        h.observe(&nas(9, 500), 600);
        assert_eq!(h.observe(&nas(1, 3), 5000), Event::Finished(Outcome::Errors));
        assert_eq!(h.records.len(), 1);
        assert_eq!(h.records[0].elapsed(9999), (5000, true));
    }
}
