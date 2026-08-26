use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::config::{ProcessConfig, RestartPolicy};
use crate::manager::ProcessPhase;
pub use crate::process_state::ExitOutcome;
use crate::process_state::{
    ChildState, ProcessStatus, ReadinessState, RestartDecision, StateTransition, StopReason,
    TargetState,
};

const DEFAULT_RESTART_LIMIT_BURST: usize = 5;

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
        status: ExitOutcome,
    },
    FileChange,
    /// Stop requested by the process manager.
    StopRequested,
}

#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Restart,
    GiveUp { reason: &'static str },
    None,
}

/// Restart trigger, used in give-up messages.
#[derive(Debug, Clone, Copy)]
enum RestartTrigger {
    WatchdogTrigger,
    WatchdogTimeout,
    StartupTimeout,
    ProcessExit,
}

impl RestartTrigger {
    fn give_up_reason(self) -> &'static str {
        match self {
            Self::WatchdogTrigger => "watchdog trigger: restart rate limit exceeded",
            Self::WatchdogTimeout => "watchdog timeout: restart rate limit exceeded",
            Self::StartupTimeout => "startup timeout: restart rate limit exceeded",
            Self::ProcessExit => "process exit: restart rate limit exceeded",
        }
    }
}

pub type JobStatus = ProcessStatus;

/// Pure per-process supervision state machine with no I/O.
#[derive(Debug)]
pub struct SupervisorState {
    // Restart rate limiting; restart_limit_interval = None means a lifetime limit (no window).
    restart_timestamps: VecDeque<Instant>,
    restart_limit_burst: usize,
    restart_limit_interval: Option<Duration>,

    watchdog_armed: bool,
    watchdog_deadline: Option<Instant>,
    startup_deadline: Option<Instant>,
    status: ProcessStatus,

    watchdog_timeout: Option<Duration>,
    watchdog_require_ready: bool,
    readiness_required: bool,
    restart_policy: RestartPolicy,
    startup_timeout: Option<Duration>,
}

impl SupervisorState {
    pub fn new(config: &ProcessConfig, now: Instant) -> Self {
        // External mode observes lifecycle only; the host owns supervision policy.
        let external = config.supervisor == crate::config::SupervisionMode::External;

        let watchdog_timeout = if external {
            None
        } else {
            config
                .watchdog
                .as_ref()
                .map(|w| Duration::from_micros(w.usec))
        };
        let watchdog_require_ready = config.watchdog.as_ref().is_none_or(|w| w.require_ready);
        let startup_timeout = if external {
            None
        } else {
            config
                .ready
                .as_ref()
                .and_then(|r| r.timeout)
                .map(Duration::from_secs)
        };
        let restart_policy = if external {
            RestartPolicy::Never
        } else {
            config.restart.on
        };
        let restart_limit_burst = config.restart.max.unwrap_or(DEFAULT_RESTART_LIMIT_BURST);
        let restart_limit_interval = config.restart.window.map(Duration::from_secs);

        let readiness = if config.has_readiness_probe() {
            ReadinessState::Pending
        } else {
            ReadinessState::NotRequired
        };
        let mut state = Self {
            restart_timestamps: VecDeque::new(),
            restart_limit_burst,
            restart_limit_interval,
            watchdog_armed: false,
            watchdog_deadline: None,
            startup_deadline: startup_timeout.map(|d: Duration| now + d),
            status: ProcessStatus {
                restart_count: 0,
                target: TargetState::Running,
                transition: if readiness == ReadinessState::Pending {
                    Some(StateTransition::Launching)
                } else {
                    None
                },
                child: ChildState::Running,
                readiness,
                restart: RestartDecision::None,
            },
            watchdog_timeout,
            watchdog_require_ready,
            readiness_required: config.has_readiness_probe(),
            restart_policy,
            startup_timeout,
        };

        state.arm_initial_watchdog(now);
        state
    }

