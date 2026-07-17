//! Pure detection logic: the tick-driven call state machine and the
//! process-name matcher. No OS calls — fully unit-tested; the WASAPI scan and
//! policy handling live in the sibling modules.

/// One observation per poll tick: is some candidate app using the mic?
/// Emitted transitions tell the poller when a call started or ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detection {
    /// A call has been continuously present for the arming window.
    Start(String),
    /// The call has been gone for the grace window.
    Stop,
}

#[derive(Debug)]
enum Phase {
    Idle,
    /// Mic in use; waiting out the detection delay before acting.
    Arming { app: String, ticks: u32 },
    /// A call is considered live.
    Active,
    /// Call disappeared; waiting out the grace window before declaring it over.
    Grace { ticks: u32 },
}

pub struct Detector {
    phase: Phase,
    /// Consecutive candidate ticks required before `Start` (≥1).
    arm_ticks: u32,
    /// Consecutive empty ticks required before `Stop` (≥1).
    grace_ticks: u32,
}

impl Detector {
    pub fn new(arm_ticks: u32, grace_ticks: u32) -> Self {
        Detector {
            phase: Phase::Idle,
            arm_ticks: arm_ticks.max(1),
            grace_ticks: grace_ticks.max(1),
        }
    }

    /// Whether a call is currently considered live (Active or in Grace).
    /// Test-only today; production consumers read the emitted transitions.
    #[cfg(test)]
    pub fn call_live(&self) -> bool {
        matches!(self.phase, Phase::Active | Phase::Grace { .. })
    }

    /// Feed one tick's observation; returns a transition when one fires.
    pub fn tick(&mut self, candidate: Option<&str>) -> Option<Detection> {
        match (&mut self.phase, candidate) {
            (Phase::Idle, Some(app)) => {
                if self.arm_ticks <= 1 {
                    self.phase = Phase::Active;
                    Some(Detection::Start(app.to_string()))
                } else {
                    self.phase = Phase::Arming {
                        app: app.to_string(),
                        ticks: 1,
                    };
                    None
                }
            }
            (Phase::Idle, None) => None,
            (Phase::Arming { app, ticks }, Some(current)) => {
                // Track the most recent candidate name while arming.
                *app = current.to_string();
                *ticks += 1;
                if *ticks >= self.arm_ticks {
                    let app = app.clone();
                    self.phase = Phase::Active;
                    Some(Detection::Start(app))
                } else {
                    None
                }
            }
            (Phase::Arming { .. }, None) => {
                self.phase = Phase::Idle;
                None
            }
            (Phase::Active, Some(_)) => None,
            (Phase::Active, None) => {
                if self.grace_ticks <= 1 {
                    self.phase = Phase::Idle;
                    Some(Detection::Stop)
                } else {
                    self.phase = Phase::Grace { ticks: 1 };
                    None
                }
            }
            // Call resumed inside the grace window: still the same call.
            (Phase::Grace { .. }, Some(_)) => {
                self.phase = Phase::Active;
                None
            }
            (Phase::Grace { ticks }, None) => {
                *ticks += 1;
                if *ticks >= self.grace_ticks {
                    self.phase = Phase::Idle;
                    Some(Detection::Stop)
                } else {
                    None
                }
            }
        }
    }
}

/// Whether a process name counts as a meeting app. Case-insensitive, `.exe`
/// stripped, substring match in either direction so "ms-teams" matches
/// "ms-teams.exe" and "zoom" matches "Zoom.exe".
pub fn match_app(process_name: &str, allowlist: &[String]) -> bool {
    let name = process_name
        .to_lowercase()
        .trim_end_matches(".exe")
        .to_string();
    if name.is_empty() {
        return false;
    }
    allowlist.iter().any(|entry| {
        let entry = entry.to_lowercase().trim_end_matches(".exe").to_string();
        !entry.is_empty() && (name.contains(&entry) || entry.contains(&name))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn matcher_is_case_insensitive_and_exe_agnostic() {
        let allow = list(&["zoom", "ms-teams", "chrome"]);
        assert!(match_app("Zoom.exe", &allow));
        assert!(match_app("ms-teams.exe", &allow));
        assert!(match_app("chrome", &allow));
        assert!(match_app("GoogleChrome.exe", &allow)); // substring
        assert!(!match_app("notepad.exe", &allow));
        assert!(!match_app("", &allow));
    }

    #[test]
    fn start_fires_after_arming_window() {
        let mut d = Detector::new(3, 2);
        assert_eq!(d.tick(Some("zoom")), None);
        assert_eq!(d.tick(Some("zoom")), None);
        assert_eq!(d.tick(Some("zoom")), Some(Detection::Start("zoom".into())));
        assert!(d.call_live());
        // Steady state produces nothing.
        assert_eq!(d.tick(Some("zoom")), None);
    }

    #[test]
    fn arming_resets_when_candidate_disappears() {
        let mut d = Detector::new(3, 2);
        d.tick(Some("zoom"));
        d.tick(Some("zoom"));
        assert_eq!(d.tick(None), None); // back to idle
        d.tick(Some("zoom"));
        d.tick(Some("zoom"));
        assert_eq!(d.tick(Some("zoom")), Some(Detection::Start("zoom".into())));
    }

    #[test]
    fn stop_fires_after_grace_and_rejoin_cancels_it() {
        let mut d = Detector::new(1, 3);
        assert_eq!(d.tick(Some("zoom")), Some(Detection::Start("zoom".into())));
        // Call drops…
        assert_eq!(d.tick(None), None);
        assert_eq!(d.tick(None), None);
        // …but rejoins inside the grace window: same call, no events.
        assert_eq!(d.tick(Some("zoom")), None);
        assert!(d.call_live());
        // Drops again and stays gone.
        assert_eq!(d.tick(None), None);
        assert_eq!(d.tick(None), None);
        assert_eq!(d.tick(None), Some(Detection::Stop));
        assert!(!d.call_live());
    }

    #[test]
    fn single_tick_windows_fire_immediately() {
        let mut d = Detector::new(1, 1);
        assert_eq!(d.tick(Some("meet")), Some(Detection::Start("meet".into())));
        assert_eq!(d.tick(None), Some(Detection::Stop));
    }

    #[test]
    fn arming_tracks_latest_candidate_name() {
        let mut d = Detector::new(2, 1);
        d.tick(Some("chrome"));
        assert_eq!(d.tick(Some("zoom")), Some(Detection::Start("zoom".into())));
    }
}
