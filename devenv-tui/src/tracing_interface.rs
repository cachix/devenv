/// TUI Tracing Interface Specification
///
/// This defines the expected structure of tracing spans and events that TUI systems
/// should parse and render. Systems emit these using the standard `tracing` crate,
/// and TUI implementations parse them for display.

// Re-export tracing for convenience (not currently used but available for documentation examples)

/// Standard span fields for operations (builds, downloads, evaluations, etc.)
///
/// Usage:
/// ```rust
/// use tracing::info_span;
/// let _span = info_span!(
///     "nix_build",
///     operation.type = "build",
///     operation.name = "hello-2.12.1",
///     operation.short_name = "hello",
///     operation.derivation = "/nix/store/abc123-hello-2.12.1.drv"
/// ).entered();
/// ```
pub mod operation_fields {
    /// The type of operation being performed
    pub const TYPE: &str = "operation.type";

    /// Full descriptive name of the operation
    pub const NAME: &str = "operation.name";

    /// Short name for compact display
    pub const SHORT_NAME: &str = "operation.short_name";

    /// Nix derivation path (for builds)
    pub const DERIVATION: &str = "operation.derivation";

    /// Download URL (for fetches)
    pub const URL: &str = "operation.url";

    /// Substituter source (for downloads)
    pub const SUBSTITUTER: &str = "operation.substituter";

    /// Build phase (configure, build, install, etc.)
    pub const PHASE: &str = "operation.phase";

    /// Build machine (for distributed builds)
    pub const MACHINE: &str = "operation.machine";

    /// Nix store path
    pub const STORE_PATH: &str = "operation.store_path";
}

/// Standard span fields for high-level tasks
///
/// Usage:
/// ```rust
/// use tracing::info_span;
/// let _span = info_span!(
///     "devenv_shell",
///     task.name = "Building devenv shell",
///     task.priority = "high"
/// ).entered();
/// ```
pub mod task_fields {
    /// Task name for display
    pub const NAME: &str = "task.name";

    /// Task priority (low, normal, high, critical)
    pub const PRIORITY: &str = "task.priority";

    /// Expected number of subtasks/operations
    pub const EXPECTED_SUBTASKS: &str = "task.expected_subtasks";
}

/// Standard event types for progress updates
///
/// Usage:
/// ```rust
/// use tracing::info;
/// info!(
///     progress.type = "bytes",
///     progress.current = 1024,
///     progress.total = 4096,
///     progress.rate = 512.0,
///     "Downloaded 1KB/4KB"
/// );
/// ```
pub mod progress_events {
    /// Generic progress with current/total counts
    pub const GENERIC: &str = "generic";

    /// Byte transfer progress (downloads, uploads)
    pub const BYTES: &str = "bytes";

    /// File-based progress (for Nix evaluation)
    pub const FILES: &str = "files";

    /// Percentage completion (0.0-100.0)
    pub const PERCENTAGE: &str = "percentage";

    /// Indeterminate progress (just show activity)
    pub const INDETERMINATE: &str = "indeterminate";

    /// Progress event fields
    pub mod fields {
        pub const TYPE: &str = "progress.type";
        pub const CURRENT: &str = "progress.current";
        pub const TOTAL: &str = "progress.total";
        pub const PERCENTAGE: &str = "progress.percentage";
        pub const RATE: &str = "progress.rate"; // bytes/sec for transfers
        pub const PHASE: &str = "progress.phase"; // current build phase, etc.
    }
}

/// Standard event targets for build output streaming
///
/// Usage:
/// ```rust
/// use tracing::{event, Level};
/// use devenv_tui::tracing_interface::build_log_events::{STDOUT_TARGET, fields};
/// # let build_span = tracing::info_span!("test");
/// # let line = "test";
/// // Stream stdout from build process
/// event!(
///     Level::INFO,
///     {fields::STREAM} = "stdout",
///     {fields::MESSAGE} = %line
/// );
///
/// // Stream stderr from build process
/// event!(
///     Level::ERROR,
///     {fields::STREAM} = "stderr",
///     {fields::MESSAGE} = %line
/// );
/// ```
pub mod build_log_events {
    /// Target for stdout events
    pub const STDOUT_TARGET: &str = "stdout";

