use crate::{
    config::{Action, KeyContext, KeyMatch, KeySequenceState},
    config::{TuiPreferences, TuiRunContext},
    expanded_view::ExpandedLogView,
    inline_terminal::InlineTerminal,
    model::{ActivityModel, RenderContext, UiState, ViewMode},
    view::{
        ActivityHeights, ScrollState, activity_shows_inline_logs, available_activity_height,
        process_previews_fit, view,
    },
};
use crossterm::{
    cursor, event, execute,
    style::{Color, ResetColor, SetForegroundColor},
    terminal,
};
use devenv_activity::{ActivityEvent, ActivityLevel};
use devenv_mailbox::{FrontendCommand, FrontendEvent, ProcessCommand};
use iocraft::prelude::*;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use tokio::sync::{Notify, mpsc};
use tokio_shutdown::Shutdown;
use tracing::{debug, warn};

/// Process commands share the existing frontend/backend mailbox. Keeping a
/// second queue here made overload ambiguous: input could be accepted by the
/// view while still waiting to enter the real mailbox. A full mailbox means
/// the backend is busy, so the input remains unaccepted and can be retried.
pub(crate) type ProcessCommandSender = mpsc::Sender<FrontendEvent>;

fn enqueue_process_command(tx: &ProcessCommandSender, command: ProcessCommand) -> bool {
    match tx.try_send(FrontendEvent::Process(command)) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Closed(_)) => {
            warn!("process command receiver closed");
            false
        }
        Err(mpsc::error::TrySendError::Full(_)) => {
            debug!("process command ignored while backend mailbox is full");
            false
        }
    }
}

/// Newtype around an `Arc<Notify>` used to bypass the render throttle on
/// shutdown. Distinct from the regular activity-change notify so iocraft's
/// type-keyed context lookup can resolve them independently.
#[derive(Clone)]
pub struct RenderShutdown(pub Arc<Notify>);

/// Monotonic counter bumped by the event processor whenever the activity model
/// changes. The render loop reads it to skip layout-recomputing redraws while
/// idle (see `throttled_notify_loop`). Provided to components via context.
#[derive(Clone)]
pub struct ModelVersion(pub Arc<AtomicU64>);

/// Cooperative exit flag for TUI shutdown.
///
/// The event processor sets this when the backend is done, and TUI components
/// check it each render cycle to call `system.exit()`. This avoids cancelling
/// iocraft's render loop mid frame, which can leave the cursor at the wrong
/// position and overwrite the shell prompt.
#[derive(Clone)]
pub struct ExitFlag(Arc<AtomicBool>);

impl Default for ExitFlag {
    fn default() -> Self {
        Self::new()
    }
}

/// A backend request to hand the terminal over for interaction (for example a
/// sudo prompt), serviced by the render loop.
enum PauseSlot {
    Idle,
    Pending {
        ready: std::sync::mpsc::SyncSender<()>,
        resume: std::sync::mpsc::Receiver<()>,
    },
    /// The render loop is gone. Requests are dropped on arrival so the backend
    /// observes a closed channel instead of waiting forever.
    Closed,
}

/// Cooperative flag used to release and reacquire the terminal temporarily.
#[derive(Clone)]
pub struct PauseFlag(Arc<AtomicBool>);

