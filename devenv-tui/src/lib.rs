use iocraft::prelude::State;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::Notify;

pub mod model_events;
pub mod tracing_interface;

// UI modules
pub mod app;
pub mod components;
pub mod expanded_view;
pub mod model;
pub mod view;

pub use app::{TuiApp, TuiConfig};
pub use model::{
    Activity, ActivityModel, ActivityVariant, BuildActivity, ChildActivityLimit, DownloadActivity,
    ProgressActivity, QueryActivity, RenderContext, TaskActivity, TaskDisplayStatus, TerminalSize,
    UiState, ViewMode,
};
pub use model_events::UiEvent;

// Re-export shell session types from devenv-shell
pub use devenv_shell::{
    SessionConfig, SessionError, SessionIo, ShellCommand, ShellEvent, ShellSession, TuiHandoff,
};

/// Idle heartbeat: when nothing in the model has changed, the loop still emits
/// a redraw at this slow rate so that elapsed-time displays keep ticking. This
/// is what keeps idle CPU minimal — instead of recomputing the (taffy) layout
/// ~`max_fps` times per second, an idle TUI redraws roughly once per second.
const IDLE_HEARTBEAT_MS: u64 = 1000;

/// Sink that triggers a single UI redraw.
///
/// Abstracted behind a trait so the throttle/coalescing logic can be unit-tested
/// without constructing an iocraft `State` (which can only be built inside a
/// component's render via `Hooks::use_state`).
pub trait RedrawSink {
    /// Trigger one redraw. Returns `false` if the underlying state is gone (its
    /// owner has been dropped), signalling the loop to stop.
    fn trigger(&mut self) -> bool;
}

impl RedrawSink for State<u64> {
    fn trigger(&mut self) -> bool {
        match self.try_get() {
            Some(val) => {
                self.set(val.wrapping_add(1));
                true
            }
            None => false,
        }
    }
}