    /// Target for stderr events  
    pub const STDERR_TARGET: &str = "stderr";

    /// Build log event fields
    pub mod fields {
        /// Stream type identifier
        pub const STREAM: &str = "nix_stream";

        /// Log line content
        pub const MESSAGE: &str = "message";
    }
}

/// Standard event types for status updates
///
/// Usage:
/// ```rust
/// use tracing::{info, warn};
/// info!(
///     status = "active",
///     "Build started"
/// );
///
/// warn!(
///     status = "waiting",
///     status.reason = "waiting for network",
///     "Download stalled"
/// );
/// ```
pub mod status_events {
    pub const STARTING: &str = "starting";
    pub const ACTIVE: &str = "active";
    pub const WAITING: &str = "waiting";
    pub const PAUSED: &str = "paused";
    pub const COMPLETED: &str = "completed";
    pub const FAILED: &str = "failed";
    pub const CANCELLED: &str = "cancelled";

    pub mod fields {
        pub const STATUS: &str = "status";
        pub const REASON: &str = "status.reason"; // why waiting/failed
        pub const ERROR: &str = "status.error"; // error details
    }
}

/// Standard operation types (matches NixActivityType enum)
pub mod operation_types {
    pub const BUILD: &str = "build";
    pub const DOWNLOAD: &str = "download";
    pub const EVALUATE: &str = "evaluate";
    pub const QUERY: &str = "query";
    pub const COPY: &str = "copy";
    pub const SUBSTITUTE: &str = "substitute";
    pub const FETCH_TREE: &str = "fetch_tree";
}

/// Nix-specific fields for internal tracking
///
/// Usage:
/// ```rust
/// use tracing::info_span;
/// let _span = info_span!(
///     "nix_build",
///     operation.type = "build",
///     operation.name = "hello",
///     nix.activity_id = 42
/// ).entered();
/// ```
pub mod nix_fields {
    /// Nix internal activity ID (from --log-format internal-json)
    pub const ACTIVITY_ID: &str = "nix.activity_id";
}

/// Convenience macros for creating common span types
#[macro_export]
macro_rules! build_span {
    ($name:expr, $drv:expr, $short:expr) => {
        tracing::info_span!(
            "nix_build",
            operation.type = "build",
            operation.name = $name,
            operation.short_name = $short,
            operation.derivation = $drv
        )
    };
}

#[macro_export]
macro_rules! download_span {
    ($name:expr, $short:expr, $url:expr) => {
        tracing::info_span!(
            "nix_download",
            operation.type = "download",
            operation.name = $name,
            operation.short_name = $short,
            operation.url = $url
        )
    };
}

#[macro_export]
macro_rules! task_span {
    ($name:expr, $priority:expr) => {
        tracing::info_span!("devenv_task", task.name = $name, task.priority = $priority)
    };
}

/// Example usage patterns for common scenarios
#[cfg(doc)]
pub mod examples {
    use super::*;