impl PauseFlag {
    fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    fn set(&self, value: bool) {
        self.0.store(value, Ordering::Release);
    }

    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl ExitFlag {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    /// Signal that the TUI should exit. Must be called before `Notify::notify_waiters()`
    /// so the triggered re render sees the flag.
    pub fn set(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Original terminal settings saved before TUI enters raw mode.
static ORIGINAL_TERMIOS: OnceLock<libc::termios> = OnceLock::new();

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
    /// Set by the backend when this run is attached to an already-running
    /// process manager. Shared with the backend so the interrupt prompt can
    /// offer detach vs stop instead of the in-process keep-running vs quit.
    pub attached: Arc<AtomicBool>,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            event_batch_size: 64,
            max_log_messages: 1000,
            max_log_lines_per_build: 1000,
            log_viewport_collapsed: 10,
            max_fps: 60,
            filter_level: ActivityLevel::Info,
            attached: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Builder for creating and running the TUI application.
pub struct TuiApp {
    config: TuiConfig,
    preferences: TuiPreferences,
    run_context: TuiRunContext,
    activity_rx: mpsc::UnboundedReceiver<ActivityEvent>,
    frontend_rx: mpsc::Receiver<FrontendCommand>,
    shutdown: Arc<Shutdown>,
    event_tx: Option<mpsc::Sender<FrontendEvent>>,
}

impl TuiApp {
    /// Create a new TUI application with required dependencies.
    pub fn new(
        activity_rx: mpsc::UnboundedReceiver<ActivityEvent>,
        frontend_rx: mpsc::Receiver<FrontendCommand>,
        shutdown: Arc<Shutdown>,
    ) -> Self {
        Self {
            config: TuiConfig::default(),
            preferences: TuiPreferences::default(),
            run_context: TuiRunContext::default(),
            activity_rx,
            frontend_rx,
            shutdown,
            event_tx: None,
        }
    }

    /// Set the event sender for frontend input and process-control commands.
    pub fn with_event_sender(mut self, tx: mpsc::Sender<FrontendEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    pub fn with_preferences(mut self, preferences: TuiPreferences) -> Self {
        self.config.max_log_lines_per_build = preferences.behavior.log_history_lines;
        self.config.log_viewport_collapsed = preferences.behavior.log_preview_lines;
        self.preferences = preferences;
        self
    }

    pub fn with_run_context(mut self, context: TuiRunContext) -> Self {
        self.run_context = context;
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

    /// Run the TUI application until its activity producer closes or it
    /// receives [`FrontendCommand::ExitRenderer`]. Returns the command receiver so
    /// the frontend can hand it to the shell session after releasing the
    /// terminal.
    pub async fn run(self) -> std::io::Result<mpsc::Receiver<FrontendCommand>> {
        let config = Arc::new(self.config);
        let activity_model = Arc::new(RwLock::new(ActivityModel::with_config(config.clone())));
        let notify = Arc::new(Notify::new());
        // Bumped on every activity-model change so the render loop can tell a
        // real update from an idle safety-net wake and avoid redrawing (and
        // recomputing the layout) when nothing changed.
        let model_version = Arc::new(AtomicU64::new(0));
        // Separate notify for shutdown — bypasses render throttle so the
        // cooperative exit flag is observed promptly instead of waiting for
        // the next throttle tick (which can lose intermediate notifies).
        let render_shutdown = Arc::new(Notify::new());
        let shutdown = self.shutdown;
        let process_command_tx = self.event_tx;

        let exit_flag = ExitFlag::new();
        let pause_flag = PauseFlag::new();
        let pause_request = Arc::new(std::sync::Mutex::new(PauseSlot::Idle));

        // Spawn event processor with batching for performance
        // This only writes to ActivityModel, never touches UiState
        let event_processor_handle = tokio::spawn({
            let activity_model = activity_model.clone();
            let notify = notify.clone();
            let model_version = model_version.clone();
            let render_shutdown = render_shutdown.clone();
            let exit_flag = exit_flag.clone();
            let pause_flag = pause_flag.clone();
            let pause_request = Arc::clone(&pause_request);
            let config = config.clone();
            let mut activity_rx = self.activity_rx;
            let mut frontend_rx = self.frontend_rx;
            async move {
                let batch_size = config.event_batch_size.max(1);
                let mut batch = Vec::with_capacity(batch_size);
                let mut exit_requested = false;

                while !exit_requested {
                    tokio::select! {
                        event = activity_rx.recv() => match event {
                            Some(event) => batch.push(event),
                            // Channel closed: producer is gone, stop rendering.
                            None => exit_requested = true,
                        },
                        command = frontend_rx.recv() => match command {
                            Some(FrontendCommand::ExitRenderer) => exit_requested = true,
                            Some(FrontendCommand::SetAttached(attached)) => {
                                config.attached.store(attached, Ordering::Relaxed);
                            }
                            Some(FrontendCommand::PauseForInteraction { ready, resume }) => {
                                let mut slot =
                                    pause_request.lock().unwrap_or_else(|e| e.into_inner());
                                if matches!(*slot, PauseSlot::Closed) {
                                    // Nobody can hand the terminal over any more.
                                    // Dropping `ready` tells the backend so.
                                    drop((ready, resume));
                                } else {
                                    *slot = PauseSlot::Pending { ready, resume };
                                    drop(slot);
                                    pause_flag.set(true);
                                    model_version.fetch_add(1, Ordering::Release);
                                    notify.notify_waiters();
                                    render_shutdown.notify_waiters();
                                }
                            }
                            // Shell commands arrive after ExitRenderer and are
                            // therefore left queued for ShellSession. Seeing
                            // one here violates the mailbox ordering contract.
                            Some(FrontendCommand::Shell(_)) => {
                                unreachable!("shell command received before renderer exit")
                            }
                            // The backend is gone and no further lifecycle
                            // command can arrive. Drain queued activity below,
                            // then release the terminal.
                            None => exit_requested = true,
                        },
                    }

                    // Exit is ordered after the backend quiesces activity
                    // producers. Drain all events already in the queue so the
                    // final render includes their completions, while retaining
                    // the normal bounded batch size for a busy backend.
                    loop {
                        while batch.len() < batch_size {
                            let Ok(event) = activity_rx.try_recv() else {
                                break;
                            };
                            batch.push(event);
                        }

                        let mut any_changed = false;
                        if let Ok(mut m) = activity_model.write() {
                            for event in batch.drain(..) {
                                any_changed |= m.apply_activity_event(event);
                            }
                        }

                        // Only wake the render loop when the batch actually
                        // changed the visible model; pure no-op events (e.g.
                        // shell events, skipped .narinfo fetches) don't force
                        // a layout-recomputing redraw.
                        if any_changed && !exit_requested {
                            model_version.fetch_add(1, Ordering::Release);
                            notify.notify_waiters();
                        }

                        if !exit_requested || activity_rx.is_empty() {
                            break;
                        }
                    }
                }

                // Signal the component to exit cooperatively. Set before
                // notify so the triggered re-render sees the flag; bump the
                // version so the render loop treats it as a real change, and
                // bypass the render throttle so the flag is observed on the
                // next frame.
                exit_flag.set();
                model_version.fetch_add(1, Ordering::Release);
                notify.notify_waiters();
                render_shutdown.notify_waiters();
                frontend_rx
            }
        });

        // UiState is separate from ActivityModel to avoid lock contention.
        // The event processor only writes to ActivityModel, never UiState.
        // UiState is only modified by the UI thread.
        let viewport = self.preferences.viewport;
        let mut ui_state = UiState::new();
        ui_state.hide_stopped_processes = self.preferences.behavior.hide_stopped_processes;
        ui_state
            .set_preferences(self.preferences)
            .map_err(io::Error::other)?;
        let mut inline_terminal = InlineTerminal::new(viewport)?;
        ui_state.run_context = Arc::new(self.run_context);
        let ui_state = Arc::new(RwLock::new(ui_state));

        // Main loop - runs until backend signals completion via exit_flag.
        // The render loop exits cooperatively: the component checks exit_flag
        // each render cycle and calls system.exit() when set, ensuring iocraft
        // always completes its current frame before returning. This prevents
        // the cursor position race that occurs when cancelling mid-frame.
        loop {
            let view_result = run_view(
                activity_model.clone(),
                ui_state.clone(),
                notify.clone(),
                model_version.clone(),
                render_shutdown.clone(),
                shutdown.clone(),
                config.clone(),
                process_command_tx.clone(),
                exit_flag.clone(),
                pause_flag.clone(),
                &mut inline_terminal,
            )
            .await;

            // run_view returned either because:
            // - The backend is done (exit_flag is set)
            // - The user switched view modes (e.g. pressed 'e' for expanded)
            // - The terminal errored (e.g. the tty was revoked)
            if exit_flag.is_set() {
                break;
            }

            if pause_flag.is_set() {
                // The terminal owner knows the actual normal-buffer viewport,
                // including the frame restored after leaving fullscreen.
                let handoff_result = inline_terminal.clear().and(inline_terminal.suspend());
                if let Err(error) = handoff_result {
                    tracing::warn!(%error, "terminal handoff failed, stopping TUI");
                    break;
                }
                let request = std::mem::replace(
                    &mut *pause_request.lock().unwrap_or_else(|e| e.into_inner()),
                    PauseSlot::Idle,
                );
                if let PauseSlot::Pending { ready, resume } = request {
                    let _ = tokio::task::spawn_blocking(move || {
                        let _ = ready.send(());
                        let _ = resume.recv();
                    })
                    .await;
                }
                if let Err(error) = inline_terminal
                    .reanchor()
                    .and_then(|()| inline_terminal.resume())
                {
                    tracing::warn!(%error, "terminal resume failed, stopping TUI");
                    break;
                }
                pause_flag.set(false);
                model_version.fetch_add(1, Ordering::Release);
                notify.notify_waiters();
                continue;
            }

            if let Err(e) = view_result {
                // A dead terminal fails without suspending; re-entering would
                // busy-loop and starve the event processor.
                tracing::warn!(error = %e, "terminal render failed, stopping TUI");
                break;
            }
        }

        // No render loop remains to hand the terminal over. Close the slot so a
        // request stored since the last check, or one arriving later, is
        // dropped rather than left waiting on a renderer that is gone.
        *pause_request.lock().unwrap_or_else(|e| e.into_inner()) = PauseSlot::Closed;

        // Wait for event processor to finish draining events before final render.
        // This ensures all activity completion events are processed and visible.
        let renderer_rx = event_processor_handle
            .await
            .map_err(std::io::Error::other)?;

        // No view retains a sender now. Accepted commands are already in the
        // frontend mailbox; there is no intermediate queue to flush.
        drop(process_command_tx);

        {
            let ui = ui_state.read().unwrap();
            if let Ok(model_guard) = activity_model.read() {
                let (terminal_width, _) = crossterm::terminal::size().unwrap_or((80, 24));

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

                    let mut element = element! {
                        View(width: terminal_width) {
                            #(vec![view(&model_guard, &ui, RenderContext::Final, None, shutdown.is_cancelled()).into()])
                        }
                    };
                    let canvas = element.render(Some(terminal_width as usize));
                    inline_terminal.commit(&canvas)?;
                    inline_terminal.suspend()?;

                    // Print full error messages in red (not truncated by TUI width)
                    let has_errors = !standalone_errors.is_empty()
                        || !activity_errors.is_empty()
                        || !failed_build_errors.is_empty();
                    if has_errors {
                        let mut stderr = io::stderr();
                        eprintln!();

                        // Print standalone error messages (no parent activity)
                        for (text, details) in standalone_errors {
                            let _ = execute!(stderr, SetForegroundColor(Color::AnsiValue(160)));
                            eprintln!("{}", text);
                            if let Some(details) = details {
                                eprintln!("{}", details);
                            }
                            let _ = execute!(stderr, ResetColor);
                        }

                        // Print error messages from Activity::Message variants
                        for (text, details) in activity_errors {
                            let _ = execute!(stderr, SetForegroundColor(Color::AnsiValue(160)));
                            eprintln!("{}", text);
                            if let Some(details) = details {
                                eprintln!("{}", details);
                            }
                            let _ = execute!(stderr, ResetColor);
                        }

                        // Print build stderr (from failed or incomplete builds)
                        for (name, lines) in failed_build_errors {
                            let _ = execute!(stderr, SetForegroundColor(Color::AnsiValue(160)));
                            eprintln!("Build error: {}", name);
                            for line in lines {
                                eprintln!("  {}", line);
                            }
                            let _ = execute!(stderr, ResetColor);
                        }
                    }
                }
            }
        }
        inline_terminal.suspend()?;

        Ok(renderer_rx)
    }
}

pub(crate) fn request_interrupt_prompt(
    event_tx: Option<&ProcessCommandSender>,
    ui_state: &Arc<RwLock<UiState>>,
    attached: bool,
) -> bool {
    if event_tx.is_none() {
        return false;
    }

    if let Ok(mut ui) = ui_state.write() {
        ui.show_interrupt_prompt(attached);
        true
    } else {
        false
    }
}

#[cfg(test)]
pub(crate) fn handle_interrupt_prompt_key(
    key_event: &KeyEvent,
    ui_state: &Arc<RwLock<UiState>>,
    shutdown: &Arc<Shutdown>,
    event_tx: Option<&ProcessCommandSender>,
) -> bool {
    let action = match key_event.code {
        KeyCode::Char('s') => Some(Action::StopManager),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Esc | KeyCode::Char('c') => Some(Action::Cancel),
        _ => None,
    };
    handle_interrupt_prompt_action(
        action,
        crate::config::is_emergency_interrupt(key_event.code, key_event.modifiers),
        ui_state,
        shutdown,
        event_tx,
    )
}

pub(crate) fn handle_interrupt_prompt_action(
    action: Option<Action>,
    emergency_interrupt: bool,
    ui_state: &Arc<RwLock<UiState>>,
    shutdown: &Arc<Shutdown>,
    event_tx: Option<&ProcessCommandSender>,
) -> bool {
    let (prompt_active, attached) = ui_state
        .read()
        .map(|ui| (ui.interrupt_prompt_active(), ui.interrupt_prompt_attached()))
        .unwrap_or((false, false));
    if !prompt_active {
        return false;
    }

    match action {
        _ if emergency_interrupt => {
            shutdown.handle_interrupt();
        }
        Some(Action::StopManager) if attached => {
            if event_tx.is_some_and(|tx| enqueue_process_command(tx, ProcessCommand::StopManager))
                && let Ok(mut ui) = ui_state.write()
            {
                ui.clear_interrupt_prompt();
            }
        }
        Some(Action::Quit) if !attached => {
            shutdown.handle_interrupt();
        }
        Some(Action::Cancel) => {
            if let Ok(mut ui) = ui_state.write() {
                ui.clear_interrupt_prompt();
            }
        }
        Some(_) | None => {}
    }

    true
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
        use std::os::unix::io::AsRawFd;
        let fd = io::stdin().as_raw_fd();
        if unsafe { libc::isatty(fd) } == 0 {
            return;
        }
        let mut termios: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut termios) } == 0 {
            ORIGINAL_TERMIOS.get_or_init(|| termios);
        }
    }
}

/// Restore terminal to normal state.
/// Register this on panic to restore terminal state if the app crashes without running Drop.
pub fn restore_terminal() {
    let mut stderr = io::stderr();

    // Restore original terminal settings saved before TUI started.
    // This is the authoritative restoration — it always restores the
    // exact terminal state from before the TUI was initialized.
    #[cfg(unix)]
    if let Some(original) = ORIGINAL_TERMIOS.get() {
        use std::os::unix::io::AsRawFd;
        let fd = io::stdin().as_raw_fd();
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, original) };
    }

    // Pop keyboard enhancement flags if iocraft pushed them.
    // iocraft enables the Kitty keyboard protocol (PushKeyboardEnhancementFlags)
    // when entering raw mode on supported terminals. If the process exits without
    // iocraft's Drop running (exec, force-exit, panic), the terminal is left in
    // enhanced key reporting mode. The user's shell doesn't understand these
    // enhanced key codes, so they appear as literal escape sequences.
    // Sending PopKeyboardEnhancementFlags when enhancement isn't active is harmless.
    let _ = execute!(stderr, event::PopKeyboardEnhancementFlags);

    // Show cursor (TUI may have hidden it)
    let _ = execute!(
        stderr,
        terminal::EndSynchronizedUpdate,
        terminal::EnableLineWrap,
        ResetColor,
        cursor::Show
    );

    // Ensure output is flushed
    let _ = stderr.flush();
}