    pub fn on_event(&mut self, event: Event, now: Instant) -> Action {
        // Stop is idempotent and wins from every state.
        if event == Event::StopRequested {
            self.status.target = TargetState::Stopped(StopReason::User);
            self.status.restart = match self.status.restart {
                RestartDecision::Pending => RestartDecision::None,
                other => other,
            };
            self.status.transition = if self.status.child == ChildState::Running {
                Some(StateTransition::Terminating)
            } else {
                None
            };
            self.watchdog_armed = false;
            self.watchdog_deadline = None;
            self.startup_deadline = None;
            return Action::None;
        }

        // Ignore events while the caller tears the job down.
        if self.status.transition == Some(StateTransition::Terminating) {
            return Action::None;
        }

        // File changes may revive a process after policy restarts give up.
        if self.status.restart == RestartDecision::Exhausted && event != Event::FileChange {
            return Action::None;
        }

        match event {
            Event::Ready => {
                self.watchdog_armed = true;
                self.startup_deadline = None;
                if let Some(timeout) = self.watchdog_timeout {
                    self.watchdog_deadline = Some(now + timeout);
                }
                self.status.readiness = ReadinessState::Ready;
                self.status.transition = None;
                Action::None
            }
            Event::WatchdogPing => {
                if let Some(timeout) = self.watchdog_timeout {
                    self.watchdog_deadline = Some(now + timeout);
                    self.startup_deadline = None;
                    if !self.watchdog_require_ready {
                        self.watchdog_armed = true;
                        self.status.readiness = ReadinessState::Ready;
                        self.status.transition = None;
                    }
                }
                Action::None
            }
            Event::WatchdogTrigger => self.try_restart(now, RestartTrigger::WatchdogTrigger),
            Event::WatchdogTimeout => self.try_restart(now, RestartTrigger::WatchdogTimeout),
            Event::StartupTimeout => self.try_restart(now, RestartTrigger::StartupTimeout),
            Event::ExtendTimeout { usec } => {
                if self.status.readiness == ReadinessState::Pending
                    && let Some(deadline) = self.startup_deadline.as_mut()
                {
                    *deadline += Duration::from_micros(usec);
                }
                Action::None
            }
            Event::ProcessExit { status } => {
                self.status.child = ChildState::Exited(status);
                self.status.readiness = ReadinessState::Inactive;
                self.status.transition = None;
                self.watchdog_armed = false;
                self.watchdog_deadline = None;
                self.startup_deadline = None;
                let should_restart = match self.restart_policy {
                    RestartPolicy::Never => false,
                    RestartPolicy::Always => true,
                    RestartPolicy::OnFailure => status == ExitOutcome::Failure,
                };
                if !should_restart {
                    self.status.restart = RestartDecision::None;
                    return Action::None;
                }
                self.try_restart(now, RestartTrigger::ProcessExit)
            }
            Event::FileChange => {
                self.status.target = TargetState::Running;
                self.status.restart = RestartDecision::None;
                self.status.transition = Some(StateTransition::Replacing);
                Action::Restart
            }
            Event::StopRequested => {
                tracing::error!(
                    "stop event reached the post-stop reducer; retaining stopped state"
                );
                Action::None
            }
        }
    }

    /// Reset after an explicit restart, including its restart budget.
    pub fn reset_for_explicit_restart(&mut self, now: Instant) {
        self.restart_timestamps.clear();
        self.status.restart_count = 0;
        self.status.target = TargetState::Running;
        self.status.child = ChildState::Running;
        self.status.restart = RestartDecision::None;
        self.watchdog_armed = false;
        self.watchdog_deadline = None;
        self.reset_readiness(now, StateTransition::Replacing);
        self.arm_initial_watchdog(now);
    }

    /// Reset state after a restart completes.
    pub fn on_restart_complete(&mut self, now: Instant) {
        self.status.restart_count = match self.status.restart_count.checked_add(1) {
            Some(count) => count,
            None => {
                tracing::error!("process restart count exhausted; retaining maximum count");
                u64::MAX
            }
        };
        self.status.target = TargetState::Running;
        self.status.child = ChildState::Running;
        self.status.restart = RestartDecision::None;
        self.watchdog_armed = false;
        self.watchdog_deadline = None;
        self.reset_readiness(now, StateTransition::Replacing);
        self.arm_initial_watchdog(now);
    }

