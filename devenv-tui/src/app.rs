use crate::{
    expanded_view::ExpandedLogView,
    model::{ActivityModel, RenderContext, UiState, ViewMode},
    view::view,
};
use crossterm::{
    cursor, event, execute,
    style::{Color, ResetColor, SetForegroundColor},
    terminal,
};
use devenv_activity::{ActivityEvent, ActivityLevel};
use devenv_processes::ProcessCommand;
use iocraft::prelude::*;
use std::io::{self, Write};
use std::sync::{Arc, OnceLock, RwLock};
use tokio::sync::{Notify, mpsc, watch};
use tokio_shutdown::{Shutdown, Signal};
use tracing::debug;

#[cfg(unix)]
use std::os::unix::io::RawFd;

/// Original terminal settings saved before TUI enters raw mode.
static ORIGINAL_TERMIOS: OnceLock<rustix::termios::Termios> = OnceLock::new();

/// TTY file descriptor for rendering, stored statically for panic/exit hooks.
#[cfg(unix)]
static TTY_FD: OnceLock<RawFd> = OnceLock::new();

/// A non-owning writer to a raw file descriptor.
///
/// Does not close the fd on drop. The caller must ensure the fd outlives the writer.
#[cfg(unix)]
pub(crate) struct FdWriter(pub(crate) RawFd);

#[cfg(unix)]
impl io::Write for FdWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let fd = unsafe { std::os::unix::io::BorrowedFd::borrow_raw(self.0) };
        Ok(rustix::io::write(fd, buf)?)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Store the TTY fd for use by `restore_terminal` and other hooks.
#[cfg(unix)]
pub fn set_tty_fd(fd: RawFd) {
    TTY_FD.get_or_init(|| fd);
}

/// Get the stored TTY fd, if one was set.
#[cfg(unix)]
pub(crate) fn tty_fd() -> Option<RawFd> {
    TTY_FD.get().copied()
}

/// Create a writer targeting the TUI output.
///
/// When a tty fd is provided, writes go to that fd. Otherwise falls back to
/// stderr (fd 2), matching the pre-tty behavior.
#[cfg(unix)]
fn tui_writer(tty_fd: Option<RawFd>) -> FdWriter {
    FdWriter(tty_fd.unwrap_or(2))
}

/// Configuration for the TUI application.
///
/// Note: The TUI always renders to stderr to keep stdout available for command output
/// (e.g., `devenv print-dev-env` pipes stdout to shell eval).
#[derive(Debug, Clone)]
pub struct TuiConfig {
    /// Maximum events to batch before processing
    pub event_batch_size: usize,
    /// Maximum log messages to keep in memory
    pub max_log_messages: usize,
    /// Maximum log lines per build activity
    pub max_log_lines_per_build: usize,
    /// Number of log lines to show in collapsed view
    pub log_viewport_collapsed: usize,
    /// Maximum frames per second for rendering
    pub max_fps: u64,
    /// Minimum activity level to display (activities below this level are filtered out)
    pub filter_level: ActivityLevel,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            event_batch_size: 64,
            max_log_messages: 1000,
            max_log_lines_per_build: 1000,
            log_viewport_collapsed: 10,
            max_fps: 30,
            filter_level: ActivityLevel::Info,
        }
    }
}

/// Builder for creating and running the TUI application.
pub struct TuiApp {
    config: TuiConfig,
    activity_rx: mpsc::UnboundedReceiver<ActivityEvent>,
    shutdown: Arc<Shutdown>,
    command_tx: Option<mpsc::Sender<ProcessCommand>>,
    shutdown_on_backend_done: bool,
    /// Optional TTY fd for rendering. When set, iocraft and crossterm output
    /// goes to this fd instead of stderr, allowing fd 2 to be redirected.
    #[cfg(unix)]
    tty_fd: Option<RawFd>,
}

impl TuiApp {
    /// Create a new TUI application with required dependencies.
    pub fn new(
        activity_rx: mpsc::UnboundedReceiver<ActivityEvent>,
        shutdown: Arc<Shutdown>,
    ) -> Self {
        Self {
            config: TuiConfig::default(),
            activity_rx,
            shutdown,
            command_tx: None,
            shutdown_on_backend_done: true,
            #[cfg(unix)]
            tty_fd: None,
        }
    }