fn activity_height(heights: &std::collections::HashMap<u64, i32>, id: u64) -> i32 {
    heights.get(&id).copied().unwrap_or(1)
}

fn rendered_activity_height(
    heights: &std::collections::HashMap<u64, i32>,
    model: &ActivityModel,
    ui_state: &UiState,
    display: &crate::model::DisplayActivity,
    previews_fit: bool,
) -> i32 {
    if matches!(
        display.activity.variant,
        crate::model::ActivityVariant::Process(_)
    ) && !activity_shows_inline_logs(model, ui_state, display.activity.id, previews_fit)
    {
        1
    } else {
        activity_height(heights, display.activity.id)
    }
}

fn activity_navigation_action(
    key_event: &KeyEvent,
    viewport_height: usize,
) -> Option<(bool, usize)> {
    let control = key_event.modifiers.contains(KeyModifiers::CONTROL);
    let half_page = viewport_height.div_ceil(2).max(1);

    match key_event.code {
        KeyCode::Down | KeyCode::Char('j') => Some((true, 1)),
        KeyCode::Up | KeyCode::Char('k') => Some((false, 1)),
        KeyCode::Char('d') if control => Some((true, half_page)),
        KeyCode::Char('u') if control => Some((false, half_page)),
        _ => None,
    }
}

fn canonical_key_event(action: Action) -> KeyEvent {
    let (code, modifiers) = match action {
        Action::MoveDown => (KeyCode::Down, KeyModifiers::NONE),
        Action::MoveUp => (KeyCode::Up, KeyModifiers::NONE),
        Action::HalfPageDown => (KeyCode::Char('d'), KeyModifiers::CONTROL),
        Action::HalfPageUp => (KeyCode::Char('u'), KeyModifiers::CONTROL),
        Action::Activate | Action::Accept => (KeyCode::Enter, KeyModifiers::NONE),
        Action::Expand => (KeyCode::Right, KeyModifiers::NONE),
        Action::Collapse => (KeyCode::Left, KeyModifiers::NONE),
        Action::OpenLogs => (KeyCode::Char('e'), KeyModifiers::CONTROL),
        Action::Search => (KeyCode::Char('/'), KeyModifiers::NONE),
        Action::RestartProcess => (KeyCode::Char('r'), KeyModifiers::CONTROL),
        Action::StopProcess => (KeyCode::Char('x'), KeyModifiers::CONTROL),
        Action::ToggleStopped => (KeyCode::Char('h'), KeyModifiers::CONTROL),
        Action::Cancel | Action::Back => (KeyCode::Esc, KeyModifiers::NONE),
        Action::NextMatch | Action::LineDown => (KeyCode::Down, KeyModifiers::NONE),
        Action::PreviousMatch | Action::LineUp => (KeyCode::Up, KeyModifiers::NONE),
        Action::PageDown => (KeyCode::PageDown, KeyModifiers::NONE),
        Action::PageUp => (KeyCode::PageUp, KeyModifiers::NONE),
        Action::Top => (KeyCode::Home, KeyModifiers::NONE),
        Action::Bottom => (KeyCode::End, KeyModifiers::NONE),
        Action::Copy => (KeyCode::Char('y'), KeyModifiers::NONE),
        Action::Quit => (KeyCode::Char('q'), KeyModifiers::NONE),
        Action::StopManager => (KeyCode::Char('s'), KeyModifiers::NONE),
    };
    let mut event = KeyEvent::new(KeyEventKind::Press, code);
    event.modifiers = modifiers;
    event
}

