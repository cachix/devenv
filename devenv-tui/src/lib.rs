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
pub mod model;
pub mod view;

pub use app::{TuiApp, TuiConfig};
pub use model::{
    Activity, ActivityVariant, BuildActivity, ChildActivityLimit, DownloadActivity, Model,
    ProgressActivity, QueryActivity, TaskActivity, TaskDisplayStatus, TerminalSize, ViewMode,
};
pub use model_events::UiEvent;

/// Runs a loop that waits for notifications and triggers redraws at a throttled rate.
///
/// - Renders immediately on first notification (leading edge)
/// - Sleeps to enforce FPS cap
/// - Subsequent notifications during sleep are coalesced (model accumulates changes)
pub async fn throttled_notify_loop(notify: Arc<Notify>, mut redraw: State<u64>, max_fps: u64) {
    let throttle_duration = Duration::from_millis(1000 / max_fps);

    loop {
        notify.notified().await;
        redraw.set(redraw.get().wrapping_add(1));
        tokio::time::sleep(throttle_duration).await;
    }
}