    /// Set the command sender for process control commands.
    pub fn with_command_sender(mut self, tx: mpsc::Sender<ProcessCommand>) -> Self {
        self.command_tx = Some(tx);
        self
    }

    /// Set the TTY fd for rendering output.
    ///
    /// When set, iocraft renders to this fd instead of stderr, allowing fd 2
    /// to be redirected to `/dev/null` to suppress stray C code output.
    #[cfg(unix)]
    pub fn with_tty_fd(mut self, fd: RawFd) -> Self {
        self.tty_fd = Some(fd);
        self
    }

    /// Set the event batch size for processing activity events.
    pub fn batch_size(mut self, size: usize) -> Self {
        self.config.event_batch_size = size;
        self
    }

    /// Set the maximum number of log messages to keep in memory.
    pub fn max_messages(mut self, n: usize) -> Self {
        self.config.max_log_messages = n;
        self
    }

    /// Set the maximum log lines per build activity.
    pub fn max_build_logs(mut self, n: usize) -> Self {
        self.config.max_log_lines_per_build = n;
        self
    }

    /// Set the number of log lines to show in collapsed view.
    pub fn collapsed_lines(mut self, n: usize) -> Self {
        self.config.log_viewport_collapsed = n;
        self
    }

    /// Set the minimum activity level to display.
    /// Activities below this level will be filtered out.
    pub fn filter_level(mut self, level: ActivityLevel) -> Self {
        self.config.filter_level = level;
        self
    }

    /// Control whether backend completion should trigger global shutdown.
    /// Disable for shell reload handoff where the backend must keep running.
    pub fn shutdown_on_backend_done(mut self, enabled: bool) -> Self {
        self.shutdown_on_backend_done = enabled;
        self
    }

