use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::config::{ProcessConfig, RestartPolicy};

const DEFAULT_RESTART_LIMIT_BURST: usize = 5;
const DEFAULT_RESTART_LIMIT_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorPhase {
    /// Spawned, no READY or WATCHDOG received yet
    Starting,
    /// READY=1 received (or require_ready=false and first WATCHDOG received)
    Ready,
    /// Restart rate limit exceeded or policy says stop
    GaveUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    Success,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Ready,
    WatchdogPing,
    /// Process signals explicit failure (WATCHDOG=trigger)
    WatchdogTrigger,
    WatchdogTimeout,
    StartupTimeout,
    /// Process requests more startup time (EXTEND_TIMEOUT_USEC)
    ExtendTimeout {
        usec: u64,
    },
    ProcessExit {
        status: ExitStatus,
    },
    FileChange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Restart,
    GiveUp { reason: &'static str },
    None,
}

/// Pure state machine for per-process supervision.
///
/// No I/O, no handles — takes `Event`s, returns `Action`s.
/// The supervisor select loop maps I/O events into `Event`s, calls `on_event()`,
/// and dispatches the returned `Action` to the appropriate I/O operation.
#[derive(Debug)]
pub struct SupervisorState {
    // Restart rate limiting (sliding window, like systemd StartLimitBurst/StartLimitIntervalSec)
    restart_timestamps: VecDeque<Instant>,
    restart_count: usize,
    restart_limit_burst: usize,
    restart_limit_interval: Duration,

    watchdog_armed: bool,
    watchdog_deadline: Option<Instant>,
    startup_deadline: Option<Instant>,
    phase: SupervisorPhase,

    // Config (immutable after construction)
    watchdog_timeout: Option<Duration>,
    watchdog_require_ready: bool,
    restart_policy: RestartPolicy,
    startup_timeout: Option<Duration>,
}

impl SupervisorState {
    pub fn new(config: &ProcessConfig, now: Instant) -> Self {
        let watchdog_timeout = config
            .watchdog
            .as_ref()
            .map(|w| Duration::from_micros(w.usec));
        let watchdog_require_ready = config.watchdog.as_ref().is_none_or(|w| w.require_ready);
        // startup_timeout, restart_limit_burst, restart_limit_interval added in Phase 9.
        // Until then, fall back to max_restarts for burst limit.
        let startup_timeout = None;
        let restart_limit_burst = config.max_restarts.unwrap_or(DEFAULT_RESTART_LIMIT_BURST);

        let mut state = Self {
            restart_timestamps: VecDeque::new(),
            restart_count: 0,
            restart_limit_burst,
            restart_limit_interval: DEFAULT_RESTART_LIMIT_INTERVAL,
            watchdog_armed: false,
            watchdog_deadline: None,
            startup_deadline: startup_timeout.map(|d: Duration| now + d),
            phase: SupervisorPhase::Starting,
            watchdog_timeout,
            watchdog_require_ready,
            restart_policy: config.restart,
            startup_timeout,
        };

        state.arm_initial_watchdog(now);
        state
    }

    pub fn on_event(&mut self, event: Event, now: Instant) -> Action {
        match event {
            Event::Ready => {
                self.watchdog_armed = true;
                self.startup_deadline = None;
                if let Some(timeout) = self.watchdog_timeout {
                    self.watchdog_deadline = Some(now + timeout);
                }
                self.phase = SupervisorPhase::Ready;
                Action::None
            }
            Event::WatchdogPing => {
                if let Some(timeout) = self.watchdog_timeout {
                    self.watchdog_deadline = Some(now + timeout);
                }
                self.startup_deadline = None;
                if !self.watchdog_require_ready {
                    self.watchdog_armed = true;
                    self.phase = SupervisorPhase::Ready;
                }
                Action::None
            }
            Event::WatchdogTrigger => self.try_restart(now, "watchdog trigger"),
            Event::WatchdogTimeout => self.try_restart(now, "watchdog timeout"),
            Event::StartupTimeout => self.try_restart(now, "startup timeout"),
            Event::ExtendTimeout { usec } => {
                if self.phase == SupervisorPhase::Starting
                    && let Some(deadline) = self.startup_deadline.as_mut()
                {
                    *deadline += Duration::from_micros(usec);
                }
                Action::None
            }
            Event::ProcessExit { status } => {
                let should_restart = match self.restart_policy {
                    RestartPolicy::Never => false,
                    RestartPolicy::Always => true,
                    RestartPolicy::OnFailure => status == ExitStatus::Failure,
                };
                if !should_restart {
                    return Action::None;
                }
                self.try_restart(now, "process exit")
            }
            Event::FileChange => Action::Restart,
        }
    }

    /// Called after a restart completes — single place to reset state.
    pub fn on_restart_complete(&mut self, now: Instant) {
        self.restart_count += 1;
        self.watchdog_armed = false;
        self.watchdog_deadline = None;
        self.phase = SupervisorPhase::Starting;
        self.startup_deadline = self.startup_timeout.map(|d| now + d);
        self.arm_initial_watchdog(now);
    }

    /// Next deadline the select loop should wake for.
    pub fn next_deadline(&self) -> Option<Instant> {
        let wd = if self.watchdog_armed {
            self.watchdog_deadline
        } else {
            None
        };
        match (self.startup_deadline, wd) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    }

    /// Whether `deadline` is the startup deadline (vs watchdog).
    pub fn is_startup_deadline(&self, deadline: Instant) -> bool {
        self.startup_deadline == Some(deadline)
    }

    pub fn restart_count(&self) -> usize {
        self.restart_count
    }

    pub fn is_ready(&self) -> bool {
        self.phase == SupervisorPhase::Ready
    }

    pub fn phase(&self) -> SupervisorPhase {
        self.phase
    }

    fn try_restart(&mut self, now: Instant, context: &'static str) -> Action {
        if self.can_restart(now) {
            Action::Restart
        } else {
            self.phase = SupervisorPhase::GaveUp;
            Action::GiveUp {
                reason: match context {
                    "watchdog trigger" => "watchdog trigger: restart rate limit exceeded",
                    "watchdog timeout" => "watchdog timeout: restart rate limit exceeded",
                    "startup timeout" => "startup timeout: restart rate limit exceeded",
                    "process exit" => "process exit: restart rate limit exceeded",
                    _ => "restart rate limit exceeded",
                },
            }
        }
    }

    /// Check if a restart is allowed under the sliding-window rate limit.
    /// If allowed, records the timestamp.
    fn can_restart(&mut self, now: Instant) -> bool {
        let cutoff = now - self.restart_limit_interval;
        while self.restart_timestamps.front().is_some_and(|&t| t < cutoff) {
            self.restart_timestamps.pop_front();
        }
        if self.restart_timestamps.len() >= self.restart_limit_burst {
            return false;
        }
        self.restart_timestamps.push_back(now);
        true
    }

    /// When require_ready=false and watchdog is configured, arm from the start
    /// so that the watchdog timeout fires if the process never pings.
    fn arm_initial_watchdog(&mut self, now: Instant) {
        if !self.watchdog_require_ready
            && let Some(timeout) = self.watchdog_timeout
        {
            self.watchdog_armed = true;
            self.watchdog_deadline = Some(now + timeout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WatchdogConfig;

    // -- helpers --

    fn config_default() -> ProcessConfig {
        ProcessConfig::default()
    }

    fn config_with_watchdog(usec: u64, require_ready: bool) -> ProcessConfig {
        ProcessConfig {
            watchdog: Some(WatchdogConfig {
                usec,
                require_ready,
            }),
            ..Default::default()
        }
    }

    fn config_with_policy(policy: RestartPolicy) -> ProcessConfig {
        ProcessConfig {
            restart: policy,
            ..Default::default()
        }
    }

    fn config_watchdog_and_policy(
        usec: u64,
        require_ready: bool,
        policy: RestartPolicy,
    ) -> ProcessConfig {
        ProcessConfig {
            watchdog: Some(WatchdogConfig {
                usec,
                require_ready,
            }),
            restart: policy,
            ..Default::default()
        }
    }

    /// Build a state with startup_timeout (not yet in ProcessConfig).
    fn state_with_startup_timeout(
        config: &ProcessConfig,
        startup_timeout: Duration,
        now: Instant,
    ) -> SupervisorState {
        let mut state = SupervisorState::new(config, now);
        state.startup_timeout = Some(startup_timeout);
        state.startup_deadline = Some(now + startup_timeout);
        state
    }

    // =============================================================
    // Watchdog
    // =============================================================

    #[test]
    fn watchdog_armed_immediately_when_require_ready_false() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, false);
        let state = SupervisorState::new(&config, now);

        assert!(state.watchdog_armed);
        assert_eq!(state.watchdog_deadline, Some(now + Duration::from_secs(1)));
        assert!(state.next_deadline().is_some());
    }

    #[test]
    fn watchdog_not_armed_until_ready_when_require_ready_true() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, true);
        let state = SupervisorState::new(&config, now);

        assert!(!state.watchdog_armed);
        assert!(state.watchdog_deadline.is_none());
        assert!(state.next_deadline().is_none());
    }

    #[test]
    fn watchdog_ping_resets_deadline() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, false);
        let mut state = SupervisorState::new(&config, now);

        let later = now + Duration::from_millis(500);
        let action = state.on_event(Event::WatchdogPing, later);

        assert_eq!(action, Action::None);
        assert_eq!(
            state.watchdog_deadline,
            Some(later + Duration::from_secs(1))
        );
    }

    #[test]
    fn watchdog_ping_before_ready_resets_deadline_but_does_not_arm() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, true);
        let mut state = SupervisorState::new(&config, now);

        let later = now + Duration::from_millis(500);
        state.on_event(Event::WatchdogPing, later);

        // Deadline is set (for tracking)...
        assert_eq!(
            state.watchdog_deadline,
            Some(later + Duration::from_secs(1))
        );
        // ...but not armed, so next_deadline() doesn't return it
        assert!(!state.watchdog_armed);
        assert!(state.next_deadline().is_none());
    }

    #[test]
    fn watchdog_timeout_triggers_restart() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, false);
        let mut state = SupervisorState::new(&config, now);

        let action = state.on_event(Event::WatchdogTimeout, now + Duration::from_secs(1));
        assert_eq!(action, Action::Restart);
    }

    #[test]
    fn watchdog_timeout_respects_rate_limit() {
        let now = Instant::now();
        let config = config_with_watchdog(100_000, false);
        let mut state = SupervisorState::new(&config, now);

        // Exhaust the burst limit (default 5)
        for i in 0..5 {
            let t = now + Duration::from_millis(i * 10);
            let action = state.on_event(Event::WatchdogTimeout, t);
            assert_eq!(action, Action::Restart);
            state.on_restart_complete(t);
        }

        // Next one should fail
        let t = now + Duration::from_millis(100);
        let action = state.on_event(Event::WatchdogTimeout, t);
        assert!(matches!(action, Action::GiveUp { .. }));
        assert_eq!(state.phase(), SupervisorPhase::GaveUp);
    }

    #[test]
    fn watchdog_trigger_triggers_restart() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, false);
        let mut state = SupervisorState::new(&config, now);

        let action = state.on_event(Event::WatchdogTrigger, now);
        assert_eq!(action, Action::Restart);
    }

    #[test]
    fn watchdog_trigger_respects_rate_limit() {
        let now = Instant::now();
        let config = config_with_watchdog(100_000, false);
        let mut state = SupervisorState::new(&config, now);

        for i in 0..5 {
            let t = now + Duration::from_millis(i * 10);
            assert_eq!(state.on_event(Event::WatchdogTrigger, t), Action::Restart);
            state.on_restart_complete(t);
        }

        let t = now + Duration::from_millis(100);
        assert!(matches!(
            state.on_event(Event::WatchdogTrigger, t),
            Action::GiveUp { .. }
        ));
    }

    // =============================================================
    // Startup timeout
    // =============================================================

    #[test]
    fn startup_timeout_triggers_restart() {
        let now = Instant::now();
        let config = config_default();
        let mut state = state_with_startup_timeout(&config, Duration::from_secs(5), now);

        let action = state.on_event(Event::StartupTimeout, now + Duration::from_secs(5));
        assert_eq!(action, Action::Restart);
    }

    #[test]
    fn startup_timeout_cleared_by_ready() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, true);
        let mut state = state_with_startup_timeout(&config, Duration::from_secs(5), now);
        assert!(state.startup_deadline.is_some());

        state.on_event(Event::Ready, now + Duration::from_secs(1));
        assert!(state.startup_deadline.is_none());
    }

    #[test]
    fn startup_timeout_cleared_by_watchdog_ping() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, true);
        let mut state = state_with_startup_timeout(&config, Duration::from_secs(5), now);
        assert!(state.startup_deadline.is_some());

        state.on_event(Event::WatchdogPing, now + Duration::from_secs(1));
        assert!(state.startup_deadline.is_none());
    }

    #[test]
    fn startup_timeout_respects_rate_limit() {
        let now = Instant::now();
        let config = config_default();
        let mut state = state_with_startup_timeout(&config, Duration::from_secs(1), now);

        for i in 0..5 {
            let t = now + Duration::from_millis(i * 10);
            assert_eq!(state.on_event(Event::StartupTimeout, t), Action::Restart);
            state.on_restart_complete(t);
        }

        let t = now + Duration::from_millis(100);
        assert!(matches!(
            state.on_event(Event::StartupTimeout, t),
            Action::GiveUp { .. }
        ));
    }

    #[test]
    fn extend_timeout_pushes_startup_deadline_forward() {
        let now = Instant::now();
        let config = config_default();
        let mut state = state_with_startup_timeout(&config, Duration::from_secs(5), now);

        let original_deadline = state.startup_deadline.unwrap();
        state.on_event(Event::ExtendTimeout { usec: 3_000_000 }, now);
        assert_eq!(
            state.startup_deadline,
            Some(original_deadline + Duration::from_secs(3))
        );
    }

    #[test]
    fn extend_timeout_ignored_when_not_starting() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, true);
        let mut state = state_with_startup_timeout(&config, Duration::from_secs(5), now);

        // Move to Ready phase
        state.on_event(Event::Ready, now);
        let deadline_before = state.startup_deadline;

        state.on_event(Event::ExtendTimeout { usec: 3_000_000 }, now);
        assert_eq!(state.startup_deadline, deadline_before);
    }

    #[test]
    fn next_deadline_returns_earlier_of_startup_and_watchdog() {
        let now = Instant::now();
        let config = config_with_watchdog(10_000_000, false); // 10s watchdog
        let mut state = state_with_startup_timeout(&config, Duration::from_secs(3), now);
        // require_ready=false → watchdog armed from start

        let startup = now + Duration::from_secs(3);
        let watchdog = now + Duration::from_secs(10);

        // Startup is earlier
        assert_eq!(state.next_deadline(), Some(startup));
        assert!(state.is_startup_deadline(startup));

        // After extending startup past watchdog, watchdog is earlier
        state.on_event(
            Event::ExtendTimeout {
                usec: 10_000_000, // +10s
            },
            now,
        );
        assert_eq!(state.next_deadline(), Some(watchdog));
        assert!(!state.is_startup_deadline(watchdog));
    }

    // =============================================================
    // Restart rate limiting
    // =============================================================

    #[test]
    fn restarts_within_window_are_allowed() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        for i in 0..5 {
            let t = now + Duration::from_millis(i * 10);
            let action = state.on_event(
                Event::ProcessExit {
                    status: ExitStatus::Failure,
                },
                t,
            );
            assert_eq!(action, Action::Restart);
            state.on_restart_complete(t);
        }
    }

    #[test]
    fn restarts_exceeding_burst_trigger_give_up() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        for i in 0..5 {
            let t = now + Duration::from_millis(i * 10);
            state.on_event(
                Event::ProcessExit {
                    status: ExitStatus::Failure,
                },
                t,
            );
            state.on_restart_complete(t);
        }

        let t = now + Duration::from_millis(100);
        let action = state.on_event(
            Event::ProcessExit {
                status: ExitStatus::Failure,
            },
            t,
        );
        assert!(matches!(action, Action::GiveUp { .. }));
    }

    #[test]
    fn old_restart_timestamps_expire_outside_window() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        // Use up 4 of the 5 burst slots
        for i in 0..4 {
            let t = now + Duration::from_millis(i * 10);
            state.on_event(
                Event::ProcessExit {
                    status: ExitStatus::Failure,
                },
                t,
            );
            state.on_restart_complete(t);
        }

        // Jump past the 10s window — old timestamps expire
        let later = now + Duration::from_secs(15);
        let action = state.on_event(
            Event::ProcessExit {
                status: ExitStatus::Failure,
            },
            later,
        );
        assert_eq!(action, Action::Restart);
    }

    #[test]
    fn healthy_service_can_crash_and_restart_again() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        // Exhaust the burst limit
        for i in 0..5 {
            let t = now + Duration::from_millis(i * 10);
            state.on_event(
                Event::ProcessExit {
                    status: ExitStatus::Failure,
                },
                t,
            );
            state.on_restart_complete(t);
        }

        // After running healthy for a long time, crash again — should be allowed
        let much_later = now + Duration::from_secs(60);
        let action = state.on_event(
            Event::ProcessExit {
                status: ExitStatus::Failure,
            },
            much_later,
        );
        assert_eq!(action, Action::Restart);
    }

    #[test]
    fn file_change_restarts_do_not_count_toward_rate_limit() {
        let now = Instant::now();
        let config = config_default();
        let mut state = SupervisorState::new(&config, now);

        // Flood with file changes — should never hit the limit
        for i in 0..20 {
            let t = now + Duration::from_millis(i * 10);
            let action = state.on_event(Event::FileChange, t);
            assert_eq!(action, Action::Restart);
            state.on_restart_complete(t);
        }
    }

    #[test]
    fn rate_limit_shared_across_trigger_types() {
        let now = Instant::now();
        let config = config_watchdog_and_policy(100_000, false, RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        // Mix of trigger types, all counting toward the same limit
        let t0 = now;
        assert_eq!(state.on_event(Event::WatchdogTimeout, t0), Action::Restart);
        state.on_restart_complete(t0);

        let t1 = now + Duration::from_millis(10);
        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitStatus::Failure,
                },
                t1,
            ),
            Action::Restart
        );
        state.on_restart_complete(t1);

        let t2 = now + Duration::from_millis(20);
        assert_eq!(state.on_event(Event::StartupTimeout, t2), Action::Restart);
        state.on_restart_complete(t2);

        let t3 = now + Duration::from_millis(30);
        assert_eq!(state.on_event(Event::WatchdogTrigger, t3), Action::Restart);
        state.on_restart_complete(t3);

        let t4 = now + Duration::from_millis(40);
        assert_eq!(state.on_event(Event::WatchdogTimeout, t4), Action::Restart);
        state.on_restart_complete(t4);

        // 6th restart within the window — should be denied
        let t5 = now + Duration::from_millis(50);
        assert!(matches!(
            state.on_event(Event::WatchdogTimeout, t5),
            Action::GiveUp { .. }
        ));
    }

    // =============================================================
    // Restart policy
    // =============================================================

    #[test]
    fn policy_never_does_not_restart() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Never);
        let mut state = SupervisorState::new(&config, now);

        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitStatus::Success,
                },
                now,
            ),
            Action::None
        );

        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitStatus::Failure,
                },
                now,
            ),
            Action::None
        );
    }

    #[test]
    fn policy_always_restarts_on_success_and_failure() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitStatus::Success,
                },
                now,
            ),
            Action::Restart
        );
        state.on_restart_complete(now);

        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitStatus::Failure,
                },
                now + Duration::from_millis(10),
            ),
            Action::Restart
        );
    }

    #[test]
    fn policy_on_failure_restarts_on_failure_not_success() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::OnFailure);
        let mut state = SupervisorState::new(&config, now);

        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitStatus::Success,
                },
                now,
            ),
            Action::None
        );

        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitStatus::Failure,
                },
                now,
            ),
            Action::Restart
        );
    }

    // =============================================================
    // Phase transitions
    // =============================================================

    #[test]
    fn phase_starting_to_ready_via_ready() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, true);
        let mut state = SupervisorState::new(&config, now);

        assert_eq!(state.phase(), SupervisorPhase::Starting);
        state.on_event(Event::Ready, now);
        assert_eq!(state.phase(), SupervisorPhase::Ready);
        assert!(state.is_ready());
    }

    #[test]
    fn phase_starting_to_ready_via_watchdog_ping_when_not_require_ready() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, false);
        let mut state = SupervisorState::new(&config, now);

        assert_eq!(state.phase(), SupervisorPhase::Starting);
        state.on_event(Event::WatchdogPing, now);
        assert_eq!(state.phase(), SupervisorPhase::Ready);
        assert!(state.is_ready());
    }

    #[test]
    fn phase_to_gave_up_via_rate_limit() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        for i in 0..5 {
            let t = now + Duration::from_millis(i * 10);
            state.on_event(
                Event::ProcessExit {
                    status: ExitStatus::Failure,
                },
                t,
            );
            state.on_restart_complete(t);
        }

        let t = now + Duration::from_millis(100);
        state.on_event(
            Event::ProcessExit {
                status: ExitStatus::Failure,
            },
            t,
        );
        assert_eq!(state.phase(), SupervisorPhase::GaveUp);
    }

    #[test]
    fn on_restart_complete_resets_all_transient_state() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, true);
        let mut state = state_with_startup_timeout(&config, Duration::from_secs(5), now);

        // Move to Ready, arm watchdog
        state.on_event(Event::Ready, now);
        assert_eq!(state.phase(), SupervisorPhase::Ready);
        assert!(state.watchdog_armed);
        assert!(state.startup_deadline.is_none());

        // Restart
        let t = now + Duration::from_millis(100);
        state.on_restart_complete(t);

        assert_eq!(state.phase(), SupervisorPhase::Starting);
        assert!(!state.watchdog_armed);
        assert!(state.watchdog_deadline.is_none());
        assert_eq!(state.startup_deadline, Some(t + Duration::from_secs(5)));
        assert_eq!(state.restart_count(), 1);
    }

    #[test]
    fn on_restart_complete_rearms_watchdog_when_not_require_ready() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, false);
        let mut state = SupervisorState::new(&config, now);

        let t = now + Duration::from_millis(100);
        state.on_restart_complete(t);

        assert!(state.watchdog_armed);
        assert_eq!(state.watchdog_deadline, Some(t + Duration::from_secs(1)));
        assert_eq!(state.phase(), SupervisorPhase::Starting);
    }

    // =============================================================
    // File change
    // =============================================================

    #[test]
    fn file_change_always_triggers_restart() {
        let now = Instant::now();
        let config = config_default();
        let mut state = SupervisorState::new(&config, now);

        assert_eq!(state.on_event(Event::FileChange, now), Action::Restart);
    }

    #[test]
    fn file_change_works_even_after_rate_limit_exhausted() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        // Exhaust rate limit via process exits
        for i in 0..5 {
            let t = now + Duration::from_millis(i * 10);
            state.on_event(
                Event::ProcessExit {
                    status: ExitStatus::Failure,
                },
                t,
            );
            state.on_restart_complete(t);
        }

        // Verify rate limit is hit
        let t = now + Duration::from_millis(100);
        assert!(matches!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitStatus::Failure,
                },
                t,
            ),
            Action::GiveUp { .. }
        ));

        // File change still works
        assert_eq!(state.on_event(Event::FileChange, t), Action::Restart);
    }
}