/// Scroll the viewport so the selected activity is visible.
fn scroll_selected_into_view(
    handle: &mut ScrollViewHandle,
    heights: &std::collections::HashMap<u64, i32>,
    model: &ActivityModel,
    ui_state: &UiState,
    display_activities: &[crate::model::DisplayActivity],
    selected_id: u64,
    previews_fit: bool,
) {
    let Some(position) = display_activities
        .iter()
        .position(|da| da.activity.id == selected_id)
    else {
        return;
    };

    let offset: i32 = display_activities[..position]
        .iter()
        .map(|display| rendered_activity_height(heights, model, ui_state, display, previews_fit))
        .sum();
    let target_height = display_activities
        .get(position)
        .map(|display| rendered_activity_height(heights, model, ui_state, display, previews_fit))
        .unwrap_or(1);

    let vp = handle.viewport_height() as i32;
    let current = handle.scroll_offset();
    if offset < current {
        handle.scroll_to(offset);
    } else if offset + target_height > current + vp {
        handle.scroll_to(offset + target_height - vp);
    }
}

fn update_process_search_selection(
    model: &ActivityModel,
    display: &[crate::model::DisplayActivity],
    ui_state: &mut UiState,
) {
    let Some(query) = ui_state
        .process_search
        .as_ref()
        .map(|search| search.query.clone())
    else {
        return;
    };
    let matches = model.get_matching_process_activity_ids_from_display(display, &query);
    if !ui_state
        .selected_activity
        .is_some_and(|id| matches.contains(&id))
    {
        ui_state.selected_activity = matches.first().copied();
    }
}

fn cancel_process_search(model: &ActivityModel, ui_state: &mut UiState) {
    ui_state.cancel_process_search();
    if ui_state
        .selected_activity
        .is_some_and(|id| !model.is_selectable(id, ui_state))
    {
        ui_state.selected_activity = None;
    }
}

fn handle_process_search_key(
    key_event: &KeyEvent,
    action: Option<Action>,
    key_consumed: bool,
    model: &ActivityModel,
    display: &[crate::model::DisplayActivity],
    ui_state: &mut UiState,
) -> bool {
    if ui_state.process_search.is_none() {
        return false;
    }
    match action {
        Some(Action::Cancel) => {
            cancel_process_search(model, ui_state);
        }
        None if crate::config::is_emergency_interrupt(key_event.code, key_event.modifiers) => {
            cancel_process_search(model, ui_state);
        }
        Some(Action::Accept) => ui_state.finish_process_search(),
        Some(Action::NextMatch | Action::PreviousMatch) => {
            let query = ui_state
                .process_search
                .as_ref()
                .map(|search| search.query.as_str())
                .unwrap_or_default();
            let matches = model.get_matching_process_activity_ids_from_display(display, query);
            if !ui_state
                .selected_activity
                .is_some_and(|id| matches.contains(&id))
            {
                ui_state.selected_activity = None;
            }
            ui_state.select_activity(&matches, action == Some(Action::NextMatch));
        }
        None if !key_consumed && key_event.code == KeyCode::Backspace => {
            if let Some(search) = &mut ui_state.process_search {
                search.query.pop();
            }
            update_process_search_selection(model, display, ui_state);
        }
        None if !key_consumed
            && matches!(key_event.code, KeyCode::Char(_))
            && !key_event.modifiers.contains(KeyModifiers::CONTROL)
            && !key_event.modifiers.contains(KeyModifiers::ALT) =>
        {
            let KeyCode::Char(character) = key_event.code else {
                unreachable!()
            };
            if let Some(search) = &mut ui_state.process_search {
                search.query.push(character);
            }
            update_process_search_selection(model, display, ui_state);
        }
        Some(_) | None => {}
    }

    true
}

fn activate_selected_activity(model: &ActivityModel, ui_state: &mut UiState, previews_fit: bool) {
    if let Some(activity_id) = ui_state.selected_activity
        && model.is_activity_collapsible(activity_id, ui_state)
    {
        ui_state.toggle_activity_expansion(activity_id);
        ui_state.inline_logs_activity = None;
    } else if let Some(activity_id) = ui_state.selected_activity
        && model.get_activity(activity_id).is_some_and(|activity| {
            matches!(activity.variant, crate::model::ActivityVariant::Process(_))
        })
    {
        if activity_shows_inline_logs(model, ui_state, activity_id, previews_fit) {
            ui_state.hide_process_previews();
        } else {
            ui_state.focus_inline_logs(activity_id);
        }
    } else {
        ui_state.toggle_inline_logs();
    }
}

fn expand_selected_activity(model: &ActivityModel, ui_state: &mut UiState) {
    let Some(activity_id) = ui_state.selected_activity else {
        return;
    };
    if model.is_activity_collapsible(activity_id, ui_state) {
        ui_state.expanded_activities.insert(activity_id);
        ui_state.inline_logs_activity = None;
    } else if model.get_activity(activity_id).is_some_and(|activity| {
        matches!(activity.variant, crate::model::ActivityVariant::Process(_))
    }) {
        ui_state.focus_inline_logs(activity_id);
    }
}

fn collapse_selected_activity(model: &ActivityModel, ui_state: &mut UiState) {
    let Some(activity_id) = ui_state.selected_activity else {
        return;
    };
    if model.is_activity_collapsible(activity_id, ui_state) {
        ui_state.expanded_activities.remove(&activity_id);
        ui_state.inline_logs_activity = None;
    } else if model.get_activity(activity_id).is_some_and(|activity| {
        matches!(activity.variant, crate::model::ActivityVariant::Process(_))
    }) {
        ui_state.hide_process_previews();
    }
}

fn hide_selected_preview(
    model: &ActivityModel,
    ui_state: &mut UiState,
    previews_fit: bool,
) -> bool {
    let Some(activity_id) = ui_state.selected_activity else {
        return false;
    };
    if !activity_shows_inline_logs(model, ui_state, activity_id, previews_fit) {
        return false;
    }
    if model.get_activity(activity_id).is_some_and(|activity| {
        matches!(activity.variant, crate::model::ActivityVariant::Process(_))
    }) {
        ui_state.hide_process_previews();
    } else {
        ui_state.inline_logs_activity = None;
    }
    true
}