    /// Run the TUI application until the backend completes.
    ///
    /// The `backend_done` receiver signals when the backend has completed its
    /// initial phase (or fully completed). The TUI will drain any remaining
    /// events and then exit.
    /// Run the TUI and return the final render height (for cursor positioning after handoff).
    pub async fn run(
        self,
        backend_done: tokio::sync::oneshot::Receiver<()>,
    ) -> std::io::Result<u16> {
        let config = Arc::new(self.config);
        let activity_model = Arc::new(RwLock::new(ActivityModel::with_config(config.clone())));
        let notify = Arc::new(Notify::new());
        let shutdown = self.shutdown;
        let command_tx = self.command_tx;
        let shutdown_on_backend_done = self.shutdown_on_backend_done;
        #[cfg(unix)]
        let tty_fd: Option<i32> = self.tty_fd;
        #[cfg(not(unix))]
        let tty_fd: Option<i32> = None;
        let (exit_tx, mut exit_rx) = watch::channel(false);

        // Spawn event processor with batching for performance
        // This only writes to ActivityModel, never touches UiState
        // When backend_done fires, it drains remaining events and signals shutdown
        let event_processor_handle = tokio::spawn({
            let activity_model = activity_model.clone();
            let notify = notify.clone();
            let shutdown = shutdown.clone();
            let event_batch_size = config.event_batch_size;
            let mut activity_rx = self.activity_rx;
            let mut backend_done = backend_done;
            let exit_tx = exit_tx.clone();
            async move {
                let mut batch = Vec::with_capacity(event_batch_size);

                loop {
                    tokio::select! {
                        event = activity_rx.recv() => {
                            let Some(event) = event else {
                                // Channel closed unexpectedly
                                let _ = exit_tx.send(true);
                                if shutdown_on_backend_done {
                                    shutdown.shutdown();
                                }
                                break;
                            };

                            batch.push(event);
                            while let Ok(event) = activity_rx.try_recv() {
                                batch.push(event);
                                if batch.len() >= event_batch_size {
                                    break;
                                }
                            }

                            if let Ok(mut m) = activity_model.write() {
                                for event in batch.drain(..) {
                                    m.apply_activity_event(event);
                                }
                            }

                            notify.notify_waiters();
                        }

                        _ = &mut backend_done => {
                            // Backend is done - drain any remaining events
                            while let Ok(event) = activity_rx.try_recv() {
                                batch.push(event);
                            }

                            if !batch.is_empty() {
                                if let Ok(mut m) = activity_model.write() {
                                    for event in batch.drain(..) {
                                        m.apply_activity_event(event);
                                    }
                                }
                                notify.notify_waiters();
                            }

                            let _ = exit_tx.send(true);
                            if shutdown_on_backend_done {
                                shutdown.shutdown();
                            }
                            break;
                        }
                    }
                }
            }
        });

        // Track height to clear when returning from expanded view
        let mut pre_expand_height: u16 = 0;

        // UiState is separate from ActivityModel to avoid lock contention.
        // The event processor only writes to ActivityModel, never UiState.
        // UiState is only modified by the UI thread.
        let ui_state = Arc::new(RwLock::new(UiState::new()));

        // Main loop - runs until shutdown (either Ctrl+C or event processor signals completion)
        loop {
            tokio::select! {
                biased;

                _ = shutdown.wait_for_shutdown() => {
                    break;
                }

                changed = exit_rx.changed() => {
                    if changed.is_err() || *exit_rx.borrow() {
                        break;
                    }
                }

                _ = run_view(
                    activity_model.clone(),
                    ui_state.clone(),
                    notify.clone(),
                    shutdown.clone(),
                    config.clone(),
                    command_tx.clone(),
                    &mut pre_expand_height,
                    tty_fd,
                ) => { }
            }
        }

        // Wait for event processor to finish draining events before final render.
        // This ensures all activity completion events are processed and visible.
        let _ = event_processor_handle.await;

        // Final render pass to ensure all drained events are displayed.
        // Clear previous inline render, then render final state.
        // Use tty writer so output reaches the terminal even when fd 2 is redirected.
        let mut final_render_height: u16 = 0;
        {
            let ui = ui_state.read().unwrap();
            if let Ok(model_guard) = activity_model.read() {
                // Clear the previous inline render output (which had the summary line)
                let lines_to_clear = ui.last_render_height;

                if lines_to_clear > 0 {
                    #[cfg(unix)]
                    let mut out = tui_writer(tty_fd);
                    #[cfg(not(unix))]
                    let mut out = io::stderr();
                    let _ = execute!(
                        out,
                        cursor::MoveToPreviousLine(lines_to_clear),
                        terminal::Clear(terminal::ClearType::FromCursorDown)
                    );
                }

                {
                    // Collect standalone error messages (no parent) from message_log
                    let standalone_errors: Vec<_> = model_guard
                        .get_error_messages()
                        .into_iter()
                        .map(|m| (m.text.clone(), m.details.clone()))
                        .collect();

                    // Collect nested error messages (with parent) from activities
                    let activity_errors: Vec<_> = model_guard
                        .get_activity_error_messages()
                        .into_iter()
                        .map(|(name, details)| (name.to_string(), details.map(|s| s.to_string())))
                        .collect();

                    // Collect stderr from failed builds
                    let failed_build_errors: Vec<_> = model_guard
                        .get_failed_build_errors()
                        .into_iter()
                        .map(|(name, lines)| (name.to_string(), lines.to_vec()))
                        .collect();

                    let (terminal_width, _) = crossterm::terminal::size().unwrap_or((80, 24));

                    let mut element = element! {
                        View(width: terminal_width) {
                            #(vec![view(&model_guard, &ui, RenderContext::Final).into()])
                        }
                    };
                    let canvas = element.render(Some(terminal_width as usize));
                    final_render_height = canvas.height() as u16;
                    #[cfg(unix)]
                    let _ = canvas.write_ansi(tui_writer(tty_fd));
                    #[cfg(not(unix))]
                    let _ = canvas.write_ansi(io::stderr());

                    // Print full error messages in red (not truncated by TUI width)
                    let has_errors = !standalone_errors.is_empty()
                        || !activity_errors.is_empty()
                        || !failed_build_errors.is_empty();
                    if has_errors {
                        #[cfg(unix)]
                        let mut out = tui_writer(tty_fd);
                        #[cfg(not(unix))]
                        let mut out = io::stderr();
                        let _ = writeln!(out);
                        final_render_height += 1; // for the empty line

                        // Print standalone error messages (no parent activity)
                        for (text, details) in standalone_errors {
                            let _ = execute!(out, SetForegroundColor(Color::AnsiValue(160)));
                            let _ = writeln!(out, "{}", text);
                            final_render_height += 1;
                            if let Some(details) = details {
                                final_render_height += details.lines().count() as u16;
                                let _ = writeln!(out, "{}", details);
                            }
                            let _ = execute!(out, ResetColor);
                        }

                        // Print error messages from Activity::Message variants
                        for (text, details) in activity_errors {
                            let _ = execute!(out, SetForegroundColor(Color::AnsiValue(160)));
                            let _ = writeln!(out, "{}", text);
                            final_render_height += 1;
                            if let Some(details) = details {
                                final_render_height += details.lines().count() as u16;
                                let _ = writeln!(out, "{}", details);
                            }
                            let _ = execute!(out, ResetColor);
                        }

                        // Print build stderr (from failed or incomplete builds)
                        for (name, lines) in failed_build_errors {
                            let _ = execute!(out, SetForegroundColor(Color::AnsiValue(160)));
                            let _ = writeln!(out, "Build error: {}", name);
                            final_render_height += 1;
                            for line in lines {
                                let _ = writeln!(out, "  {}", line);
                                final_render_height += 1;
                            }
                            let _ = execute!(out, ResetColor);
                        }
                    }
                }
            }
        }

        Ok(final_render_height)
    }
}

