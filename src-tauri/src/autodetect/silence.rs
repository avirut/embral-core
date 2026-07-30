//! The silence check-in's decision rule ([detection.md] §Auto-stop on
//! silence): after a configured stretch with no transcribed word, the
//! recording asks "Still recording?"; unanswered past a fixed grace it
//! stops (or stands down, per the setting). Pure — the watcher task in
//! `commands::recording` feeds it clocks and acts on the verdict.

/// How long an unanswered check-in waits before acting. A constant, not a
/// setting: long enough to grab the mouse mid-meeting, short enough to
/// still save the hours the feature exists to save.
pub const GRACE_SECS: u64 = 120;

/// The check-in's current standing, as the watcher tracks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// No check-in showing.
    None,
    /// A check-in fired this long ago and is awaiting an answer.
    Pending { age_secs: u64 },
    /// "Keep recording" ran its course — no re-nagging until speech resumes.
    StoodDown,
}

/// What one watcher tick should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing to do.
    Quiet,
    /// Silence just crossed the threshold: raise the check-in.
    Notify,
    /// A check-in is up; keep waiting for an answer.
    Waiting,
    /// The grace ran out with no answer: act per the setting.
    Unanswered,
    /// Speech resumed: take the check-in (or the stand-down) back down.
    Cleared,
}

pub fn check(silence_secs: u64, threshold_secs: u64, notice: Notice) -> Verdict {
    if threshold_secs == 0 {
        // Off. The caller also skips entirely; this keeps the rule total.
        return match notice {
            Notice::None => Verdict::Quiet,
            _ => Verdict::Cleared,
        };
    }
    if silence_secs < threshold_secs {
        return match notice {
            Notice::None => Verdict::Quiet,
            _ => Verdict::Cleared,
        };
    }
    match notice {
        Notice::None => Verdict::Notify,
        Notice::Pending { age_secs } if age_secs >= GRACE_SECS => Verdict::Unanswered,
        Notice::Pending { .. } => Verdict::Waiting,
        Notice::StoodDown => Verdict::Quiet,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THRESHOLD: u64 = 300; // the 5-minute default

    #[test]
    fn quiet_until_the_threshold_then_notify_once() {
        assert_eq!(check(0, THRESHOLD, Notice::None), Verdict::Quiet);
        assert_eq!(check(299, THRESHOLD, Notice::None), Verdict::Quiet);
        assert_eq!(check(300, THRESHOLD, Notice::None), Verdict::Notify);
        // With the notice up, later ticks wait rather than re-notify.
        assert_eq!(
            check(360, THRESHOLD, Notice::Pending { age_secs: 60 }),
            Verdict::Waiting
        );
    }

    #[test]
    fn the_grace_running_out_means_unanswered() {
        assert_eq!(
            check(420, THRESHOLD, Notice::Pending { age_secs: GRACE_SECS }),
            Verdict::Unanswered
        );
    }

    #[test]
    fn speech_resuming_clears_a_notice_or_a_stand_down() {
        assert_eq!(
            check(10, THRESHOLD, Notice::Pending { age_secs: 60 }),
            Verdict::Cleared
        );
        assert_eq!(check(10, THRESHOLD, Notice::StoodDown), Verdict::Cleared);
        assert_eq!(check(10, THRESHOLD, Notice::None), Verdict::Quiet);
    }

    #[test]
    fn a_stand_down_never_renags_while_silence_continues() {
        assert_eq!(check(9999, THRESHOLD, Notice::StoodDown), Verdict::Quiet);
    }

    #[test]
    fn zero_threshold_is_off() {
        assert_eq!(check(9999, 0, Notice::None), Verdict::Quiet);
        // A notice left up when the setting turns off comes down.
        assert_eq!(
            check(9999, 0, Notice::Pending { age_secs: 10 }),
            Verdict::Cleared
        );
    }
}
