use iocraft::prelude::State;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

pub mod model_events;
pub mod tracing_interface;

// UI modules
pub mod app;
pub mod components;
pub mod expanded_view;
pub mod input;
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

/// Runs a loop that waits for notifications and triggers redraws at a throttled rate.
///
/// Uses leading-edge throttling:
/// - Waits for a notification
/// - On wake: triggers redraw, then sleeps to enforce FPS cap
/// - Subsequent notifications during throttle sleep are coalesced by `Notify`
///
/// The TUI uses `Notify::notify_one()`, which stores a permit if no task is currently
/// waiting. That avoids the periodic idle redraws that `notify_waiters()` required as a
/// fallback and reduces visible flicker in terminals like tmux.
pub async fn throttled_notify_loop(notify: Arc<Notify>, mut redraw: State<u64>, max_fps: u64) {
    let throttle_duration = Duration::from_millis(1000 / max_fps);

    loop {
        notify.notified().await;
        let Some(val) = redraw.try_get() else {
            break;
        };
        redraw.set(val.wrapping_add(1));
        // Throttle: minimum time between renders to cap FPS
        tokio::time::sleep(throttle_duration).await;
    }
}