/// Save the current terminal state before starting the TUI.
///
/// Must be called before iocraft's render_loop enters raw mode, so we have
/// the original (cooked) terminal settings to restore later. This is more
/// robust than relying on crossterm's `disable_raw_mode()`, which only works
/// if crossterm's own `enable_raw_mode()` was used to enter raw mode.
pub fn save_terminal_state() {
    #[cfg(unix)]
    {
        use std::os::unix::io::{AsRawFd, BorrowedFd};
        // Prefer the dedicated tty fd (which always points to the real terminal)
        // over stdin, since fd 0 might not be a terminal in some setups.
        let raw_fd = TTY_FD
            .get()
            .copied()
            .unwrap_or_else(|| io::stdin().as_raw_fd());
        let fd = unsafe { BorrowedFd::borrow_raw(raw_fd) };
        if !rustix::termios::isatty(&fd) {
            return;
        }
        if let Ok(termios) = rustix::termios::tcgetattr(&fd) {
            ORIGINAL_TERMIOS.get_or_init(|| termios);
        }
    }
}

/// Restore terminal to normal state.
/// Register this on panic to restore terminal state if the app crashes without running Drop.
pub fn restore_terminal() {
    // Use the tty fd for writing escape sequences if available.
    // During TUI, fd 2 (stderr) may be redirected to /dev/null, so we need
    // the dedicated tty fd to reach the actual terminal.
    #[cfg(unix)]
    let mut writer: Box<dyn Write> = match TTY_FD.get().copied() {
        Some(fd) => Box::new(FdWriter(fd)),
        None => Box::new(io::stderr()),
    };
    #[cfg(not(unix))]
    let mut writer: Box<dyn Write> = Box::new(io::stderr());

    // Restore original terminal settings saved before TUI started.
    // This is the authoritative restoration — it always restores the
    // exact terminal state from before the TUI was initialized.
    #[cfg(unix)]
    if let Some(original) = ORIGINAL_TERMIOS.get() {
        use std::os::unix::io::{AsRawFd, BorrowedFd};
        let raw_fd = TTY_FD
            .get()
            .copied()
            .unwrap_or_else(|| io::stdin().as_raw_fd());
        let fd = unsafe { BorrowedFd::borrow_raw(raw_fd) };
        let _ = rustix::termios::tcsetattr(&fd, rustix::termios::OptionalActions::Now, original);
    }

    // Pop keyboard enhancement flags if iocraft pushed them.
    // iocraft enables the Kitty keyboard protocol (PushKeyboardEnhancementFlags)
    // when entering raw mode on supported terminals. If the process exits without
    // iocraft's Drop running (exec, force-exit, panic), the terminal is left in
    // enhanced key reporting mode. The user's shell doesn't understand these
    // enhanced key codes, so they appear as literal escape sequences.
    // Sending PopKeyboardEnhancementFlags when enhancement isn't active is harmless.
    let _ = execute!(writer, event::PopKeyboardEnhancementFlags);

    // Show cursor (TUI may have hidden it)
    let _ = execute!(writer, cursor::Show);

    // Ensure output is flushed
    let _ = writer.flush();
}

