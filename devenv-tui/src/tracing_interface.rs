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
    pub const TYPE: &str = "devenv.ui.operation.type";

    /// Full descriptive name of the operation
    pub const NAME: &str = "devenv.ui.operation.name";

    /// Short name for compact display
    pub const SHORT_NAME: &str = "devenv.ui.operation.short_name";
}

/// Standard detail fields for supplementary operation metadata
///
/// These fields provide additional context shown as suffixes or secondary
/// information in the UI (e.g., build phase, machine, derivation path).
///
/// Usage:
/// ```rust
/// use tracing::info;
/// info!(
///     devenv.ui.details.phase = "configure",
///     devenv.ui.details.machine = "build-host",
///     "Building package"
/// );
/// ```
pub mod details_fields {
    /// Build phase (configure, build, install, etc.)
    pub const PHASE: &str = "devenv.ui.details.phase";

    /// Build machine (for distributed builds)
    pub const MACHINE: &str = "devenv.ui.details.machine";

    /// Nix derivation path (for builds)
    pub const DERIVATION: &str = "devenv.ui.details.derivation";

    /// Nix store path
    pub const STORE_PATH: &str = "devenv.ui.details.store_path";

    /// Substituter source (for downloads)
    pub const SUBSTITUTER: &str = "devenv.ui.details.substituter";

    /// Download URL (for fetches)
    pub const URL: &str = "devenv.ui.details.url";
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
    pub const NAME: &str = "devenv.ui.task.name";

    /// Task priority (low, normal, high, critical)
    pub const PRIORITY: &str = "devenv.ui.task.priority";

    /// Expected number of subtasks/operations
    pub const EXPECTED_SUBTASKS: &str = "devenv.ui.task.expected_subtasks";
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
        pub const TYPE: &str = "devenv.ui.progress.type";
        pub const CURRENT: &str = "devenv.ui.progress.current";
        pub const TOTAL: &str = "devenv.ui.progress.total";
        pub const PERCENTAGE: &str = "devenv.ui.progress.percentage";
        pub const RATE: &str = "devenv.ui.progress.rate"; // bytes/sec for transfers
        pub const PHASE: &str = "devenv.ui.progress.phase"; // current build phase, etc.
    }
}

/// Standard log output streaming fields and targets
///
/// Used for streaming stdout/stderr from operations (builds, etc.) to the TUI.
///
/// Usage:
/// ```rust
/// use tracing::{event, Level};
/// use devenv_tui::tracing_interface::log_fields;
/// // Stream stdout
/// event!(
///     target: "stdout",
///     Level::INFO,
///     { log_fields::STREAM } = "stdout",
///     { log_fields::MESSAGE } = "output line"
/// );
///
/// // Stream stderr
/// event!(
///     target: "stderr",
///     Level::ERROR,
///     { log_fields::STREAM } = "stderr",
///     { log_fields::MESSAGE } = "error line"
/// );
/// ```
pub mod log_fields {
    /// Target for stdout events
    pub const STDOUT_TARGET: &str = "stdout";

    /// Target for stderr events
    pub const STDERR_TARGET: &str = "stderr";

    /// Stream type identifier (stdout/stderr)
    pub const STREAM: &str = "devenv.ui.log.stream";

    /// Log line content
    pub const MESSAGE: &str = "devenv.ui.log.message";
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
        pub const STATUS: &str = "devenv.ui.status";
        pub const REASON: &str = "devenv.ui.status.reason"; // why waiting/failed
        pub const ERROR: &str = "devenv.ui.status.error"; // error details
        pub const RESULT: &str = "devenv.ui.status.result"; // task result (success/failed/cached/skipped/etc)
    }
}

/// Standard operation types (matches ActivityVariant enum in TUI model)
///
/// These define the types of operations that can be displayed in the TUI.
/// Each type may render differently with type-specific details and formatting.
pub mod operation_types {
    /// Build operation (compiling, linking)
    pub const BUILD: &str = "build";

    /// Download operation (fetching from cache/substituter)
    pub const DOWNLOAD: &str = "download";

