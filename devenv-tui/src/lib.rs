use iocraft::prelude::State;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

pub mod model_events;
pub mod shell_runner;
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
pub use shell_runner::{ShellRunner, ShellRunnerError};

/// Runs a loop that waits for notifications and triggers redraws at a throttled rate.
///
/// Uses leading-edge throttling:
/// - Waits for notification OR timeout (whichever comes first)
/// - On wake: triggers redraw, then sleeps to enforce FPS cap
/// - Subsequent notifications during throttle sleep are coalesced (model accumulates)
///
/// The timeout on the notification wait serves as a safety net: `Notify::notify_waiters()`
/// only wakes tasks that are actively waiting on `notified()`. If a notification arrives
/// while we're in the throttle sleep, it's lost. The timeout ensures we periodically
/// check for state changes (like the final Done event) even if we missed a notification.
pub async fn throttled_notify_loop(notify: Arc<Notify>, mut redraw: State<u64>, max_fps: u64) {
    let throttle_duration = Duration::from_millis(1000 / max_fps);

    loop {
        // Wait for notification OR timeout. The timeout ensures we don't miss the final
        // state if a notification arrived during the previous throttle sleep.
        tokio::select! {
            _ = notify.notified() => {}
            _ = tokio::time::sleep(throttle_duration) => {}
        }
        redraw.set(redraw.get().wrapping_add(1));
        // Throttle: minimum time between renders to cap FPS
        tokio::time::sleep(throttle_duration).await;
    }
}