/// Main TUI component (inline mode)
#[component]
fn MainView(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let config = hooks.use_context::<Arc<TuiConfig>>();
    let activity_model = hooks.use_context::<Arc<RwLock<ActivityModel>>>();
    let ui_state = hooks.use_context::<Arc<RwLock<UiState>>>();
    let notify = hooks.use_context::<Arc<Notify>>();
    let (terminal_width, terminal_height) = hooks.use_terminal_size();
    let mut should_exit = hooks.use_state(|| false);
    let shutdown = hooks.use_context::<Arc<Shutdown>>();
    let mut system = hooks.use_context_mut::<SystemContext>();

    // Redraw when notified of activity model changes (throttled)
    let redraw = hooks.use_state(|| 0u64);
    hooks.use_future({
        let notify = notify.clone();
        let max_fps = config.max_fps;
        async move {
            crate::throttled_notify_loop(notify, redraw, max_fps).await;
        }
    });

    // Track terminal size changes (update UiState, no activity model lock needed)
    let mut prev_size = hooks.use_state(crate::TerminalSize::default);
    let current_size = crate::TerminalSize {
        width: terminal_width,
        height: terminal_height,
    };
    if current_size != prev_size.get() {
        prev_size.set(current_size);
        if let Ok(mut ui) = ui_state.write() {
            ui.set_terminal_size(current_size.width, current_size.height);
        }
    }

    // Get optional command sender for process control
    let command_tx = hooks.use_context::<Option<mpsc::Sender<ProcessCommand>>>();

    // Handle keyboard events - only UI state updates, no activity model writes
    hooks.use_terminal_events({
        let activity_model = activity_model.clone();
        let ui_state = ui_state.clone();
        let shutdown = shutdown.clone();
        let command_tx = command_tx.clone();

        move |event| {
            if let TerminalEvent::Key(key_event) = event
                && key_event.kind != KeyEventKind::Release
            {
                debug!("Key event: {:?}", key_event);
                match key_event.code {
                    KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Set signal so Nix backend knows to interrupt operations
                        shutdown.set_last_signal(Signal::SIGINT);
                        shutdown.shutdown();
                    }
                    KeyCode::Char('r') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Restart selected process
                        if let Some(tx) = command_tx.as_ref()
                            && let Ok(ui) = ui_state.read()
                            && let Some(activity_id) = ui.selected_activity
                        {
                            // Get the process name from the activity
                            if let Ok(model) = activity_model.read()
                                && let Some(activity) = model.get_activity(activity_id)
                                && matches!(
                                    activity.variant,
                                    crate::model::ActivityVariant::Process(_)
                                )
                            {
                                let _ = tx.try_send(ProcessCommand::Restart(activity.name.clone()));
                            }
                        }
                    }
                    KeyCode::Char('e') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        if let Ok(mut ui) = ui_state.write()
                            && let Some(activity_id) = ui.selected_activity
                        {
                            ui.view_mode = ViewMode::ExpandedLogs { activity_id };
                            should_exit.set(true);
                        }
                    }
                    KeyCode::Down => {
                        // Get selectable IDs from activity model (read-only)
                        if let Ok(model) = activity_model.read() {
                            let selectable = model.get_selectable_activity_ids();
                            if let Ok(mut ui) = ui_state.write() {
                                ui.select_next_activity(&selectable);
                            }
                        }
                    }
                    KeyCode::Up => {
                        // Get selectable IDs from activity model (read-only)
                        if let Ok(model) = activity_model.read() {
                            let selectable = model.get_selectable_activity_ids();
                            if let Ok(mut ui) = ui_state.write() {
                                ui.select_previous_activity(&selectable);
                            }
                        }
                    }
                    KeyCode::Esc => {
                        if let Ok(mut ui) = ui_state.write() {
                            ui.selected_activity = None;
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    // Exit for explicit view mode switch (user pressed 'e' to expand)
    // Note: We do NOT exit on shutdown.is_cancelled() - we keep running until
    // the backend is fully done so all events are processed and displayed.
    if should_exit.get() {
        system.exit();
    }

    // Render the view - read activity model briefly, UI state separately
    let ui = ui_state.read().unwrap();
    let (rendered, last_render_height) = if let Ok(model_guard) = activity_model.read() {
        let mut measure = element! {
            View(width: terminal_width) {
                #(vec![view(&model_guard, &ui, RenderContext::Normal).into()])
            }
        };
        let height = measure.render(Some(terminal_width as usize)).height() as u16;
        let rendered = element! {
            View(width: terminal_width) {
                #(vec![view(&model_guard, &ui, RenderContext::Normal).into()])
            }
        };
        (rendered, height)
    } else {
        (element!(View(width: terminal_width)), 0)
    };
    drop(ui);
    if let Ok(mut ui) = ui_state.write() {
        ui.last_render_height = last_render_height;
    }

    rendered
}

#[allow(clippy::too_many_arguments)]
async fn run_view(
    activity_model: Arc<RwLock<ActivityModel>>,
    ui_state: Arc<RwLock<UiState>>,
    notify: Arc<Notify>,
    shutdown: Arc<Shutdown>,
    config: Arc<TuiConfig>,
    command_tx: Option<mpsc::Sender<ProcessCommand>>,
    pre_expand_height: &mut u16,
    _tty_fd: Option<i32>,
) -> std::io::Result<()> {
    // Copy view_mode in a block to ensure the guard is dropped before any await
    let view_mode = {
        let guard = ui_state.read().unwrap();
        guard.view_mode
    };

    match view_mode {
        ViewMode::Main => {
            if *pre_expand_height > 0 {
                #[cfg(unix)]
                let mut out = tui_writer(_tty_fd);
                #[cfg(not(unix))]
                let mut out = io::stderr();
                let _ = execute!(
                    out,
                    cursor::MoveToPreviousLine(*pre_expand_height),
                    terminal::Clear(terminal::ClearType::FromCursorDown)
                );
                *pre_expand_height = 0;
            }

            let mut element = element! {
                ContextProvider(value: Context::owned(config.clone())) {
                    ContextProvider(value: Context::owned(shutdown.clone())) {
                        ContextProvider(value: Context::owned(notify.clone())) {
                            ContextProvider(value: Context::owned(activity_model.clone())) {
                                ContextProvider(value: Context::owned(ui_state.clone())) {
                                    ContextProvider(value: Context::owned(command_tx.clone())) {
                                        MainView
                                    }
                                }
                            }
                        }
                    }
                }
            };

            #[cfg(unix)]
            let render_loop = {
                let mut rl = element.render_loop().output(Output::Stderr).ignore_ctrl_c();
                if let Some(fd) = _tty_fd {
                    rl = rl.stderr(FdWriter(fd));
                }
                rl
            };
            #[cfg(not(unix))]
            let render_loop = element.render_loop().output(Output::Stderr).ignore_ctrl_c();

            render_loop.await
        }
        ViewMode::ExpandedLogs { activity_id } => {
            // Calculate height before switching to expanded view
            // Use a block to ensure guards are dropped before await
            *pre_expand_height = {
                let ui = ui_state.read().unwrap();
                let model = activity_model.read().unwrap();
                let (terminal_width, _) = crossterm::terminal::size().unwrap_or((80, 24));
                let mut normal_view = element! {
                    View(width: terminal_width) {
                        #(vec![view(&model, &ui, RenderContext::Normal).into()])
                    }
                };
                normal_view.render(Some(terminal_width as usize)).height() as u16
            };

            let mut element = element! {
                ContextProvider(value: Context::owned(config.clone())) {
                    ContextProvider(value: Context::owned(shutdown.clone())) {
                        ContextProvider(value: Context::owned(notify.clone())) {
                            ContextProvider(value: Context::owned(activity_model.clone())) {
                                ContextProvider(value: Context::owned(ui_state.clone())) {
                                    ContextProvider(value: Context::owned(activity_id)) {
                                        ExpandedLogView
                                    }
                                }
                            }
                        }
                    }
                }
            };

            #[cfg(unix)]
            let render_loop = {
                let mut rl = element.fullscreen().output(Output::Stderr).ignore_ctrl_c();
                if let Some(fd) = _tty_fd {
                    rl = rl.stderr(FdWriter(fd));
                }
                rl
            };
            #[cfg(not(unix))]
            let render_loop = element.fullscreen().ignore_ctrl_c();

            render_loop.await
        }
    }
}