    /// Example: Nix build with streaming logs
    #[allow(unused)]
    pub fn nix_build_with_logs_example() {
        use super::build_log_events::{STDOUT_TARGET, fields};
        use super::operation_fields::{PHASE, TYPE};
        use super::progress_events::fields::{CURRENT, TOTAL};
        use super::status_events::fields::STATUS;
        use tracing::{error, event, info, info_span};

        // Create build span using convenience macro
        let span = build_span!(
            "hello-2.12.1",
            "/nix/store/abc123-hello-2.12.1.drv",
            "hello"
        )
        .entered();

        // Signal build start
        info!({ STATUS } = "starting", "Build started");

        // Phase updates
        info!({ PHASE } = "configure", "Configuring package");
        info!(
            { TYPE } = "generic",
            { CURRENT } = 1,
            { TOTAL } = 4,
            "Phase 1/4 complete"
        );

        // Stream stdout from build process
        event!(
            target: STDOUT_TARGET,
            parent: &span,
            {fields::STREAM} = "stdout",
            {fields::MESSAGE} = "checking for gcc... gcc"
        );
        event!(
            target: STDOUT_TARGET,
            parent: &span,
            {fields::STREAM} = "stdout",
            {fields::MESSAGE} = "checking whether the C compiler works... yes"
        );

        info!({ PHASE } = "build", "Building package");
        info!(
            { TYPE } = "generic",
            { CURRENT } = 2,
            { TOTAL } = 4,
            "Phase 2/4 complete"
        );

        // More build output
        event!(
            target: STDOUT_TARGET,
            parent: &span,
            {fields::STREAM} = "stdout",
            {fields::MESSAGE} = "gcc -DHAVE_CONFIG_H -I. hello.c -o hello"
        );

        info!({ STATUS } = "completed", "Build finished successfully");
    }

    /// Example: Download with progress
    pub fn download_example() {
        use super::operation_fields::{NAME, SHORT_NAME, SUBSTITUTER, TYPE, URL};
        use super::progress_events::fields::{CURRENT, RATE, TOTAL};
        use super::status_events::fields::STATUS;
        use tracing::{info, info_span};

        let _span = info_span!(
            "nix_download",
            { TYPE } = "download",
            { NAME } = "source.tar.gz",
            { SHORT_NAME } = "source.tar.gz",
            { URL } = "https://example.com/source.tar.gz",
            { SUBSTITUTER } = "cache.nixos.org"
        )
        .entered();

        // Progress updates
        for transferred in (0..=10).map(|i| i * 1024) {
            info!(
                { TYPE } = "bytes",
                { CURRENT } = transferred,
                { TOTAL } = 10240,
                { RATE } = 2048.0,
                "Downloaded {}/{} bytes",
                transferred,
                10240
            );
        }

        info!({ STATUS } = "completed", "Download finished");
    }

    /// Example: Task with subtasks
    pub fn task_hierarchy_example() {
        use super::operation_fields::{NAME, TYPE};
        use super::task_fields;
        use tracing::{info, info_span};

        let _main_task = info_span!(
            "build_project",
            { task_fields::NAME } = "Building project",
            { task_fields::PRIORITY } = "high",
            { task_fields::EXPECTED_SUBTASKS } = 3
        )
        .entered();

        // Subtask 1
        {
            let _deps = info_span!(
                "fetch_dependencies",
                { TYPE } = "download",
                { NAME } = "Fetching dependencies"
            )
            .entered();

            info!("Downloaded 15 dependencies");
        }

        // Subtask 2
        {
            let _build = info_span!(
                "compile_sources",
                { TYPE } = "build",
                { NAME } = "Compiling sources"
            )
            .entered();

            info!("Compiled 42 files");
        }

        info!("Project built successfully");
    }
}

/// TUI Integration Guidelines
///
/// For TUI implementations parsing these events:
///
/// ## Tree Component (In-Progress Items)
/// - Parse span creation/entry to show new operations in hierarchy
/// - Update progress bars from progress.* events  
/// - Update status indicators from status events
/// - Show operation.short_name for compact display
/// - Nest spans based on tracing's built-in parent-child relationships
///
/// ## Status Summary Component  
/// - Track completed/failed/active operation counts
/// - Show overall progress across all operations
/// - Display current phase/status of active operations
/// - Aggregate transfer rates, build phases, etc.
///
/// ## Event Processing
/// - Use tracing_subscriber::Layer to capture spans/events
/// - Store operation data in span extensions or external map keyed by span ID
/// - Update TUI state on each event and re-render
/// - Handle span close events to mark operations complete
///
/// ## Display Patterns
/// - `operation.short_name` in tree nodes for compact display
/// - Progress bars for progress.* events with current/total
/// - Status icons/colors based on status field values
/// - Hierarchical indentation based on span nesting depth
/// - Real-time log streaming from info!/warn!/error! events
pub mod tui_integration {}
