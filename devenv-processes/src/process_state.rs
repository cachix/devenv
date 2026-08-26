//! Compact, coherent process status.
//!
//! Lifecycle decisions use these independent facts. [`ProcessPhase`] is only
//! the serialized CLI/API projection of a coherent status value.

use crate::manager::ProcessPhase;
use devenv_activity::ProcessStatus as ActivityProcessStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitOutcome {
    Success,
    Failure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetState {
    Running,
    Stopped(StopReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopReason {
    NotRequested,
    User,
    ManagerShutdown,
    DependencyFailure,
    LaunchFailure,
}

#[cfg_attr(test, derive(strum::EnumIter))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateTransition {
    WaitingForDependencies,
    Launching,
    Replacing,
    Terminating,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildState {
    NeverSpawned,
    Running,
    Exited(ExitOutcome),
    Terminated,
}

impl ChildState {
    pub fn was_spawned(self) -> bool {
        self != Self::NeverSpawned
    }

    pub fn is_exited(self) -> bool {
        matches!(self, Self::Exited(_))
    }

    pub fn exit_outcome(self) -> Option<ExitOutcome> {
        match self {
            Self::Exited(status) => Some(status),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadinessState {
    Inactive,
    NotRequired,
    Pending,
    Ready,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestartDecision {
    None,
    Pending,
    Exhausted,
}

/// The complete hot-path status for one process.
///
/// The representation is deliberately left to Rust so the compiler can pack
/// the facts and their small payloads without freezing an internal ABI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessStatus {
    pub restart_count: u64,
    pub target: TargetState,
    pub transition: Option<StateTransition>,
    pub child: ChildState,
    pub readiness: ReadinessState,
    pub restart: RestartDecision,
}

impl ProcessStatus {
    pub fn running(readiness_required: bool, transition: StateTransition) -> Self {
        if !matches!(
            transition,
            StateTransition::Launching | StateTransition::Replacing
        ) {
            tracing::error!(
                ?transition,
                "invalid running-process transition; resetting status"
            );
            return Self::stopped(StopReason::LaunchFailure, ChildState::NeverSpawned);
        }
        Self {
            restart_count: 0,
            target: TargetState::Running,
            transition: if readiness_required {
                Some(transition)
            } else {
                None
            },
            child: ChildState::Running,
            readiness: if readiness_required {
                ReadinessState::Pending
            } else {
                ReadinessState::NotRequired
            },
            restart: RestartDecision::None,
        }
    }

    pub const fn not_started() -> Self {
        Self {
            restart_count: 0,
            target: TargetState::Stopped(StopReason::NotRequested),
            transition: None,
            child: ChildState::NeverSpawned,
            readiness: ReadinessState::Inactive,
            restart: RestartDecision::None,
        }
    }

    pub const fn waiting() -> Self {
        Self {
            restart_count: 0,
            target: TargetState::Running,
            transition: Some(StateTransition::WaitingForDependencies),
            child: ChildState::NeverSpawned,
            readiness: ReadinessState::Inactive,
            restart: RestartDecision::None,
        }
    }

    pub const fn stopped(reason: StopReason, child: ChildState) -> Self {
        Self {
            restart_count: 0,
            target: TargetState::Stopped(reason),
            transition: None,
            child,
            readiness: ReadinessState::Inactive,
            restart: RestartDecision::None,
        }
    }

    pub fn is_ready(self) -> bool {
        self.target == TargetState::Running
            && self.transition.is_none()
            && self.child == ChildState::Running
            && matches!(
                self.readiness,
                ReadinessState::Ready | ReadinessState::NotRequired
            )
    }

    pub fn is_settled(self) -> bool {
        self.transition.is_none()
            && self.child != ChildState::Running
            && self.restart != RestartDecision::Pending
    }

    /// Internal activity/TUI projection. Unlike `display_phase`, this keeps
    /// activity-only distinctions such as Starting, Running, and Restarting
    /// and never routes lifecycle decisions through the compatibility phase.
    pub fn activity_status(self) -> ActivityProcessStatus {
        if !self.is_valid() {
            tracing::error!(status = ?self, "invalid process status at activity boundary; using stopped fallback");
            return ActivityProcessStatus::Stopped;
        }

        if self.transition == Some(StateTransition::Terminating) {
            return ActivityProcessStatus::Stopping;
        }
        if self.transition == Some(StateTransition::WaitingForDependencies) {
            return ActivityProcessStatus::Waiting;
        }
        if self.transition == Some(StateTransition::Replacing)
            || self.restart == RestartDecision::Pending
        {
            return ActivityProcessStatus::Restarting;
        }
        if self.transition == Some(StateTransition::Launching) {
            return ActivityProcessStatus::Starting;
        }

        if self.target == TargetState::Running && self.child == ChildState::Running {
            return match self.readiness {
                ReadinessState::Ready => ActivityProcessStatus::Ready,
                ReadinessState::NotRequired => ActivityProcessStatus::Running,
                ReadinessState::Pending => ActivityProcessStatus::Starting,
                ReadinessState::Inactive => {
                    tracing::error!(status = ?self, "running child has inactive readiness; using stopped fallback");
                    ActivityProcessStatus::Stopped
                }
            };
        }

        match (self.target, self.child, self.restart) {
            (TargetState::Stopped(StopReason::User), _, _) => ActivityProcessStatus::Stopped,
            (_, ChildState::Exited(_) | ChildState::Terminated, RestartDecision::Exhausted) => {
                ActivityProcessStatus::GaveUp
            }
            (_, ChildState::Exited(_), _) => ActivityProcessStatus::Exited,
            (TargetState::Stopped(StopReason::NotRequested), ChildState::NeverSpawned, _) => {
                ActivityProcessStatus::NotStarted
            }
            (TargetState::Stopped(_), ChildState::NeverSpawned | ChildState::Terminated, _) => {
                ActivityProcessStatus::Stopped
            }
            (TargetState::Running, ChildState::NeverSpawned | ChildState::Terminated, _) => {
                ActivityProcessStatus::NotStarted
            }
            _ => {
                tracing::error!(status = ?self, "incomplete process activity projection; using stopped fallback");
                ActivityProcessStatus::Stopped
            }
        }
    }

    /// Executable definition of the valid compact state space.
    pub fn is_valid(self) -> bool {
        let transition = match self.transition {
            None => true,
            Some(StateTransition::WaitingForDependencies) => {
                self.target == TargetState::Running
                    && self.child == ChildState::NeverSpawned
                    && self.readiness == ReadinessState::Inactive
                    && self.restart == RestartDecision::None
            }
            Some(StateTransition::Launching | StateTransition::Replacing) => {
                self.target == TargetState::Running
            }
            Some(StateTransition::Terminating) => {
                matches!(self.target, TargetState::Stopped(_))
                    || (self.target == TargetState::Running
                        && self.child == ChildState::Running
                        && self.restart == RestartDecision::Exhausted)
            }
        };
        let stopped_running = !matches!(self.target, TargetState::Stopped(_))
            || self.child != ChildState::Running
            || self.transition == Some(StateTransition::Terminating);
        let readiness = if self.child == ChildState::Running {
            self.readiness != ReadinessState::Inactive
        } else {
            self.readiness == ReadinessState::Inactive
        };
        let restart = match self.restart {
            RestartDecision::None => true,
            RestartDecision::Pending => {
                self.target == TargetState::Running
                    && self.child.is_exited()
                    && self.transition.is_none()
            }
            RestartDecision::Exhausted => {
                (self.child == ChildState::Running
                    && self.transition == Some(StateTransition::Terminating))
                    || (matches!(self.child, ChildState::Exited(_) | ChildState::Terminated)
                        && self.transition.is_none())
            }
        };
        let not_requested = self.target != TargetState::Stopped(StopReason::NotRequested)
            || (self.transition.is_none()
                && self.child == ChildState::NeverSpawned
                && self.readiness == ReadinessState::Inactive
                && self.restart == RestartDecision::None);
        let terminated_is_settled =
            self.child != ChildState::Terminated || self.transition.is_none();

        transition
            && stopped_running
            && readiness
            && restart
            && not_requested
            && terminated_is_settled
    }

    /// Compatibility projection used only when constructing external status.
    pub fn display_phase(self) -> ProcessPhase {
        if !self.is_valid() {
            tracing::error!(status = ?self, "invalid process status at display boundary; using stopped fallback");
            return ProcessPhase::Stopped;
        }

        if self.target == TargetState::Running
            && self.transition.is_none()
            && self.child == ChildState::Running
            && matches!(
                self.readiness,
                ReadinessState::Ready | ReadinessState::NotRequired
            )
        {
            return ProcessPhase::Ready;
        }

        if self.transition == Some(StateTransition::Terminating) {
            return ProcessPhase::Stopping;
        }

        if self.transition == Some(StateTransition::WaitingForDependencies) {
            return ProcessPhase::Waiting;
        }

        if matches!(
            self.transition,
            Some(StateTransition::Launching | StateTransition::Replacing)
        ) || self.restart == RestartDecision::Pending
            || (self.child == ChildState::Running && self.readiness == ReadinessState::Pending)
        {
            return ProcessPhase::Starting;
        }

        match (self.target, self.child, self.restart) {
            (TargetState::Stopped(StopReason::User), _, _) => ProcessPhase::Stopped,
            (_, ChildState::Exited(_) | ChildState::Terminated, RestartDecision::Exhausted) => {
                ProcessPhase::GaveUp
            }
            (_, ChildState::Exited(_), _) => ProcessPhase::Exited,
            (TargetState::Stopped(StopReason::NotRequested), ChildState::NeverSpawned, _) => {
                ProcessPhase::NotStarted
            }
            (TargetState::Stopped(_), ChildState::NeverSpawned | ChildState::Terminated, _) => {
                ProcessPhase::Stopped
            }
            (TargetState::Running, ChildState::NeverSpawned | ChildState::Terminated, _) => {
                ProcessPhase::NotStarted
            }
            (TargetState::Running, ChildState::Running, _) => ProcessPhase::Starting,
            _ => {
                tracing::error!(status = ?self, "incomplete process display projection; using stopped fallback");
                ProcessPhase::Stopped
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransitionCause {
    StartRequest,
    RestartRequest,
    StopRequest,
    AutomaticRestart,
}

macro_rules! generation_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub(crate) struct $name(u64);

        impl $name {
            pub(crate) const fn initial() -> Self {
                Self(0)
            }

            pub(crate) fn successor(self) -> Option<Self> {
                self.0.checked_add(1).map(Self)
            }

            pub(crate) fn next(&mut self) -> Option<Self> {
                let next = self.successor()?;
                *self = next;
                Some(*self)
            }
        }
    };
}

generation_id!(StartIntentId);
generation_id!(OperationId);
generation_id!(RunId);
generation_id!(DeadlineId);
generation_id!(WatcherId);
generation_id!(ListenerId);

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    const TARGETS: [TargetState; 6] = [
        TargetState::Running,
        TargetState::Stopped(StopReason::NotRequested),
        TargetState::Stopped(StopReason::User),
        TargetState::Stopped(StopReason::ManagerShutdown),
        TargetState::Stopped(StopReason::DependencyFailure),
        TargetState::Stopped(StopReason::LaunchFailure),
    ];
    const CHILDREN: [ChildState; 5] = [
        ChildState::NeverSpawned,
        ChildState::Running,
        ChildState::Exited(ExitOutcome::Success),
        ChildState::Exited(ExitOutcome::Failure),
        ChildState::Terminated,
    ];
    const READINESS: [ReadinessState; 4] = [
        ReadinessState::Inactive,
        ReadinessState::NotRequired,
        ReadinessState::Pending,
        ReadinessState::Ready,
    ];
    const RESTART: [RestartDecision; 3] = [
        RestartDecision::None,
        RestartDecision::Pending,
        RestartDecision::Exhausted,
    ];

    fn transitions() -> impl Iterator<Item = Option<StateTransition>> {
        std::iter::once(None).chain(StateTransition::iter().map(Some))
    }

    fn every_status() -> impl Iterator<Item = ProcessStatus> {
        TARGETS.into_iter().flat_map(move |target| {
            transitions().flat_map(move |transition| {
                CHILDREN.into_iter().flat_map(move |child| {
                    READINESS.into_iter().flat_map(move |readiness| {
                        RESTART.into_iter().map(move |restart| ProcessStatus {
                            restart_count: 7,
                            target,
                            transition,
                            child,
                            readiness,
                            restart,
                        })
                    })
                })
            })
        })
    }

    // This is intentionally a table-shaped oracle rather than a call to
    // `ProcessStatus::is_valid`: the exhaustive test below must be able to
    // detect an accidental widening or narrowing of the production predicate.
    fn contract_accepts(status: ProcessStatus) -> bool {
        let readiness_matches_child = match status.child {
            ChildState::Running => status.readiness != ReadinessState::Inactive,
            ChildState::NeverSpawned | ChildState::Exited(_) | ChildState::Terminated => {
                status.readiness == ReadinessState::Inactive
            }
        };

        let stopped_target_does_not_keep_a_child =
            !matches!(status.target, TargetState::Stopped(_))
                || status.child != ChildState::Running
                || status.transition == Some(StateTransition::Terminating);

        let transition_accepts = match status.transition {
            None => true,
            Some(StateTransition::WaitingForDependencies) => matches!(
                (
                    status.target,
                    status.child,
                    status.readiness,
                    status.restart
                ),
                (
                    TargetState::Running,
                    ChildState::NeverSpawned,
                    ReadinessState::Inactive,
                    RestartDecision::None
                )
            ),
            Some(StateTransition::Launching | StateTransition::Replacing) => {
                status.target == TargetState::Running
            }
            Some(StateTransition::Terminating) => {
                matches!(status.target, TargetState::Stopped(_))
                    || matches!(
                        (status.target, status.child, status.restart),
                        (
                            TargetState::Running,
                            ChildState::Running,
                            RestartDecision::Exhausted
                        )
                    )
            }
        };

        let restart_accepts = match status.restart {
            RestartDecision::None => true,
            RestartDecision::Pending => matches!(
                (status.target, status.transition, status.child),
                (TargetState::Running, None, ChildState::Exited(_))
            ),
            RestartDecision::Exhausted => {
                matches!(
                    (status.child, status.transition),
                    (ChildState::Running, Some(StateTransition::Terminating))
                ) || matches!(
                    (status.child, status.transition),
                    (ChildState::Exited(_) | ChildState::Terminated, None)
                )
            }
        };
        let not_requested_accepts = status.target != TargetState::Stopped(StopReason::NotRequested)
            || matches!(
                (
                    status.transition,
                    status.child,
                    status.readiness,
                    status.restart
                ),
                (
                    None,
                    ChildState::NeverSpawned,
                    ReadinessState::Inactive,
                    RestartDecision::None
                )
            );
        let terminated_is_settled =
            status.child != ChildState::Terminated || status.transition.is_none();

        readiness_matches_child
            && stopped_target_does_not_keep_a_child
            && transition_accepts
            && restart_accepts
            && not_requested_accepts
            && terminated_is_settled
    }

    #[test]
    fn validity_predicate_matches_the_independent_contract_for_every_status() {
        for status in every_status() {
            assert_eq!(
                status.is_valid(),
                contract_accepts(status),
                "status validity disagrees with the contract: {status:?}"
            );
        }
    }

    #[test]
    fn display_projection_is_total_for_every_valid_status() {
        let mut count = 0;
        for status in every_status().filter(|status| contract_accepts(*status)) {
            count += 1;
            let _ = status.display_phase();
        }
        assert_ne!(count, 0);
    }

    #[test]
    fn activity_projection_is_total_for_every_valid_status() {
        let mut count = 0;
        for status in every_status().filter(|status| contract_accepts(*status)) {
            count += 1;
            let _ = status.activity_status();
        }
        assert_ne!(count, 0);
    }

    #[test]
    fn activity_projection_preserves_internal_distinctions() {
        let cases = [
            (ProcessStatus::waiting(), ActivityProcessStatus::Waiting),
            (
                ProcessStatus::running(true, StateTransition::Launching),
                ActivityProcessStatus::Starting,
            ),
            (
                ProcessStatus::running(false, StateTransition::Launching),
                ActivityProcessStatus::Running,
            ),
            (
                ProcessStatus {
                    restart_count: 1,
                    target: TargetState::Running,
                    transition: Some(StateTransition::Replacing),
                    child: ChildState::Running,
                    readiness: ReadinessState::Ready,
                    restart: RestartDecision::None,
                },
                ActivityProcessStatus::Restarting,
            ),
        ];

        for (status, expected) in cases {
            assert!(status.is_valid(), "{status:?}");
            assert_eq!(status.activity_status(), expected, "{status:?}");
        }
    }

    #[test]
    fn display_precedence_is_exhaustive() {
        let cases = [
            (
                ProcessStatus {
                    restart_count: 3,
                    target: TargetState::Running,
                    transition: Some(StateTransition::Terminating),
                    child: ChildState::Running,
                    readiness: ReadinessState::Pending,
                    restart: RestartDecision::Exhausted,
                },
                ProcessPhase::Stopping,
            ),
            (
                ProcessStatus {
                    restart_count: 3,
                    target: TargetState::Running,
                    transition: None,
                    child: ChildState::Terminated,
                    readiness: ReadinessState::Inactive,
                    restart: RestartDecision::Exhausted,
                },
                ProcessPhase::GaveUp,
            ),
            (
                ProcessStatus {
                    restart_count: 3,
                    target: TargetState::Stopped(StopReason::User),
                    transition: None,
                    child: ChildState::Terminated,
                    readiness: ReadinessState::Inactive,
                    restart: RestartDecision::Exhausted,
                },
                ProcessPhase::Stopped,
            ),
        ];

        for (status, expected) in cases {
            assert!(status.is_valid(), "{status:?}");
            assert_eq!(status.display_phase(), expected, "{status:?}");
        }
    }

    #[test]
    fn settled_excludes_every_source_of_live_work() {
        let mut status = ProcessStatus::stopped(StopReason::User, ChildState::Terminated);
        assert!(status.is_settled());

        status.target = TargetState::Running;
        status.child = ChildState::NeverSpawned;
        status.transition = Some(StateTransition::WaitingForDependencies);
        assert!(!status.is_settled());
        status.transition = Some(StateTransition::Launching);
        assert!(!status.is_settled());
        status.transition = None;
        status.child = ChildState::Running;
        status.readiness = ReadinessState::Pending;
        assert!(!status.is_settled());
        status.child = ChildState::Exited(ExitOutcome::Failure);
        status.readiness = ReadinessState::Inactive;
        status.restart = RestartDecision::Pending;
        assert!(!status.is_settled());
    }

    #[test]
    fn rust_owns_the_compact_layout() {
        let size = std::mem::size_of::<ProcessStatus>();
        let align = std::mem::align_of::<ProcessStatus>();
        eprintln!("ProcessStatus: size={size}, align={align}");
        assert!(
            size <= 32,
            "ProcessStatus unexpectedly grew to {size} bytes"
        );
    }

    #[test]
    fn generation_ids_are_monotonic_and_distinct_types() {
        let mut run = RunId::initial();
        assert_eq!(run.next(), Some(RunId(1)));
        assert_eq!(run.next(), Some(RunId(2)));

        let mut deadline = DeadlineId::initial();
        assert_eq!(deadline.next(), Some(DeadlineId(1)));

        let mut exhausted = RunId(u64::MAX);
        assert_eq!(exhausted.next(), None);
        assert_eq!(exhausted, RunId(u64::MAX));
    }
}
