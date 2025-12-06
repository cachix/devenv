use super::human_duration::HumanReadableDuration;
use std::time::{Duration, Instant};

/// A structure to capture span timings, similar to what is available internally in tracing_subscriber.
#[derive(Debug, Clone)]
pub(crate) struct SpanTimings {
    idle: Duration,
    busy: Duration,
    last: Instant,
}

impl SpanTimings {
    pub fn new() -> Self {
        Self {
            idle: Duration::ZERO,
            busy: Duration::ZERO,
            last: Instant::now(),
        }
    }

    pub fn enter(&mut self) {
        let now = Instant::now();
        self.idle += now - self.last;
        self.last = now;
    }

    pub fn exit(&mut self) {
        let now = Instant::now();
        self.busy += now - self.last;
        self.last = now;
    }

    /// Returns the total duration of the span, combining the idle and busy times.
    pub fn total_duration(&self) -> HumanReadableDuration {
        HumanReadableDuration(self.idle + self.busy)
    }
}