/// Main TUI component (inline mode)
#[component]
fn MainView(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let config = hooks.use_context::<Arc<TuiConfig>>();
    let activity_model = hooks.use_context::<Arc<RwLock<ActivityModel>>>();
    let ui_state = hooks.use_context::<Arc<RwLock<UiState>>>();
    let notify = hooks.use_context::<Arc<Notify>>();
    let model_version = hooks.use_context::<ModelVersion>().0.clone();
    let render_shutdown = hooks.use_context::<RenderShutdown>().0.clone();
    let (terminal_width, terminal_height) = hooks.use_terminal_size();
    let mut should_exit = hooks.use_state(|| false);
    let shutdown = hooks.use_context::<Arc<Shutdown>>();
    let mut system = hooks.use_context_mut::<SystemContext>();

    // ScrollView handle and per-activity height measurements
    let scroll_handle = hooks.use_ref_default::<ScrollViewHandle>();
    let mut activity_heights: ActivityHeights = hooks.use_ref_default();
    // Tracks whether the ScrollView is currently rendered (and handle is valid)
    let mut scroll_view_active = hooks.use_ref_default::<bool>();

    // Redraw when notified of activity model changes (throttled)
    let redraw = hooks.use_state(|| 0u64);
    hooks.use_future({
        let notify = notify.clone();
        let model_version = model_version.clone();
        let render_shutdown = render_shutdown.clone();
        let max_fps = config.max_fps;
        async move {
            crate::throttled_notify_loop(notify, render_shutdown, model_version, redraw, max_fps)
                .await;
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
    let event_tx = hooks.use_context::<Option<ProcessCommandSender>>();
    let keymap = ui_state.read().ok().map(|ui| ui.keymap().clone());
    let key_sequence = hooks.use_state(KeySequenceState::default);
    let key_sequence_wake = hooks.use_ref(|| Arc::new(Notify::new()));
    let key_sequence_wake_for_timer = key_sequence_wake.read().clone();
    hooks.use_future({
        let ui_state = ui_state.clone();
        let notify = notify.clone();
        let mut key_sequence = key_sequence;
        async move {
            loop {
                key_sequence_wake_for_timer.notified().await;
                loop {
                    let remaining = key_sequence.read().remaining_timeout();
                    let Some(remaining) = remaining else {
                        break;
                    };
                    tokio::select! {
                        _ = tokio::time::sleep(remaining) => {
                            if key_sequence.write().expire() {
                                if let Ok(mut ui) = ui_state.write() {
                                    ui.pending_key = None;
                                }
                                notify.notify_one();
                            }
                            break;
                        }
                        _ = key_sequence_wake_for_timer.notified() => {}
                    }
                }
            }
        }
    });

    // Handle keyboard events - only UI state updates, no activity model writes
    hooks.use_terminal_events({
        let activity_model = activity_model.clone();
        let ui_state = ui_state.clone();
        let shutdown = shutdown.clone();
        let event_tx = event_tx.clone();
        let notify = notify.clone();
        let attached_flag = config.attached.clone();
        let mut scroll_handle = scroll_handle;
        let scroll_view_active = scroll_view_active;
        let keymap = keymap.clone();
        let mut key_sequence = key_sequence;
        let key_sequence_wake = key_sequence_wake.read().clone();

        move |event| {
            if let TerminalEvent::Key(raw_key_event) = event
                && raw_key_event.kind != KeyEventKind::Release
            {
                let context = ui_state
                    .read()
                    .map(|ui| {
                        if ui.interrupt_prompt_active() {
                            KeyContext::Prompt
                        } else if ui.process_search.is_some() {
                            KeyContext::ProcessSearch
                        } else {
                            KeyContext::Main
                        }
                    })
                    .unwrap_or(KeyContext::Main);
                let emergency_interrupt = crate::config::is_emergency_interrupt(
                    raw_key_event.code,
                    raw_key_event.modifiers,
                );
                let (key_match, pending_key) = {
                    let mut sequence = key_sequence.write();
                    let key_match = if emergency_interrupt {
                        sequence.clear();
                        KeyMatch::None
                    } else if let Some(keymap) = keymap.as_deref() {
                        sequence.input_key(
                            keymap,
                            context,
                            raw_key_event.code,
                            raw_key_event.modifiers,
                        )
                    } else {
                        KeyMatch::None
                    };
                    (key_match, sequence.pending_label())
                };
                key_sequence_wake.notify_one();
                if let Ok(mut ui) = ui_state.write() {
                    ui.pending_key = pending_key;
                }
                let action = match key_match {
                    KeyMatch::Action(action) => Some(action),
                    KeyMatch::Prefix | KeyMatch::None => None,
                };
                let key_event = if emergency_interrupt {
                    raw_key_event.clone()
                } else {
                    action
                        .map(canonical_key_event)
                        .unwrap_or_else(|| KeyEvent::new(KeyEventKind::Press, KeyCode::Null))
                };
                debug!("Key event: {:?}", key_event);
                let search_handled = if let Ok(model) = activity_model.read()
                    && let Ok(mut ui) = ui_state.write()
                    && ui.process_search.is_some()
                {
                    let display = model.get_display_activities(&ui);
                    let handled = handle_process_search_key(
                        &raw_key_event,
                        action,
                        key_match != KeyMatch::None,
                        &model,
                        &display,
                        &mut ui,
                    );
                    if handled
                        && let Some(selected_id) = ui.selected_activity
                        && *scroll_view_active.read()
                    {
                        let previews_fit = process_previews_fit(&model, &display, &ui);
                        let heights = activity_heights.read();
                        scroll_selected_into_view(
                            &mut scroll_handle.write(),
                            &heights,
                            &model,
                            &ui,
                            &display,
                            selected_id,
                            previews_fit,
                        );
                    }
                    handled
                } else {
                    false
                };
                if !search_handled
                    && !handle_interrupt_prompt_action(
                        action,
                        emergency_interrupt,
                        &ui_state,
                        &shutdown,
                        event_tx.as_ref(),
                    )
                {
                    match key_event.code {
                        KeyCode::Char('c')
                            if crate::config::is_emergency_interrupt(
                                key_event.code,
                                key_event.modifiers,
                            ) =>
                        {
                            if !request_interrupt_prompt(
                                event_tx.as_ref(),
                                &ui_state,
                                attached_flag.load(Ordering::Relaxed),
                            ) {
                                shutdown.handle_interrupt();
                            }
                        }
                        KeyCode::Char('r')
                            if key_event.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            // Restart selected process
                            if key_event.kind == KeyEventKind::Press
                                && let Some(tx) = event_tx.as_ref()
                                && let Ok(ui) = ui_state.read()
                                && let Some(activity_id) = ui.selected_activity
                                && let Ok(model) = activity_model.read()
                                && let Some(activity) = model.get_activity(activity_id)
                                && matches!(
                                    activity.variant,
                                    crate::model::ActivityVariant::Process(ref proc)
                                        if proc.status.is_restartable()
                                )
                            {
                                enqueue_process_command(
                                    tx,
                                    ProcessCommand::Restart(activity.name.clone()),
                                );
                            }
                        }
                        KeyCode::Char('x')
                            if key_event.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            // Stop selected process (only if active)
                            if key_event.kind == KeyEventKind::Press
                                && let Some(tx) = event_tx.as_ref()
                                && let Ok(ui) = ui_state.read()
                                && let Some(activity_id) = ui.selected_activity
                                && let Ok(model) = activity_model.read()
                                && let Some(activity) = model.get_activity(activity_id)
                                && let crate::model::ActivityVariant::Process(ref proc) =
                                    activity.variant
                                && proc.status.is_stoppable()
                            {
                                enqueue_process_command(
                                    tx,
                                    ProcessCommand::Stop(activity.name.clone()),
                                );
                            }
                        }
                        KeyCode::Char('e')
                            if key_event.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            if let Ok(mut ui) = ui_state.write()
                                && let Some(activity_id) = ui.selected_activity
                            {
                                ui.view_mode = ViewMode::ExpandedLogs { activity_id };
                                should_exit.set(true);
                            }
                        }
                        KeyCode::Char('h')
                            if key_event.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            if let Ok(model) = activity_model.read()
                                && let Ok(mut ui) = ui_state.write()
                            {
                                ui.toggle_hide_stopped_processes();
                                if let Some(id) = ui.selected_activity
                                    && !model.is_selectable(id, &ui)
                                {
                                    ui.selected_activity = None;
                                    ui.inline_logs_activity = None;
                                }
                            }
                        }
                        KeyCode::Char('/')
                            if !key_event.modifiers.contains(KeyModifiers::CONTROL)
                                && !key_event.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            if let Ok(model) = activity_model.read()
                                && let Ok(mut ui) = ui_state.write()
                            {
                                ui.start_process_search();
                                let display = model.get_display_activities(&ui);
                                update_process_search_selection(&model, &display, &mut ui);
                                if let Some(selected_id) = ui.selected_activity
                                    && *scroll_view_active.read()
                                {
                                    let previews_fit = process_previews_fit(&model, &display, &ui);
                                    let heights = activity_heights.read();
                                    scroll_selected_into_view(
                                        &mut scroll_handle.write(),
                                        &heights,
                                        &model,
                                        &ui,
                                        &display,
                                        selected_id,
                                        previews_fit,
                                    );
                                }
                            }
                        }
                        KeyCode::Enter => {
                            if let Ok(model) = activity_model.read()
                                && let Ok(mut ui) = ui_state.write()
                            {
                                let display = model.get_display_activities(&ui);
                                let previews_fit = process_previews_fit(&model, &display, &ui);
                                activate_selected_activity(&model, &mut ui, previews_fit);
                            }
                        }
                        KeyCode::Right | KeyCode::Char('l')
                            if !key_event.modifiers.contains(KeyModifiers::CONTROL)
                                && !key_event.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            if let Ok(model) = activity_model.read()
                                && let Ok(mut ui) = ui_state.write()
                            {
                                expand_selected_activity(&model, &mut ui);
                            }
                        }
                        KeyCode::Left | KeyCode::Char('h')
                            if !key_event.modifiers.contains(KeyModifiers::CONTROL)
                                && !key_event.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            if let Ok(model) = activity_model.read()
                                && let Ok(mut ui) = ui_state.write()
                            {
                                collapse_selected_activity(&model, &mut ui);
                            }
                        }
                        _ if activity_navigation_action(&key_event, terminal_height as usize)
                            .is_some() =>
                        {
                            if let Ok(model) = activity_model.read()
                                && let Ok(mut ui) = ui_state.write()
                            {
                                let display = model.get_display_activities(&ui);
                                let selectable =
                                    model.get_selectable_activity_ids_from_display(&display, &ui);
                                let (forward, steps) = activity_navigation_action(
                                    &key_event,
                                    available_activity_height(&ui),
                                )
                                .unwrap();
                                ui.select_activity_by(&selectable, steps, forward);
                                ui.inline_logs_activity = None;
                                if let Some(selected_id) = ui.selected_activity
                                    && *scroll_view_active.read()
                                {
                                    let previews_fit = process_previews_fit(&model, &display, &ui);
                                    let heights = activity_heights.read();
                                    scroll_selected_into_view(
                                        &mut scroll_handle.write(),
                                        &heights,
                                        &model,
                                        &ui,
                                        &display,
                                        selected_id,
                                        previews_fit,
                                    );
                                }
                            }
                        }
                        KeyCode::Esc => {
                            if let Ok(model) = activity_model.read()
                                && let Ok(mut ui) = ui_state.write()
                            {
                                let display = model.get_display_activities(&ui);
                                let previews_fit = process_previews_fit(&model, &display, &ui);
                                if !hide_selected_preview(&model, &mut ui, previews_fit) {
                                    ui.selected_activity = None;
                                }
                            }
                            if ui_state
                                .read()
                                .is_ok_and(|ui| ui.selected_activity.is_none())
                                && *scroll_view_active.read()
                            {
                                scroll_handle.write().scroll_to_bottom();
                            }
                        }
                        _ => {}
                    }
                }

                // Key handlers above mutate `ui_state` (and the interrupt
                // prompt), which iocraft cannot observe on its own. Wake the
                // render loop so the change is painted promptly instead of
                // waiting for the idle heartbeat (#2915).
                notify.notify_one();
            }
        }
    });

    // Exit cooperatively when the backend signals completion via exit_flag.
    // This ensures the render loop completes its current frame before returning,
    // leaving the cursor at the correct position for the final render.
    let exit_flag = hooks.use_context::<ExitFlag>();
    let pause_flag = hooks.use_context::<PauseFlag>();
    if exit_flag.is_set() || pause_flag.is_set() {
        system.exit();
    }

    // Exit for explicit view mode switch (user pressed 'e' to expand)
    // Note: We do NOT exit on shutdown.is_cancelled() - we keep running until
    // the backend is fully done so all events are processed and displayed.
    if should_exit.get() {
        system.exit();
    }

    // Render the view - read activity model briefly, UI state separately
    let ui = ui_state.read().unwrap();
    let is_shutting_down = shutdown.is_cancelled();
    let rendered = if let Ok(model_guard) = activity_model.read() {
        let display = model_guard.get_display_activities(&ui);
        let previews_fit = process_previews_fit(&model_guard, &display, &ui);

        // Prune stale entries and compute total content height in a single lock
        let total_content_height: i32 = {
            let active_ids: std::collections::HashSet<u64> =
                display.iter().map(|da| da.activity.id).collect();
            let mut heights = activity_heights.write();
            heights.retain(|id, _| active_ids.contains(id));
            display
                .iter()
                .map(|display| {
                    rendered_activity_height(&heights, &model_guard, &ui, display, previews_fit)
                })
                .sum()
        };

        // Only enable ScrollView when content exceeds available terminal height.
        let available_height = available_activity_height(&ui) as i32;
        let scroll_handle_opt = if total_content_height > available_height {
            Some(scroll_handle)
        } else {
            None
        };
        *scroll_view_active.write() = scroll_handle_opt.is_some();

        element! {
            ContextProvider(value: iocraft::Context::owned(activity_heights)) {
                View(width: terminal_width) {
                    #(vec![view(&model_guard, &ui, RenderContext::Normal, Some(ScrollState { handle: scroll_handle_opt, display_activities: display, process_previews_fit: previews_fit }), is_shutting_down).into()])
                }
            }
        }
    } else {
        element!(ContextProvider(value: iocraft::Context::owned(activity_heights)) {
            View(width: terminal_width)
        })
    };
    drop(ui);

    rendered
}