    pub fn on_termination_complete(&mut self) {
        self.status.child = ChildState::Terminated;
        self.status.readiness = ReadinessState::Inactive;
        self.status.transition = None;
        self.watchdog_armed = false;
        self.watchdog_deadline = None;
        self.startup_deadline = None;
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

    pub fn restart_count(&self) -> u64 {
        self.status.restart_count
    }

    pub fn is_ready(&self) -> bool {
        self.status.is_ready()
    }

    pub fn phase(&self) -> ProcessPhase {
        self.status.display_phase()
    }

    pub fn status(&self) -> JobStatus {
        self.status
    }

    fn try_restart(&mut self, now: Instant, trigger: RestartTrigger) -> Action {
        if self.can_restart(now) {
            if self.status.child == ChildState::Running {
                self.status.transition = Some(StateTransition::Replacing);
                self.status.restart = RestartDecision::None;
            } else {
                self.status.transition = None;
                self.status.restart = RestartDecision::Pending;
            }
            Action::Restart
        } else {
            self.status.restart = RestartDecision::Exhausted;
            self.status.transition = if self.status.child == ChildState::Running {
                Some(StateTransition::Terminating)
            } else {
                None
            };
            Action::GiveUp {
                reason: trigger.give_up_reason(),
            }
        }
    }

    /// Check and record a restart against the rate limit. A window (`Some`) expires old
    /// timestamps so the budget refills; a lifetime limit (`None`) never does.
    fn can_restart(&mut self, now: Instant) -> bool {
        if let Some(interval) = self.restart_limit_interval {
            let cutoff = now - interval;
            while self.restart_timestamps.front().is_some_and(|&t| t < cutoff) {
                self.restart_timestamps.pop_front();
            }
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

    fn reset_readiness(&mut self, now: Instant, transition: StateTransition) {
        if self.readiness_required {
            self.status.readiness = ReadinessState::Pending;
            self.status.transition = Some(transition);
            self.startup_deadline = self.startup_timeout.map(|duration| now + duration);
        } else {
            self.status.readiness = ReadinessState::NotRequired;
            self.status.transition = None;
            self.startup_deadline = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ReadyConfig, RestartConfig, WatchdogConfig};

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
            restart: RestartConfig {
                on: policy,
                ..Default::default()
            },
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
            restart: RestartConfig {
                on: policy,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn config_with_startup_timeout(secs: u64) -> ProcessConfig {
        ProcessConfig {
            ready: Some(ReadyConfig {
                timeout: Some(secs),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn config_with_startup_and_watchdog(
        startup_secs: u64,
        watchdog_usec: u64,
        require_ready: bool,
    ) -> ProcessConfig {
        ProcessConfig {
            ready: Some(ReadyConfig {
                timeout: Some(startup_secs),
                ..Default::default()
            }),
            watchdog: Some(WatchdogConfig {
                usec: watchdog_usec,
                require_ready,
            }),
            ..Default::default()
        }
    }

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
        let _ = state.on_event(Event::WatchdogPing, later);

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
    fn watchdog_ping_without_watchdog_configured_is_noop() {
        let now = Instant::now();
        let config = config_with_startup_timeout(5);
        let mut state = SupervisorState::new(&config, now);

        let startup_before = state.startup_deadline;
        let _ = state.on_event(Event::WatchdogPing, now + Duration::from_secs(1));

        // Startup deadline must NOT be cleared — no watchdog is configured
        assert_eq!(state.startup_deadline, startup_before);
        assert_eq!(state.phase(), ProcessPhase::Starting);
        assert!(!state.watchdog_armed);
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
        assert_eq!(state.phase(), ProcessPhase::Stopping);
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

    #[test]
    fn startup_timeout_triggers_restart() {
        let now = Instant::now();
        let config = config_with_startup_timeout(5);
        let mut state = SupervisorState::new(&config, now);

        let action = state.on_event(Event::StartupTimeout, now + Duration::from_secs(5));
        assert_eq!(action, Action::Restart);
    }

    #[test]
    fn startup_timeout_cleared_by_ready() {
        let now = Instant::now();
        let config = config_with_startup_and_watchdog(5, 1_000_000, true);
        let mut state = SupervisorState::new(&config, now);
        assert!(state.startup_deadline.is_some());

        let _ = state.on_event(Event::Ready, now + Duration::from_secs(1));
        assert!(state.startup_deadline.is_none());
    }

    #[test]
    fn startup_timeout_cleared_by_watchdog_ping() {
        let now = Instant::now();
        let config = config_with_startup_and_watchdog(5, 1_000_000, true);
        let mut state = SupervisorState::new(&config, now);
        assert!(state.startup_deadline.is_some());

        let _ = state.on_event(Event::WatchdogPing, now + Duration::from_secs(1));
        assert!(state.startup_deadline.is_none());
    }

    #[test]
    fn startup_timeout_respects_rate_limit() {
        let now = Instant::now();
        let config = config_with_startup_timeout(1);
        let mut state = SupervisorState::new(&config, now);

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
        let config = config_with_startup_timeout(5);
        let mut state = SupervisorState::new(&config, now);

        let original_deadline = state.startup_deadline.unwrap();
        let _ = state.on_event(Event::ExtendTimeout { usec: 3_000_000 }, now);
        assert_eq!(
            state.startup_deadline,
            Some(original_deadline + Duration::from_secs(3))
        );
    }

    #[test]
    fn extend_timeout_ignored_when_not_starting() {
        let now = Instant::now();
        let config = config_with_startup_and_watchdog(5, 1_000_000, true);
        let mut state = SupervisorState::new(&config, now);

        // Move to Ready phase
        let _ = state.on_event(Event::Ready, now);
        let deadline_before = state.startup_deadline;

        let _ = state.on_event(Event::ExtendTimeout { usec: 3_000_000 }, now);
        assert_eq!(state.startup_deadline, deadline_before);
    }

    #[test]
    fn next_deadline_returns_earlier_of_startup_and_watchdog() {
        let now = Instant::now();
        let config = config_with_startup_and_watchdog(3, 10_000_000, false);
        let mut state = SupervisorState::new(&config, now);
        // require_ready=false → watchdog armed from start

        let startup = now + Duration::from_secs(3);
        let watchdog = now + Duration::from_secs(10);

        // Startup is earlier
        assert_eq!(state.next_deadline(), Some(startup));

        // After extending startup past watchdog, watchdog is earlier
        let _ = state.on_event(
            Event::ExtendTimeout {
                usec: 10_000_000, // +10s
            },
            now,
        );
        assert_eq!(state.next_deadline(), Some(watchdog));
    }

    #[test]
    fn restarts_within_window_are_allowed() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        for i in 0..5 {
            let t = now + Duration::from_millis(i * 10);
            let action = state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure,
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
            let _ = state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure,
                },
                t,
            );
            state.on_restart_complete(t);
        }

        let t = now + Duration::from_millis(100);
        let action = state.on_event(
            Event::ProcessExit {
                status: ExitOutcome::Failure,
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
            let _ = state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure,
                },
                t,
            );
            state.on_restart_complete(t);
        }

        // Jump past the 10s window — old timestamps expire
        let later = now + Duration::from_secs(15);
        let action = state.on_event(
            Event::ProcessExit {
                status: ExitOutcome::Failure,
            },
            later,
        );
        assert_eq!(action, Action::Restart);
    }

    #[test]
    fn healthy_service_can_crash_and_restart_again() {
        let now = Instant::now();
        // Needs an explicit window; the default is now a lifetime limit.
        let config = ProcessConfig {
            restart: RestartConfig {
                on: RestartPolicy::Always,
                window: Some(10),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut state = SupervisorState::new(&config, now);

        // Exhaust the burst limit
        for i in 0..5 {
            let t = now + Duration::from_millis(i * 10);
            let _ = state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure,
                },
                t,
            );
            state.on_restart_complete(t);
        }

        // After running healthy for a long time, crash again — should be allowed
        let much_later = now + Duration::from_secs(60);
        let action = state.on_event(
            Event::ProcessExit {
                status: ExitOutcome::Failure,
            },
            much_later,
        );
        assert_eq!(action, Action::Restart);
    }

    #[test]
    fn lifetime_limit_never_refills_regardless_of_elapsed_time() {
        let now = Instant::now();
        // Default config: max = 5, window = None (lifetime limit).
        let config = config_with_policy(RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        // Exhaust the lifetime budget of 5 restarts
        for i in 0..5 {
            let t = now + Duration::from_millis(i * 10);
            let _ = state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure,
                },
                t,
            );
            state.on_restart_complete(t);
        }

        // Even after a long wait the budget never refills — the process gives up.
        let much_later = now + Duration::from_secs(60);
        let action = state.on_event(
            Event::ProcessExit {
                status: ExitOutcome::Failure,
            },
            much_later,
        );
        assert!(matches!(action, Action::GiveUp { .. }));
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
                    status: ExitOutcome::Failure,
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

    #[test]
    fn policy_never_does_not_restart() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Never);
        let mut state = SupervisorState::new(&config, now);

        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Success,
                },
                now,
            ),
            Action::None
        );

        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure,
                },
                now,
            ),
            Action::None
        );
    }

    /// A non-restarting process exit satisfies `@completed` dependencies.
    #[test]
    fn policy_never_transitions_to_exited() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Never);
        let mut state = SupervisorState::new(&config, now);
        assert_eq!(state.phase(), ProcessPhase::Ready);

        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Success,
                },
                now,
            ),
            Action::None
        );
        assert_eq!(state.phase(), ProcessPhase::Exited);
        assert_eq!(
            state.status().child.exit_outcome(),
            Some(ExitOutcome::Success)
        );
    }

    /// Same regression test for on_failure policy with a successful exit.
    #[test]
    fn policy_on_failure_transitions_to_exited_on_success() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::OnFailure);
        let mut state = SupervisorState::new(&config, now);

        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Success,
                },
                now,
            ),
            Action::None
        );
        assert_eq!(state.phase(), ProcessPhase::Exited);
    }

    #[test]
    fn policy_always_restarts_on_success_and_failure() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Success,
                },
                now,
            ),
            Action::Restart
        );
        state.on_restart_complete(now);

        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure,
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
                    status: ExitOutcome::Success,
                },
                now,
            ),
            Action::None
        );

        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure,
                },
                now,
            ),
            Action::Restart
        );
    }

    #[test]
    fn phase_starting_to_ready_via_ready() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, true);
        let mut state = SupervisorState::new(&config, now);

        assert_eq!(state.phase(), ProcessPhase::Ready);
        let _ = state.on_event(Event::Ready, now);
        assert_eq!(state.phase(), ProcessPhase::Ready);
        assert!(state.is_ready());
    }

    #[test]
    fn phase_starting_to_ready_via_watchdog_ping_when_not_require_ready() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, false);
        let mut state = SupervisorState::new(&config, now);

        assert_eq!(state.phase(), ProcessPhase::Ready);
        let _ = state.on_event(Event::WatchdogPing, now);
        assert_eq!(state.phase(), ProcessPhase::Ready);
        assert!(state.is_ready());
    }

    #[test]
    fn phase_to_gave_up_via_rate_limit() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        for i in 0..5 {
            let t = now + Duration::from_millis(i * 10);
            let _ = state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure,
                },
                t,
            );
            state.on_restart_complete(t);
        }

        let t = now + Duration::from_millis(100);
        let _ = state.on_event(
            Event::ProcessExit {
                status: ExitOutcome::Failure,
            },
            t,
        );
        assert_eq!(state.phase(), ProcessPhase::GaveUp);
    }

    #[test]
    fn events_after_gave_up_are_ignored() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        // Exhaust rate limit to reach GaveUp
        for i in 0..5 {
            let t = now + Duration::from_millis(i * 10);
            let _ = state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure,
                },
                t,
            );
            state.on_restart_complete(t);
        }
        let t = now + Duration::from_millis(100);
        let _ = state.on_event(
            Event::ProcessExit {
                status: ExitOutcome::Failure,
            },
            t,
        );
        assert_eq!(state.phase(), ProcessPhase::GaveUp);

        // All events should be ignored
        assert_eq!(state.on_event(Event::Ready, t), Action::None);
        assert_eq!(state.phase(), ProcessPhase::GaveUp);

        assert_eq!(state.on_event(Event::WatchdogPing, t), Action::None);
        assert_eq!(state.on_event(Event::WatchdogTrigger, t), Action::None);
        assert_eq!(state.on_event(Event::WatchdogTimeout, t), Action::None);
        assert_eq!(state.on_event(Event::StartupTimeout, t), Action::None);
        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure,
                },
                t,
            ),
            Action::None
        );
        assert_eq!(state.phase(), ProcessPhase::GaveUp);
    }

    #[test]
    fn on_restart_complete_resets_all_transient_state() {
        let now = Instant::now();
        let config = config_with_startup_and_watchdog(5, 1_000_000, true);
        let mut state = SupervisorState::new(&config, now);

        // Move to Ready, arm watchdog
        let _ = state.on_event(Event::Ready, now);
        assert_eq!(state.phase(), ProcessPhase::Ready);
        assert!(state.watchdog_armed);
        assert!(state.startup_deadline.is_none());

        // Restart
        let t = now + Duration::from_millis(100);
        state.on_restart_complete(t);

        assert_eq!(state.phase(), ProcessPhase::Starting);
        assert!(!state.watchdog_armed);
        assert!(state.watchdog_deadline.is_none());
        assert_eq!(state.startup_deadline, Some(t + Duration::from_secs(5)));
        assert_eq!(state.restart_count(), 1);
    }

    #[test]
    fn reset_for_explicit_restart_clears_gave_up_phase() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        for i in 0..5 {
            let t = now + Duration::from_millis(i * 10);
            let _ = state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure,
                },
                t,
            );
            state.on_restart_complete(t);
        }
        let t = now + Duration::from_millis(100);
        let _ = state.on_event(
            Event::ProcessExit {
                status: ExitOutcome::Failure,
            },
            t,
        );
        assert_eq!(state.phase(), ProcessPhase::GaveUp);
        assert!(state.restart_count() > 0);

        let now2 = now + Duration::from_secs(1);
        state.reset_for_explicit_restart(now2);

        assert_eq!(state.phase(), ProcessPhase::Ready);
        assert_eq!(state.restart_count(), 0);
        assert!(state.restart_timestamps.is_empty());

        let action = state.on_event(
            Event::ProcessExit {
                status: ExitOutcome::Failure,
            },
            now2,
        );
        assert_eq!(action, Action::Restart);
    }

    #[test]
    fn reset_for_explicit_restart_rearms_watchdog_when_not_require_ready() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, false);
        let mut state = SupervisorState::new(&config, now);

        let _ = state.on_event(Event::Ready, now);
        assert_eq!(state.phase(), ProcessPhase::Ready);

        let later = now + Duration::from_secs(2);
        state.reset_for_explicit_restart(later);

        assert_eq!(state.phase(), ProcessPhase::Ready);
        assert!(state.watchdog_armed);
        assert_eq!(
            state.watchdog_deadline,
            Some(later + Duration::from_secs(1))
        );
    }

    #[test]
    fn reset_for_explicit_restart_holds_watchdog_when_require_ready() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, true);
        let mut state = SupervisorState::new(&config, now);

        let _ = state.on_event(Event::Ready, now);
        assert!(state.watchdog_armed);

        let later = now + Duration::from_secs(2);
        state.reset_for_explicit_restart(later);

        assert_eq!(state.phase(), ProcessPhase::Ready);
        assert!(!state.watchdog_armed);
        assert!(state.watchdog_deadline.is_none());
    }

    #[test]
    fn reset_for_explicit_restart_sets_startup_deadline() {
        let now = Instant::now();
        let config = config_with_startup_timeout(7);
        let mut state = SupervisorState::new(&config, now);

        let _ = state.on_event(Event::Ready, now);
        assert!(state.startup_deadline.is_none());

        let later = now + Duration::from_secs(5);
        state.reset_for_explicit_restart(later);

        assert_eq!(state.startup_deadline, Some(later + Duration::from_secs(7)));
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
        assert_eq!(state.phase(), ProcessPhase::Ready);
    }

    #[test]
    fn stop_requested_from_starting_transitions_to_stopping() {
        let now = Instant::now();
        let config = config_default();
        let mut state = SupervisorState::new(&config, now);

        let action = state.on_event(Event::StopRequested, now);
        assert_eq!(action, Action::None);
        assert_eq!(state.phase(), ProcessPhase::Stopping);
    }

    #[test]
    fn stop_requested_from_ready_transitions_to_stopping() {
        let now = Instant::now();
        let config = config_with_watchdog(1_000_000, true);
        let mut state = SupervisorState::new(&config, now);

        let _ = state.on_event(Event::Ready, now);
        assert_eq!(state.phase(), ProcessPhase::Ready);

        let action = state.on_event(Event::StopRequested, now);
        assert_eq!(action, Action::None);
        assert_eq!(state.phase(), ProcessPhase::Stopping);
    }

    #[test]
    fn stop_requested_from_gave_up_transitions_to_stopping() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        for i in 0..5 {
            let t = now + Duration::from_millis(i * 10);
            let _ = state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure,
                },
                t,
            );
            state.on_restart_complete(t);
        }
        let t = now + Duration::from_millis(100);
        let _ = state.on_event(
            Event::ProcessExit {
                status: ExitOutcome::Failure,
            },
            t,
        );
        assert_eq!(state.phase(), ProcessPhase::GaveUp);

        let action = state.on_event(Event::StopRequested, t);
        assert_eq!(action, Action::None);
        assert_eq!(state.phase(), ProcessPhase::Stopped);
    }

    #[test]
    fn stop_requested_clears_deadlines_and_disarms_watchdog() {
        let now = Instant::now();
        let config = config_with_startup_and_watchdog(5, 1_000_000, false);
        let mut state = SupervisorState::new(&config, now);

        assert!(state.watchdog_armed);
        assert!(state.startup_deadline.is_some());
        assert!(state.next_deadline().is_some());

        let _ = state.on_event(Event::StopRequested, now);

        assert!(!state.watchdog_armed);
        assert!(state.watchdog_deadline.is_none());
        assert!(state.startup_deadline.is_none());
        assert!(state.next_deadline().is_none());
    }

    #[test]
    fn events_after_stopping_are_ignored() {
        let now = Instant::now();
        let config = config_with_policy(RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        let _ = state.on_event(Event::StopRequested, now);
        assert_eq!(state.phase(), ProcessPhase::Stopping);

        assert_eq!(state.on_event(Event::Ready, now), Action::None);
        assert_eq!(state.phase(), ProcessPhase::Stopping);

        assert_eq!(state.on_event(Event::WatchdogPing, now), Action::None);
        assert_eq!(state.on_event(Event::WatchdogTrigger, now), Action::None);
        assert_eq!(state.on_event(Event::WatchdogTimeout, now), Action::None);
        assert_eq!(state.on_event(Event::StartupTimeout, now), Action::None);
        assert_eq!(state.on_event(Event::FileChange, now), Action::None);
        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure,
                },
                now,
            ),
            Action::None
        );
        assert_eq!(state.phase(), ProcessPhase::Stopping);
    }

    #[test]
    fn stop_requested_is_idempotent() {
        let now = Instant::now();
        let config = config_default();
        let mut state = SupervisorState::new(&config, now);

        assert_eq!(state.on_event(Event::StopRequested, now), Action::None);
        assert_eq!(state.on_event(Event::StopRequested, now), Action::None);
        assert_eq!(state.phase(), ProcessPhase::Stopping);
    }

    #[test]
    fn status_snapshot_reflects_state() {
        let now = Instant::now();
        let config = config_watchdog_and_policy(1_000_000, true, RestartPolicy::Always);
        let mut state = SupervisorState::new(&config, now);

        let status = state.status();
        assert_eq!(status.display_phase(), ProcessPhase::Ready);
        assert_eq!(status.restart_count, 0);
        assert_eq!(status.child.exit_outcome(), None);
        assert!(status.is_ready());

        let _ = state.on_event(Event::Ready, now);
        let status = state.status();
        assert_eq!(status.display_phase(), ProcessPhase::Ready);
        assert!(status.is_ready());

        let t = now + Duration::from_millis(10);
        let _ = state.on_event(
            Event::ProcessExit {
                status: ExitOutcome::Failure,
            },
            t,
        );
        assert_eq!(
            state.status().child.exit_outcome(),
            Some(ExitOutcome::Failure)
        );
        state.on_restart_complete(t);
        let status = state.status();
        assert_eq!(status.display_phase(), ProcessPhase::Ready);
        assert_eq!(status.restart_count, 1);
        assert_eq!(status.child.exit_outcome(), None);
        assert!(status.is_ready());
    }

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
            let _ = state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure,
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
                    status: ExitOutcome::Failure,
                },
                t,
            ),
            Action::GiveUp { .. }
        ));

        // File change still works
        assert_eq!(state.on_event(Event::FileChange, t), Action::Restart);
    }

    #[test]
    fn config_startup_timeout_sets_initial_deadline() {
        let now = Instant::now();
        let config = config_with_startup_timeout(30);
        let state = SupervisorState::new(&config, now);

        assert_eq!(state.startup_deadline, Some(now + Duration::from_secs(30)));
        assert_eq!(state.startup_timeout, Some(Duration::from_secs(30)));
    }

    #[test]
    fn config_restart_max_controls_burst() {
        let now = Instant::now();
        let config = ProcessConfig {
            restart: RestartConfig {
                on: RestartPolicy::Always,
                max: Some(7),
                window: None,
            },
            ..Default::default()
        };
        let mut state = SupervisorState::new(&config, now);

        // Should allow 7 restarts
        for i in 0..7 {
            let t = now + Duration::from_millis(i * 10);
            assert_eq!(
                state.on_event(
                    Event::ProcessExit {
                        status: ExitOutcome::Failure
                    },
                    t
                ),
                Action::Restart
            );
            state.on_restart_complete(t);
        }
        let t = now + Duration::from_millis(100);
        assert!(matches!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure
                },
                t
            ),
            Action::GiveUp { .. }
        ));
    }

    #[test]
    fn config_restart_window_controls_interval() {
        let now = Instant::now();
        let config = ProcessConfig {
            restart: RestartConfig {
                on: RestartPolicy::Always,
                max: Some(3),
                window: Some(2), // 2 second window
            },
            ..Default::default()
        };
        let mut state = SupervisorState::new(&config, now);

        // Use up all 3 slots
        for i in 0..3 {
            let t = now + Duration::from_millis(i * 10);
            assert_eq!(
                state.on_event(
                    Event::ProcessExit {
                        status: ExitOutcome::Failure
                    },
                    t
                ),
                Action::Restart
            );
            state.on_restart_complete(t);
        }

        // 4th within 2s window — denied
        let t = now + Duration::from_secs(1);
        assert!(matches!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure
                },
                t
            ),
            Action::GiveUp { .. }
        ));
    }

    #[test]
    fn config_restart_window_expires() {
        let now = Instant::now();
        let config = ProcessConfig {
            restart: RestartConfig {
                on: RestartPolicy::Always,
                max: Some(3),
                window: Some(2), // 2 second window
            },
            ..Default::default()
        };
        let mut state = SupervisorState::new(&config, now);

        // Use up all 3 slots
        for i in 0..3 {
            let t = now + Duration::from_millis(i * 10);
            let _ = state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure,
                },
                t,
            );
            state.on_restart_complete(t);
        }

        // After 3s (past the 2s window), old timestamps expire — restart allowed
        let t = now + Duration::from_secs(3);
        assert_eq!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure
                },
                t
            ),
            Action::Restart
        );
    }

    #[test]
    fn config_restart_max_as_burst_limit() {
        let now = Instant::now();
        let config = ProcessConfig {
            restart: RestartConfig {
                on: RestartPolicy::Always,
                max: Some(2),
                window: None,
            },
            ..Default::default()
        };
        let mut state = SupervisorState::new(&config, now);

        for i in 0..2 {
            let t = now + Duration::from_millis(i * 10);
            assert_eq!(
                state.on_event(
                    Event::ProcessExit {
                        status: ExitOutcome::Failure
                    },
                    t
                ),
                Action::Restart
            );
            state.on_restart_complete(t);
        }
        let t = now + Duration::from_millis(50);
        assert!(matches!(
            state.on_event(
                Event::ProcessExit {
                    status: ExitOutcome::Failure
                },
                t
            ),
            Action::GiveUp { .. }
        ));
    }
}
