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