    /// Evaluation operation (Nix expression evaluation)
    pub const EVALUATE: &str = "evaluate";

    /// Query operation (checking cache for paths)
    pub const QUERY: &str = "query";

    /// Copy operation (copying paths between stores)
    pub const COPY: &str = "copy";

    /// Substitute operation (substituting from binary cache)
    pub const SUBSTITUTE: &str = "substitute";

    /// Fetch tree operation (fetching Git repos, tarballs, etc.)
    pub const FETCH_TREE: &str = "fetch_tree";

    /// Devenv operation (user-facing devenv messages)
    ///
    /// Usage:
    /// ```rust
    /// use tracing::info;
    /// use devenv_tui::tracing_interface::{operation_fields, operation_types};
    /// info!(
    ///     { operation_fields::TYPE } = operation_types::DEVENV,
    ///     { operation_fields::NAME } = "Entering shell"
    /// );
    /// ```
    pub const DEVENV: &str = "devenv";
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
            { $crate::tracing_interface::operation_fields::TYPE } = "build",
            { $crate::tracing_interface::operation_fields::NAME } = $name,
            { $crate::tracing_interface::operation_fields::SHORT_NAME } = $short,
            { $crate::tracing_interface::details_fields::DERIVATION } = $drv
        )
    };
}

#[macro_export]
macro_rules! download_span {
    ($name:expr, $short:expr, $url:expr) => {
        tracing::info_span!(
            "nix_download",
            { $crate::tracing_interface::operation_fields::TYPE } = "download",
            { $crate::tracing_interface::operation_fields::NAME } = $name,
            { $crate::tracing_interface::operation_fields::SHORT_NAME } = $short,
            { $crate::tracing_interface::details_fields::URL } = $url
        )
    };
}

#[macro_export]
macro_rules! task_span {
    ($name:expr, $priority:expr) => {
        tracing::info_span!(
            "devenv_task",
            { $crate::tracing_interface::task_fields::NAME } = $name,
            { $crate::tracing_interface::task_fields::PRIORITY } = $priority
        )
    };
}

/// Create a task span with proper operation fields for TUI tracking
///
/// This is the recommended way to create task spans that will be properly tracked
/// by the TUI. It includes both task-specific fields and operation fields for
/// comprehensive tracking.
///
/// # Example
/// ```
/// use devenv_tui::tracing_interface::create_task_span;
/// let span = create_task_span("myns:mytask", "normal");
/// let _guard = span.enter();
/// // ... perform task operations ...
/// // span is automatically closed when _guard is dropped
/// ```
pub fn create_task_span(task_name: &str, priority: &str) -> tracing::Span {
    use tracing::info_span;
    info_span!(
        "devenv_task",
        { task_fields::NAME } = task_name,
        { task_fields::PRIORITY } = priority,
        { operation_fields::TYPE } = "task",
        { operation_fields::NAME } = task_name,
        { operation_fields::SHORT_NAME } = task_name,
    )
}

/// Example usage patterns for common scenarios
#[cfg(doc)]
pub mod examples {
    use super::*;

    /// Example: Nix build with streaming logs
    #[allow(unused)]
    pub fn nix_build_with_logs_example() {
        use super::details_fields::PHASE;
        use super::log_fields::STDOUT_TARGET;
        use super::operation_fields::TYPE;
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
            {log_fields::STREAM} = "stdout",
            {log_fields::MESSAGE} = "checking for gcc... gcc"
        );
        event!(
            target: STDOUT_TARGET,
            parent: &span,
            {log_fields::STREAM} = "stdout",
            {log_fields::MESSAGE} = "checking whether the C compiler works... yes"
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
            {log_fields::STREAM} = "stdout",
            {log_fields::MESSAGE} = "gcc -DHAVE_CONFIG_H -I. hello.c -o hello"
        );

        info!({ STATUS } = "completed", "Build finished successfully");
    }

    /// Example: Download with progress
    pub fn download_example() {
        use super::details_fields::{SUBSTITUTER, URL};
        use super::operation_fields::{NAME, SHORT_NAME, TYPE};
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
