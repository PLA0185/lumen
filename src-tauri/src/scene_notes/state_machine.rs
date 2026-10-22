use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneState {
    Outside,
    Entering,
    Visible,
    Acked,
    Snoozed,
    Exiting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneTransition {
    None,
    Show,
    Hide,
}

#[derive(Debug, Clone)]
pub struct SceneStateMachine {
    state: SceneState,
    consecutive_hits: u8,
    miss_started: Option<Instant>,
    snooze_until: Option<Instant>,
    exit_previous: Option<SceneState>,
}

impl Default for SceneStateMachine {
    fn default() -> Self {
        Self {
            state: SceneState::Outside,
            consecutive_hits: 0,
            miss_started: None,
            snooze_until: None,
            exit_previous: None,
        }
    }
}

impl SceneStateMachine {
    pub fn restore(state: SceneState, snooze_remaining: Option<Duration>) -> Self {
        let mut machine = Self::default();
        match state {
            SceneState::Visible => machine.state = SceneState::Visible,
            SceneState::Acked => machine.state = SceneState::Acked,
            SceneState::Snoozed => {
                machine.state = SceneState::Snoozed;
                machine.snooze_until = Some(Instant::now() + snooze_remaining.unwrap_or_default());
            }
            SceneState::Outside | SceneState::Entering | SceneState::Exiting => {}
        }
        machine
    }

    pub fn state(&self) -> SceneState {
        self.state
    }

    /// Keep a visible reminder on screen during the configured transient-leave window.
    pub fn should_show_note(&self) -> bool {
        self.state == SceneState::Visible
            || (self.state == SceneState::Exiting
                && self.exit_previous == Some(SceneState::Visible))
    }

    pub fn acknowledge(&mut self) {
        self.state = SceneState::Acked;
        self.miss_started = None;
        self.snooze_until = None;
        self.consecutive_hits = 0;
        self.exit_previous = None;
    }

    pub fn snooze(&mut self, now: Instant, duration: Duration) {
        self.state = SceneState::Snoozed;
        self.snooze_until = Some(now + duration);
        self.miss_started = None;
        self.consecutive_hits = 0;
        self.exit_previous = None;
    }

    pub fn observe(
        &mut self,
        matched: bool,
        now: Instant,
        required_hits: u8,
        leave_after: Duration,
    ) -> SceneTransition {
        if matched {
            self.miss_started = None;
            match self.state {
                SceneState::Outside => {
                    self.state = SceneState::Entering;
                    self.consecutive_hits = 1;
                    if self.consecutive_hits >= required_hits.max(1) {
                        self.state = SceneState::Visible;
                        return SceneTransition::Show;
                    }
                }
                SceneState::Entering => {
                    self.consecutive_hits = self.consecutive_hits.saturating_add(1);
                    if self.consecutive_hits >= required_hits.max(1) {
                        self.state = SceneState::Visible;
                        return SceneTransition::Show;
                    }
                }
                SceneState::Snoozed => {
                    if self.snooze_until.is_some_and(|until| now >= until) {
                        self.state = SceneState::Visible;
                        self.snooze_until = None;
                        return SceneTransition::Show;
                    }
                }
                SceneState::Exiting => {
                    self.state = self.exit_previous.take().unwrap_or(SceneState::Visible);
                    self.consecutive_hits = 0;
                }
                SceneState::Visible | SceneState::Acked => {}
            }
            return SceneTransition::None;
        }

        self.consecutive_hits = 0;
        match self.state {
            SceneState::Outside => SceneTransition::None,
            SceneState::Entering => {
                self.state = SceneState::Outside;
                SceneTransition::None
            }
            SceneState::Visible | SceneState::Snoozed | SceneState::Acked => {
                self.exit_previous = Some(self.state);
                self.state = SceneState::Exiting;
                self.miss_started = Some(now);
                SceneTransition::None
            }
            SceneState::Exiting => {
                if self
                    .miss_started
                    .is_some_and(|started| now.duration_since(started) >= leave_after)
                {
                    self.state = SceneState::Outside;
                    self.miss_started = None;
                    self.snooze_until = None;
                    self.exit_previous = None;
                    SceneTransition::Hide
                } else {
                    SceneTransition::None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_stable_entry_and_acknowledges_until_a_real_exit() {
        let start = Instant::now();
        let mut machine = SceneStateMachine::default();
        let leave = Duration::from_secs(6);
        assert_eq!(
            machine.observe(true, start, 2, leave),
            SceneTransition::None
        );
        assert_eq!(
            machine.observe(true, start + Duration::from_secs(2), 2, leave),
            SceneTransition::Show
        );
        machine.acknowledge();
        assert!(!machine.should_show_note());
        assert_eq!(
            machine.observe(true, start + Duration::from_secs(3), 2, leave),
            SceneTransition::None
        );
        machine.observe(false, start + Duration::from_secs(4), 2, leave);
        assert_eq!(
            machine.observe(true, start + Duration::from_secs(7), 2, leave),
            SceneTransition::None
        );
        assert_eq!(machine.state(), SceneState::Acked);
        machine.acknowledge();
        machine.observe(false, start + Duration::from_secs(8), 2, leave);
        assert_eq!(
            machine.observe(true, start + Duration::from_secs(9), 2, leave),
            SceneTransition::None
        );
        assert_eq!(machine.state(), SceneState::Acked);
        machine.observe(false, start + Duration::from_secs(10), 2, leave);
        assert_eq!(
            machine.observe(false, start + Duration::from_secs(16), 2, leave),
            SceneTransition::Hide
        );
        assert_eq!(
            machine.observe(true, start + Duration::from_secs(17), 2, leave),
            SceneTransition::None
        );
        assert_eq!(
            machine.observe(true, start + Duration::from_secs(19), 2, leave),
            SceneTransition::Show
        );
    }

    #[test]
    fn snooze_only_redisplays_when_due_while_scene_still_matches() {
        let now = Instant::now();
        let mut machine = SceneStateMachine::default();
        machine.observe(true, now, 1, Duration::from_secs(6));
        machine.snooze(now, Duration::from_secs(10));
        assert_eq!(
            machine.observe(
                true,
                now + Duration::from_secs(9),
                1,
                Duration::from_secs(6)
            ),
            SceneTransition::None
        );
        assert_eq!(
            machine.observe(
                true,
                now + Duration::from_secs(10),
                1,
                Duration::from_secs(6)
            ),
            SceneTransition::Show
        );
    }

    #[test]
    fn visible_reminder_survives_a_transient_miss_until_leave_timeout() {
        let start = Instant::now();
        let mut machine = SceneStateMachine::default();
        let leave = Duration::from_secs(6);
        assert_eq!(
            machine.observe(true, start, 1, leave),
            SceneTransition::Show
        );
        assert!(machine.should_show_note());
        assert_eq!(
            machine.observe(false, start + Duration::from_secs(1), 1, leave),
            SceneTransition::None
        );
        assert!(
            machine.should_show_note(),
            "a short mismatch must keep the existing note visible"
        );
        assert_eq!(
            machine.observe(true, start + Duration::from_secs(2), 1, leave),
            SceneTransition::None
        );
        assert!(machine.should_show_note());
        machine.observe(false, start + Duration::from_secs(3), 1, leave);
        assert_eq!(
            machine.observe(false, start + Duration::from_secs(9), 1, leave),
            SceneTransition::Hide
        );
        assert!(!machine.should_show_note());
    }

    #[test]
    fn restores_acknowledgement_and_snooze_without_restoring_transient_entry() {
        let acked = SceneStateMachine::restore(SceneState::Acked, None);
        assert_eq!(acked.state(), SceneState::Acked);
        assert!(!acked.should_show_note());

        let snoozed =
            SceneStateMachine::restore(SceneState::Snoozed, Some(Duration::from_secs(60)));
        assert_eq!(snoozed.state(), SceneState::Snoozed);
        assert!(!snoozed.should_show_note());

        let transient = SceneStateMachine::restore(SceneState::Entering, None);
        assert_eq!(transient.state(), SceneState::Outside);
    }
}