/// Runs a loop that triggers UI redraws, throttled to cap the frame rate.
///
/// `version` is a monotonic counter bumped by the event processor whenever the
/// activity model changes. The loop redraws (recomputing the taffy layout) on:
/// - the very first frame (so the UI draws on startup),
/// - any real model change (`version` advanced),
/// - an explicit `notify` wake with no version change — e.g. a keypress that
///   mutated `ui_state`, which iocraft cannot observe on its own and which would
///   otherwise not repaint until the heartbeat (#2915), and
/// - a slow [`IDLE_HEARTBEAT_MS`] heartbeat (so elapsed-time displays advance).
///
/// This is the fix for the high idle-CPU behaviour (#2915): previously the loop
/// redrew on every safety-net timeout (~`max_fps` times/sec) regardless of
/// whether anything changed, continuously recomputing the flexbox layout. Now an
/// idle TUI redraws roughly once per second.
///
/// Lost-wakeup safety: `Notify::notify_waiters` only wakes tasks already parked
/// on `notified()` and stores no permit. We therefore register interest (via
/// `Notified::enable`) *before* reading `version`, so a notification that races
/// our check is observed on the very next wait rather than being dropped until
/// the heartbeat.
///
/// Throttling/coalescing: after any redraw we sleep at least `throttle` before
/// the next one; notifications during that sleep are coalesced because `version`
/// keeps climbing and is re-read on the next iteration.
///
/// The `shutdown` notify bypasses everything: when fired the loop emits one
/// final redraw and returns immediately, so cooperative-exit flags are observed
/// without waiting up to a full throttle period.
///
/// `redraw` is generic over [`RedrawSink`] so the throttle/coalescing logic can
/// be unit-tested without constructing an iocraft `State`.
pub async fn throttled_notify_loop(
    notify: Arc<Notify>,
    shutdown: Arc<Notify>,
    version: Arc<AtomicU64>,
    mut redraw: impl RedrawSink,
    max_fps: u64,
) {
    // `max(1)` guards the division: a misconfigured `max_fps` of 0 would panic.
    let throttle = Duration::from_millis(1000 / max_fps.max(1));
    let idle_heartbeat = Duration::from_millis(IDLE_HEARTBEAT_MS);

    // Force the first frame regardless of the starting version.
    let mut rendered_version = u64::MAX;

    loop {
        // Register for notifications before reading `version` (see "Lost-wakeup
        // safety" above). `enable()` registers the waiter without awaiting.
        let notified = notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        let current = version.load(Ordering::Acquire);
        let changed = current != rendered_version;
        rendered_version = current;

        if !changed {
            // Idle: wait for a model change, an explicit wake (input), the
            // heartbeat (to tick elapsed timers), or shutdown.
            tokio::select! {
                _ = notified.as_mut() => {}
                _ = tokio::time::sleep(idle_heartbeat) => {}
                _ = shutdown.notified() => {
                    redraw.trigger();
                    return;
                }
            }
            // Re-read after waking, then redraw unconditionally: a heartbeat or
            // input wake still needs to repaint (timers / ui_state), and a model
            // change is picked up here too.
            rendered_version = version.load(Ordering::Acquire);
        }

        if !redraw.trigger() {
            break;
        }

        // Cap the frame rate: at least `throttle` between redraws.
        tokio::select! {
            _ = tokio::time::sleep(throttle) => {}
            _ = shutdown.notified() => {
                redraw.trigger();
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use tokio::task::JoinHandle;

    /// Test redraw sink that just counts how many times it was triggered.
    struct CountingSink(Arc<AtomicU64>);

    impl RedrawSink for CountingSink {
        fn trigger(&mut self) -> bool {
            self.0.fetch_add(1, Ordering::Relaxed);
            true
        }
    }

    /// Handles to a spawned loop under test.
    struct Harness {
        notify: Arc<Notify>,
        shutdown: Arc<Notify>,
        version: Arc<AtomicU64>,
        count: Arc<AtomicU64>,
        handle: JoinHandle<()>,
    }

    impl Harness {
        fn spawn() -> Self {
            let notify = Arc::new(Notify::new());
            let shutdown = Arc::new(Notify::new());
            let version = Arc::new(AtomicU64::new(0));
            let count = Arc::new(AtomicU64::new(0));
            let handle = tokio::spawn(throttled_notify_loop(
                notify.clone(),
                shutdown.clone(),
                version.clone(),
                CountingSink(count.clone()),
                60,
            ));
            Self {
                notify,
                shutdown,
                version,
                count,
                handle,
            }
        }

        fn redraws(&self) -> u64 {
            self.count.load(Ordering::Relaxed)
        }

        async fn stop(self) {
            self.shutdown.notify_waiters();
            let _ = self.handle.await;
        }
    }

    /// Regression test for #2915: an idle TUI must not redraw (recompute layout)
    /// at the full frame rate. With virtual time we advance 10 seconds with no
    /// model changes and assert the loop redrew only a handful of times (initial
    /// frame + ~1 heartbeat/sec), not ~`max_fps`/sec.
    #[tokio::test(start_paused = true)]
    async fn idle_does_not_redraw_at_frame_rate() {
        let h = Harness::spawn();

        // Advance virtual time 10s with no notifications and no version bumps.
        tokio::time::sleep(Duration::from_secs(10)).await;
        let redraws = h.redraws();
        h.stop().await;

        // At 60fps the old loop would redraw ~30/sec => ~300 over 10s. The new
        // loop should be roughly: 1 initial + ~10 heartbeats.
        assert!(
            redraws <= 20,
            "idle redraws should be ~1/sec, got {redraws} over 10s"
        );
    }

    /// A model change must trigger a redraw promptly.
    #[tokio::test(start_paused = true)]
    async fn model_change_triggers_redraw() {
        let h = Harness::spawn();

        // Let the initial frame settle.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let before = h.redraws();

        // Simulate a model change.
        h.version.fetch_add(1, Ordering::Release);
        h.notify.notify_waiters();
        tokio::time::sleep(Duration::from_millis(50)).await;

        let after = h.redraws();
        assert!(
            after > before,
            "a model change must trigger a redraw (before={before}, after={after})"
        );

        h.stop().await;
    }

    /// An explicit notify with NO version change (e.g. a keypress that mutated
    /// `ui_state`) must still trigger a redraw — otherwise input would not
    /// repaint until the idle heartbeat (#2915).
    #[tokio::test(start_paused = true)]
    async fn input_notify_without_version_change_triggers_redraw() {
        let h = Harness::spawn();

        // Let the initial frame settle so the loop is parked idle.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let before = h.redraws();

        // Notify without bumping the version, as the key handlers do.
        h.notify.notify_waiters();
        tokio::time::sleep(Duration::from_millis(50)).await;

        let after = h.redraws();
        assert!(
            after > before,
            "an input notify must trigger a redraw (before={before}, after={after})"
        );

        h.stop().await;
    }

    /// A burst of rapid changes must be throttled, not redrawn once per change.
    #[tokio::test(start_paused = true)]
    async fn rapid_changes_are_throttled() {
        let h = Harness::spawn();

        // Flood with changes for ~1s of virtual time. Yield via a tiny sleep so
        // paused-time auto-advance can run the render loop between bumps.
        for _ in 0..1000 {
            h.version.fetch_add(1, Ordering::Release);
            h.notify.notify_waiters();
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        let redraws = h.redraws();
        h.stop().await;

        // ~1s at 60fps throttle => on the order of 60 frames, not ~1000.
        assert!(
            redraws < 200,
            "rapid changes should be throttled toward the fps cap, got {redraws}"
        );
    }

    /// Shutdown emits a final redraw and the loop returns.
    #[tokio::test(start_paused = true)]
    async fn shutdown_triggers_final_redraw_and_exits() {
        let h = Harness::spawn();

        tokio::time::sleep(Duration::from_millis(50)).await;
        h.shutdown.notify_waiters();

        // The loop must actually return after shutdown.
        h.handle.await.expect("loop should exit on shutdown");
        assert!(h.count.load(Ordering::Relaxed) >= 1);
    }
}