#[allow(clippy::too_many_arguments)]
async fn run_view(
    activity_model: Arc<RwLock<ActivityModel>>,
    ui_state: Arc<RwLock<UiState>>,
    notify: Arc<Notify>,
    model_version: Arc<AtomicU64>,
    render_shutdown: Arc<Notify>,
    shutdown: Arc<Shutdown>,
    config: Arc<TuiConfig>,
    event_tx: Option<ProcessCommandSender>,
    exit_flag: ExitFlag,
    pause_flag: PauseFlag,
    inline_terminal: &mut InlineTerminal,
) -> std::io::Result<()> {
    // Copy view_mode in a block to ensure the guard is dropped before any await
    let view_mode = {
        let guard = ui_state.read().unwrap();
        guard.view_mode
    };

    match view_mode {
        ViewMode::Main => {
            let element = element! {
                ContextProvider(value: Context::owned(config.clone())) {
                    ContextProvider(value: Context::owned(shutdown.clone())) {
                        ContextProvider(value: Context::owned(notify.clone())) {
                            ContextProvider(value: Context::owned(ModelVersion(model_version.clone()))) {
                                ContextProvider(value: Context::owned(RenderShutdown(render_shutdown.clone()))) {
                                    ContextProvider(value: Context::owned(activity_model.clone())) {
                                        ContextProvider(value: Context::owned(ui_state.clone())) {
                                            ContextProvider(value: Context::owned(event_tx.clone())) {
                                                ContextProvider(value: Context::owned(exit_flag.clone())) {
                                                    ContextProvider(value: Context::owned(pause_flag.clone())) {
                                                        MainView
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            };

            inline_terminal.resume()?;
            inline_terminal.render_loop(element).await
        }
        ViewMode::ExpandedLogs { activity_id } => {
            let mouse_enabled = ui_state
                .read()
                .map(|ui| ui.preferences.behavior.mouse)
                .unwrap_or(true);
            let mut element = element! {
                ContextProvider(value: Context::owned(config.clone())) {
                    ContextProvider(value: Context::owned(shutdown.clone())) {
                        ContextProvider(value: Context::owned(notify.clone())) {
                            ContextProvider(value: Context::owned(ModelVersion(model_version.clone()))) {
                                ContextProvider(value: Context::owned(RenderShutdown(render_shutdown.clone()))) {
                                    ContextProvider(value: Context::owned(activity_model.clone())) {
                                        ContextProvider(value: Context::owned(ui_state.clone())) {
                                            ContextProvider(value: Context::owned(event_tx.clone())) {
                                                ContextProvider(value: Context::owned(exit_flag.clone())) {
                                                    ContextProvider(value: Context::owned(pause_flag.clone())) {
                                                        ContextProvider(value: Context::owned(activity_id)) {
                                                            ExpandedLogView
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            };

            inline_terminal.suspend()?;
            let mut render_loop = element.fullscreen().ignore_ctrl_c();
            if !mouse_enabled {
                render_loop = render_loop.disable_mouse_capture();
            }
            let result = render_loop.await;
            let resume_result = inline_terminal.resume();
            inline_terminal.invalidate();
            result.and(resume_result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devenv_activity::test_helpers::{
        build_complete, build_start_with, evaluate_complete, evaluate_start_with,
        operation_complete, operation_start, process_start,
    };
    use devenv_activity::{ActivityLevel, ActivityOutcome};
    use tokio::sync::mpsc;

    #[test]
    fn activity_navigation_supports_arrow_vim_and_half_page_keys() {
        let action = |code, control| {
            let mut event = KeyEvent::new(KeyEventKind::Press, code);
            if control {
                event.modifiers = KeyModifiers::CONTROL;
            }
            activity_navigation_action(&event, 21)
        };

        assert_eq!(action(KeyCode::Down, false), Some((true, 1)));
        assert_eq!(action(KeyCode::Up, false), Some((false, 1)));
        assert_eq!(action(KeyCode::Char('j'), false), Some((true, 1)));
        assert_eq!(action(KeyCode::Char('k'), false), Some((false, 1)));
        assert_eq!(action(KeyCode::Char('d'), true), Some((true, 11)));
        assert_eq!(action(KeyCode::Char('u'), true), Some((false, 11)));
        assert_eq!(action(KeyCode::Char('d'), false), None);

        let selectable: Vec<_> = (1..=20).collect();
        let mut ui_state = UiState::new();
        ui_state.selected_activity = Some(10);
        ui_state.select_activity_by(&selectable, 6, true);
        assert_eq!(ui_state.selected_activity, Some(16));
        ui_state.select_activity_by(&selectable, 50, false);
        assert_eq!(ui_state.selected_activity, Some(1));
    }

    #[test]
    fn test_request_interrupt_prompt_requires_native_process_manager() {
        let (tx, mut rx) = mpsc::channel(1);
        let ui_state = Arc::new(RwLock::new(UiState::new()));

        assert!(!request_interrupt_prompt(None, &ui_state, false));
        assert!(!ui_state.read().unwrap().interrupt_prompt_active());

        assert!(request_interrupt_prompt(Some(&tx), &ui_state, false));
        assert!(ui_state.read().unwrap().interrupt_prompt_active());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_interrupt_prompt_keys_dismiss_and_quit() {
        let ui_state = Arc::new(RwLock::new(UiState::new()));
        ui_state.write().unwrap().show_interrupt_prompt(false);
        let shutdown = tokio_shutdown::Shutdown::new();

        let dismiss = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('c'));
        assert!(handle_interrupt_prompt_key(
            &dismiss, &ui_state, &shutdown, None
        ));
        assert!(!ui_state.read().unwrap().interrupt_prompt_active());
        assert!(!shutdown.is_cancelled());

        ui_state.write().unwrap().show_interrupt_prompt(false);
        let quit = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('q'));
        assert!(handle_interrupt_prompt_key(
            &quit, &ui_state, &shutdown, None
        ));
        assert!(shutdown.is_cancelled());
    }

    #[test]
    fn test_interrupt_prompt_attached_stop_sends_command() {
        let (tx, mut rx) = mpsc::channel(1);
        let ui_state = Arc::new(RwLock::new(UiState::new()));
        ui_state.write().unwrap().show_interrupt_prompt(true);
        let shutdown = tokio_shutdown::Shutdown::new();

        // `s` in attached mode stops the manager via a command, not a shutdown.
        let stop = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('s'));
        assert!(handle_interrupt_prompt_key(
            &stop,
            &ui_state,
            &shutdown,
            Some(&tx)
        ));
        assert!(matches!(
            rx.try_recv(),
            Ok(FrontendEvent::Process(ProcessCommand::StopManager))
        ));
        assert!(!shutdown.is_cancelled());
        assert!(!ui_state.read().unwrap().interrupt_prompt_active());

        // Ctrl-C in attached mode detaches (raises the shutdown interrupt).
        ui_state.write().unwrap().show_interrupt_prompt(true);
        let mut ctrl_c = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('c'));
        ctrl_c.modifiers = KeyModifiers::CONTROL;
        assert!(handle_interrupt_prompt_key(
            &ctrl_c,
            &ui_state,
            &shutdown,
            Some(&tx)
        ));
        assert!(shutdown.is_cancelled());
    }

    #[test]
    fn process_command_is_rejected_safely_when_frontend_mailbox_is_full() {
        let (tx, mut rx) = mpsc::channel(1);
        assert!(enqueue_process_command(&tx, ProcessCommand::StopManager));
        assert!(!enqueue_process_command(
            &tx,
            ProcessCommand::Restart("alpha".to_string())
        ));
        assert!(matches!(
            rx.try_recv(),
            Ok(FrontendEvent::Process(ProcessCommand::StopManager))
        ));
    }

    #[test]
    fn process_search_updates_selection_and_can_be_cancelled() {
        let mut model = ActivityModel::new();
        model.apply_activity_event(process_start(1, "alpha"));
        model.apply_activity_event(process_start(2, "beta"));

        let mut ui_state = UiState::new();
        ui_state.selected_activity = Some(1);
        ui_state.start_process_search();
        let display = model.get_display_activities(&ui_state);
        update_process_search_selection(&model, &display, &mut ui_state);

        let e = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('e'));
        assert!(handle_process_search_key(
            &e,
            None,
            false,
            &model,
            &display,
            &mut ui_state
        ));
        assert_eq!(ui_state.selected_activity, Some(2));
        assert_eq!(
            ui_state
                .process_search
                .as_ref()
                .map(|search| search.query.as_str()),
            Some("e")
        );

        let escape = KeyEvent::new(KeyEventKind::Press, KeyCode::Esc);
        assert!(handle_process_search_key(
            &escape,
            Some(Action::Cancel),
            true,
            &model,
            &display,
            &mut ui_state
        ));
        assert_eq!(ui_state.selected_activity, Some(1));
        assert!(ui_state.process_search.is_none());

        ui_state.start_process_search();
        let mut ctrl_c = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('c'));
        ctrl_c.modifiers = KeyModifiers::CONTROL;
        assert!(handle_process_search_key(
            &ctrl_c,
            None,
            false,
            &model,
            &display,
            &mut ui_state
        ));
        assert_eq!(ui_state.selected_activity, Some(1));
        assert!(ui_state.process_search.is_none());
    }

    #[test]
    fn process_search_arrows_cycle_matching_processes() {
        let mut model = ActivityModel::new();
        model.apply_activity_event(process_start(1, "scope-api"));
        model.apply_activity_event(process_start(2, "scope-consumer"));

        let mut ui_state = UiState::new();
        ui_state.start_process_search();
        let display = model.get_display_activities(&ui_state);
        update_process_search_selection(&model, &display, &mut ui_state);
        assert_eq!(ui_state.selected_activity, Some(1));

        let down = KeyEvent::new(KeyEventKind::Press, KeyCode::Down);
        assert!(handle_process_search_key(
            &down,
            Some(Action::NextMatch),
            true,
            &model,
            &display,
            &mut ui_state
        ));
        assert_eq!(ui_state.selected_activity, Some(2));

        let up = KeyEvent::new(KeyEventKind::Press, KeyCode::Up);
        assert!(handle_process_search_key(
            &up,
            Some(Action::PreviousMatch),
            true,
            &model,
            &display,
            &mut ui_state
        ));
        assert_eq!(ui_state.selected_activity, Some(1));
    }

    #[test]
    fn enter_toggles_completed_shell_summary() {
        let mut model = ActivityModel::new();
        model.apply_activity_event(operation_start(1, "Building shell"));
        model.apply_activity_event(evaluate_start_with(
            2,
            "Evaluating Nix",
            ActivityLevel::Info,
            Some(1),
        ));
        model.apply_activity_event(build_start_with(3, "hello", Some(2)));
        model.apply_activity_event(build_complete(3, ActivityOutcome::Success));
        model.apply_activity_event(evaluate_complete(2, ActivityOutcome::Success));
        model.apply_activity_event(operation_complete(1, ActivityOutcome::Success));

        let mut ui_state = UiState::new();
        ui_state.selected_activity = Some(1);
        ui_state.inline_logs_activity = Some(1);

        activate_selected_activity(&model, &mut ui_state, false);
        assert!(ui_state.expanded_activities.contains(&1));
        assert_eq!(ui_state.inline_logs_activity, None);

        activate_selected_activity(&model, &mut ui_state, false);
        assert!(!ui_state.expanded_activities.contains(&1));
    }

    #[test]
    fn directional_open_expands_shell_and_process() {
        let mut model = ActivityModel::new();
        model.apply_activity_event(operation_start(1, "Building shell"));
        model.apply_activity_event(evaluate_start_with(
            2,
            "Evaluating Nix",
            ActivityLevel::Info,
            Some(1),
        ));
        model.apply_activity_event(evaluate_complete(2, ActivityOutcome::Success));
        model.apply_activity_event(operation_complete(1, ActivityOutcome::Success));
        model.apply_activity_event(process_start(3, "api"));

        let mut ui_state = UiState::new();
        ui_state.selected_activity = Some(1);
        expand_selected_activity(&model, &mut ui_state);
        expand_selected_activity(&model, &mut ui_state);
        assert!(ui_state.expanded_activities.contains(&1));

        ui_state.selected_activity = Some(3);
        expand_selected_activity(&model, &mut ui_state);
        assert_eq!(ui_state.inline_logs_activity, Some(3));
    }

    #[test]
    fn directional_close_collapses_shell_and_process() {
        let mut model = ActivityModel::new();
        model.apply_activity_event(operation_start(1, "Building shell"));
        model.apply_activity_event(evaluate_start_with(
            2,
            "Evaluating Nix",
            ActivityLevel::Info,
            Some(1),
        ));
        model.apply_activity_event(evaluate_complete(2, ActivityOutcome::Success));
        model.apply_activity_event(operation_complete(1, ActivityOutcome::Success));
        model.apply_activity_event(process_start(3, "api"));

        let mut ui_state = UiState::new();
        ui_state.selected_activity = Some(1);
        ui_state.expanded_activities.insert(1);
        collapse_selected_activity(&model, &mut ui_state);
        collapse_selected_activity(&model, &mut ui_state);
        assert!(!ui_state.expanded_activities.contains(&1));

        ui_state.selected_activity = Some(3);
        ui_state.inline_logs_activity = Some(3);
        collapse_selected_activity(&model, &mut ui_state);
        assert_eq!(ui_state.inline_logs_activity, None);
        assert!(ui_state.process_previews_hidden);
    }

    #[test]
    fn automatic_process_preview_can_be_hidden_and_focused_again() {
        let mut model = ActivityModel::new();
        model.apply_activity_event(process_start(1, "api"));
        model.apply_activity_event(devenv_activity::test_helpers::process_log(
            1, "ready", false,
        ));

        let mut ui_state = UiState::new();
        ui_state.selected_activity = Some(1);
        assert!(activity_shows_inline_logs(&model, &ui_state, 1, true));

        assert!(hide_selected_preview(&model, &mut ui_state, true));
        assert_eq!(ui_state.selected_activity, Some(1));
        assert!(ui_state.process_previews_hidden);
        assert!(!activity_shows_inline_logs(&model, &ui_state, 1, true));

        expand_selected_activity(&model, &mut ui_state);
        assert!(ui_state.process_previews_hidden);
        assert!(activity_shows_inline_logs(&model, &ui_state, 1, true));

        collapse_selected_activity(&model, &mut ui_state);
        assert!(!activity_shows_inline_logs(&model, &ui_state, 1, true));

        expand_selected_activity(&model, &mut ui_state);
        assert!(activity_shows_inline_logs(&model, &ui_state, 1, true));
    }

    #[test]
    fn collapsed_process_uses_row_height_before_the_next_render() {
        let mut model = ActivityModel::new();
        model.apply_activity_event(process_start(1, "api"));
        let display = model.get_display_activities(&UiState::new()).remove(0);
        let heights = std::collections::HashMap::from([(1, 11)]);

        let mut ui_state = UiState::new();
        ui_state.process_previews_hidden = true;
        assert_eq!(
            rendered_activity_height(&heights, &model, &ui_state, &display, true),
            1
        );

        ui_state.focus_inline_logs(1);
        assert_eq!(
            rendered_activity_height(&heights, &model, &ui_state, &display, true),
            11
        );
    }
}
