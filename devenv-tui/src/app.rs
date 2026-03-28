use crate::{
    expanded_view::ExpandedLogView,
    input,
    model::{ActivityModel, RenderContext, UiState, ViewMode},
    view::{ActivityHeights, SUMMARY_BAR_HEIGHT, ScrollState, view},
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use tokio::sync::{Notify, mpsc};
use tokio_shutdown::Shutdown;
use tracing::debug;

const TMUX_MAX_FPS: u64 = 12;
const MAIN_VIEW_PAGE_SCROLL_OVERLAP: i32 = 1;

/// Cooperative exit flag for TUI shutdown.
///
/// The event processor sets this when the backend is done, and TUI components
/// check it each render cycle to call `system.exit()`. This avoids cancelling
/// iocraft's render loop mid frame, which can leave the cursor at the wrong
/// position and overwrite the shell prompt.
#[derive(Clone)]
pub struct ExitFlag(Arc<AtomicBool>);

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
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            event_batch_size: 64,
            max_log_messages: 1000,
            max_log_lines_per_build: 1000,
            log_viewport_collapsed: 10,
            max_fps: if std::env::var_os("TMUX").is_some() {
                TMUX_MAX_FPS
            } else {
                30
            },
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
    fullscreen: bool,
}

fn should_present_final_snapshot(shutdown: &Shutdown) -> bool {
    shutdown.last_signal().is_none()
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
            fullscreen: false,
        }
    }

    /// Set whether the TUI should run in fullscreen mode.
    pub fn fullscreen(mut self, enabled: bool) -> Self {
        self.fullscreen = enabled;
        self
    }

    /// Set the command sender for process control commands.
    pub fn with_command_sender(mut self, tx: mpsc::Sender<ProcessCommand>) -> Self {
        self.command_tx = Some(tx);
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
    /// The `backend_done` notify signals when the backend has completed its
    /// initial phase (or fully completed). The TUI will drain any remaining
    /// events and then exit.
    /// Run the TUI and return the final render height (for cursor positioning after handoff).
    pub async fn run(self, backend_done: Arc<Notify>) -> std::io::Result<u16> {
        let config = Arc::new(self.config);
        let activity_model = Arc::new(RwLock::new(ActivityModel::with_config(config.clone())));
        let notify = Arc::new(Notify::new());
        let shutdown = self.shutdown;
        let command_tx = self.command_tx;
        let shutdown_on_backend_done = self.shutdown_on_backend_done;

        let exit_flag = ExitFlag::new();

        // Spawn event processor with batching for performance
        // This only writes to ActivityModel, never touches UiState
        // When backend_done fires, it drains remaining events and signals shutdown
        let event_processor_handle = tokio::spawn({
            let activity_model = activity_model.clone();
            let notify = notify.clone();
            let shutdown = shutdown.clone();
            let exit_flag = exit_flag.clone();
            let event_batch_size = config.event_batch_size;
            let mut activity_rx = self.activity_rx;
            async move {
                let mut batch = Vec::with_capacity(event_batch_size);
                let backend_notified = backend_done.notified();
                tokio::pin!(backend_notified);

                loop {
                    tokio::select! {
                        event = activity_rx.recv() => {
                            let Some(event) = event else {
                                // Channel closed unexpectedly
                                exit_flag.set();
                                notify.notify_waiters();
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

                        _ = &mut backend_notified => {
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
                            }

                            // Signal component to exit cooperatively. Set before
                            // notify so the triggered re-render sees the flag.
                            exit_flag.set();
                            notify.notify_waiters();

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
        if let Ok(mut ui) = ui_state.write() {
            ui.fullscreen = self.fullscreen;
        }

        // Main loop - runs until backend signals completion via exit_flag.
        // The render loop exits cooperatively: the component checks exit_flag
        // each render cycle and calls system.exit() when set, ensuring iocraft
        // always completes its current frame before returning. This prevents
        // the cursor position race that occurs when cancelling mid-frame.
        loop {
            let _ = run_view(
                activity_model.clone(),
                ui_state.clone(),
                notify.clone(),
                shutdown.clone(),
                config.clone(),
                command_tx.clone(),
                &mut pre_expand_height,
                exit_flag.clone(),
            )
            .await;

            // run_view returned either because:
            // - The backend is done (exit_flag is set)
            // - The user switched view modes (e.g. pressed 'e' for expanded)
            if exit_flag.is_set() {
                break;
            }
        }

        // Wait for event processor to finish draining events before final render.
        // This ensures all activity completion events are processed and visible.
        let _ = event_processor_handle.await;

        // Final render pass to ensure all drained events are displayed.
        // Clear previous inline render, then render final state.
        let mut final_render_height: u16 = 0;
        {
            let ui = ui_state.read().unwrap();
            if let Ok(model_guard) = activity_model.read() {
                if !should_present_final_snapshot(&shutdown) {
                    return Ok(0);
                }

                let (terminal_width, _) = crossterm::terminal::size().unwrap_or((80, 24));

                // Measure the last inline render's height so we clear the right
                // number of lines. Rendered once here at cleanup, not every frame.
                let mut measure = element! {
                    View(width: terminal_width) {
                        #(vec![view(&model_guard, &ui, RenderContext::Normal, None, false).into()])
                    }
                };
                let lines_to_clear = measure.render(Some(terminal_width as usize)).height() as u16;

                if lines_to_clear > 0 {
                    let mut stderr = io::stderr();
                    let _ = execute!(
                        stderr,
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

                    let mut element = element! {
                        View(width: terminal_width) {
                            #(vec![view(&model_guard, &ui, RenderContext::Final, None, shutdown.is_cancelled()).into()])
                        }
                    };
                    let canvas = element.render(Some(terminal_width as usize));
                    final_render_height = canvas.height() as u16;
                    let _ = canvas.write_ansi(io::stderr());

                    // Print full error messages in red (not truncated by TUI width)
                    let has_errors = !standalone_errors.is_empty()
                        || !activity_errors.is_empty()
                        || !failed_build_errors.is_empty();
                    if has_errors {
                        let mut stderr = io::stderr();
                        eprintln!();
                        final_render_height += 1; // for the empty line

                        // Print standalone error messages (no parent activity)
                        for (text, details) in standalone_errors {
                            let _ = execute!(stderr, SetForegroundColor(Color::AnsiValue(160)));
                            eprintln!("{}", text);
                            final_render_height += 1;
                            if let Some(details) = details {
                                final_render_height += details.lines().count() as u16;
                                eprintln!("{}", details);
                            }
                            let _ = execute!(stderr, ResetColor);
                        }

                        // Print error messages from Activity::Message variants
                        for (text, details) in activity_errors {
                            let _ = execute!(stderr, SetForegroundColor(Color::AnsiValue(160)));
                            eprintln!("{}", text);
                            final_render_height += 1;
                            if let Some(details) = details {
                                final_render_height += details.lines().count() as u16;
                                eprintln!("{}", details);
                            }
                            let _ = execute!(stderr, ResetColor);
                        }

                        // Print build stderr (from failed or incomplete builds)
                        for (name, lines) in failed_build_errors {
                            let _ = execute!(stderr, SetForegroundColor(Color::AnsiValue(160)));
                            eprintln!("Build error: {}", name);
                            final_render_height += 1;
                            for line in lines {
                                eprintln!("  {}", line);
                                final_render_height += 1;
                            }
                            let _ = execute!(stderr, ResetColor);
                        }
                    }
                }
            }
        }

        Ok(final_render_height)
    }
}

pub(crate) fn request_interrupt_prompt(
    command_tx: Option<&mpsc::Sender<ProcessCommand>>,
    ui_state: &Arc<RwLock<UiState>>,
) -> bool {
    if command_tx.is_none() {
        return false;
    }

    if let Ok(mut ui) = ui_state.write() {
        ui.show_interrupt_prompt();
        true
    } else {
        false
    }
}

pub(crate) fn handle_interrupt_prompt_key(
    key_event: &KeyEvent,
    ui_state: &Arc<RwLock<UiState>>,
    shutdown: &Arc<Shutdown>,
) -> bool {
    let prompt_active = ui_state
        .read()
        .map(|ui| ui.interrupt_prompt_active())
        .unwrap_or(false);
    if !prompt_active {
        return false;
    }

    match key_event.code {
        KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
            shutdown.handle_interrupt();
        }
        KeyCode::Char('q') => {
            shutdown.handle_interrupt();
        }
        KeyCode::Esc => {
            if let Ok(mut ui) = ui_state.write() {
                ui.clear_interrupt_prompt();
            }
        }
        KeyCode::Char('c') => {
            if let Ok(mut ui) = ui_state.write() {
                ui.clear_interrupt_prompt();
            }
        }
        _ => {}
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
    let _ = execute!(stderr, cursor::Show);

    // Ensure output is flushed
    let _ = stderr.flush();
}

fn activity_height(heights: &std::collections::HashMap<u64, i32>, id: u64) -> i32 {
    heights.get(&id).copied().unwrap_or(1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainViewScrollAction {
    UpLine,
    DownLine,
    UpPage,
    DownPage,
    Top,
    Bottom,
}

fn main_view_scroll_action_for_key(key_event: &KeyEvent) -> Option<MainViewScrollAction> {
    if key_event.modifiers.contains(KeyModifiers::CONTROL)
        || key_event.modifiers.contains(KeyModifiers::ALT)
        || key_event.modifiers.contains(KeyModifiers::SUPER)
    {
        return None;
    }

    match key_event.code {
        KeyCode::PageUp => Some(MainViewScrollAction::UpPage),
        KeyCode::PageDown => Some(MainViewScrollAction::DownPage),
        KeyCode::Home => Some(MainViewScrollAction::Top),
        KeyCode::End => Some(MainViewScrollAction::Bottom),
        KeyCode::Char('k') => Some(MainViewScrollAction::UpLine),
        KeyCode::Char('j') => Some(MainViewScrollAction::DownLine),
        _ => None,
    }
}

fn selected_process(
    activity_model: &Arc<RwLock<ActivityModel>>,
    ui_state: &Arc<RwLock<UiState>>,
) -> Option<(u64, String)> {
    let activity_id = ui_state.read().ok()?.selected_activity?;
    let model = activity_model.read().ok()?;
    let activity = model.get_activity(activity_id)?;
    matches!(activity.variant, crate::model::ActivityVariant::Process(_))
        .then(|| (activity.id, activity.name.clone()))
}

fn apply_main_view_scroll_action(handle: &mut ScrollViewHandle, action: MainViewScrollAction) {
    match action {
        MainViewScrollAction::UpLine => handle.scroll_by(-1),
        MainViewScrollAction::DownLine => handle.scroll_by(1),
        MainViewScrollAction::UpPage => {
            let page = (handle.viewport_height() as i32 - MAIN_VIEW_PAGE_SCROLL_OVERLAP).max(1);
            handle.scroll_by(-page);
        }
        MainViewScrollAction::DownPage => {
            let page = (handle.viewport_height() as i32 - MAIN_VIEW_PAGE_SCROLL_OVERLAP).max(1);
            handle.scroll_by(page);
        }
        MainViewScrollAction::Top => handle.scroll_to_top(),
        MainViewScrollAction::Bottom => handle.scroll_to_bottom(),
    }
}

/// Scroll the viewport so the selected activity is visible.
fn scroll_selected_into_view(
    handle: &mut ScrollViewHandle,
    heights: &std::collections::HashMap<u64, i32>,
    display_activities: &[crate::model::DisplayActivity],
    selected_id: u64,
) {
    let Some(position) = display_activities
        .iter()
        .position(|da| da.activity.id == selected_id)
    else {
        return;
    };

    let offset: i32 = display_activities[..position]
        .iter()
        .map(|da| activity_height(heights, da.activity.id))
        .sum();
    let target_height = activity_height(heights, selected_id);

    let vp = handle.viewport_height() as i32;
    let current = handle.scroll_offset();
    if offset < current {
        handle.scroll_to(offset);
    } else if offset + target_height > current + vp {
        handle.scroll_to(offset + target_height - vp);
    }
}

/// Main TUI component (inline mode)
#[component]
fn MainView(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let config = hooks.use_context::<Arc<TuiConfig>>().clone();
    let activity_model = hooks.use_context::<Arc<RwLock<ActivityModel>>>().clone();
    let ui_state = hooks.use_context::<Arc<RwLock<UiState>>>().clone();
    let notify = hooks.use_context::<Arc<Notify>>().clone();
    let (terminal_width, terminal_height) = hooks.use_terminal_size();
    let mut should_exit = hooks.use_state(|| false);
    let shutdown = hooks.use_context::<Arc<Shutdown>>().clone();
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
        let max_fps = config.max_fps;
        async move {
            crate::throttled_notify_loop(notify, redraw, max_fps).await;
        }
    });

    // Get optional command sender for process control
    let command_tx = hooks
        .use_context::<Option<mpsc::Sender<ProcessCommand>>>()
        .clone();

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
        if let Ok(mut model) = activity_model.write() {
            model.resize_vts(current_size.width, current_size.height);
        }
        if let Some(tx) = command_tx.as_ref() {
            let _ = tx.try_send(ProcessCommand::Resize {
                cols: current_size.width,
                rows: current_size.height,
            });
        }
    }
    // Handle keyboard events - only UI state updates, no activity model writes
    hooks.use_terminal_events({
        let activity_model = activity_model.clone();
        let ui_state = ui_state.clone();
        let shutdown = shutdown.clone();
        let command_tx = command_tx.clone();
        let mut scroll_handle = scroll_handle;
        let scroll_view_active = scroll_view_active;

        move |event| {
            if let TerminalEvent::Key(key_event) = event
                && key_event.kind != KeyEventKind::Release
            {
                debug!("Key event: {:?}", key_event);
                if handle_interrupt_prompt_key(&key_event, &ui_state, &shutdown) {
                    notify.notify_waiters();
                    return;
                }

                if shutdown.is_cancelled() {
                    return;
                }

                let focused_activity = ui_state.read().ok().and_then(|ui| ui.focused_activity);

                if let Some(activity_id) = focused_activity {
                    if input::is_input_toggle(&key_event) {
                        if let Ok(mut ui) = ui_state.write()
                            && ui.focused_activity == Some(activity_id)
                        {
                            ui.focused_activity = None;
                        }
                        notify.notify_waiters();
                        return;
                    }

                    if let Some(tx) = command_tx.as_ref()
                        && let Some(data) = input::encode_key_event(&key_event)
                    {
                        let _ = tx.try_send(ProcessCommand::SendInput { activity_id, data });
                        notify.notify_waiters();
                    }
                    return;
                }

                if input::is_input_toggle(&key_event) {
                    if let Some((activity_id, _)) = selected_process(&activity_model, &ui_state) {
                        if let Ok(mut ui) = ui_state.write() {
                            ui.focused_activity = Some(activity_id);
                        }
                        notify.notify_waiters();
                    }
                    return;
                }

                match key_event.code {
                    KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        if !request_interrupt_prompt(command_tx.as_ref(), &ui_state) {
                            shutdown.handle_interrupt();
                        }
                        notify.notify_waiters();
                    }
                    KeyCode::Char('r') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        if let Some(tx) = command_tx.as_ref()
                            && let Some((_, name)) = selected_process(&activity_model, &ui_state)
                        {
                            let _ = tx.try_send(ProcessCommand::Restart(name));
                            notify.notify_waiters();
                        }
                    }
                    KeyCode::Char('x') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        if let Some(tx) = command_tx.as_ref()
                            && let Some((_, name)) = selected_process(&activity_model, &ui_state)
                        {
                            let _ = tx.try_send(ProcessCommand::Stop(name));
                            notify.notify_waiters();
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
                    KeyCode::Down | KeyCode::Up => {
                        if let Ok(model) = activity_model.read() {
                            let selectable = model.get_selectable_activity_ids();
                            if let Ok(mut ui) = ui_state.write() {
                                ui.select_activity(&selectable, key_event.code == KeyCode::Down);
                                if let Some(selected_id) = ui.selected_activity
                                    && *scroll_view_active.read()
                                {
                                    let display = model.get_display_activities();
                                    let heights = activity_heights.read();
                                    scroll_selected_into_view(
                                        &mut scroll_handle.write(),
                                        &heights,
                                        &display,
                                        selected_id,
                                    );
                                }
                                notify.notify_waiters();
                            }
                        }
                    }
                    KeyCode::Esc => {
                        if let Ok(mut ui) = ui_state.write() {
                            ui.selected_activity = None;
                            ui.focused_activity = None;
                        }
                        scroll_handle.write().scroll_to_bottom();
                        notify.notify_waiters();
                    }
                    _ if *scroll_view_active.read() => {
                        if let Some(action) = main_view_scroll_action_for_key(&key_event) {
                            apply_main_view_scroll_action(&mut scroll_handle.write(), action);
                            notify.notify_waiters();
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    // Exit cooperatively when the backend signals completion via exit_flag.
    // This ensures the render loop completes its current frame before returning,
    // leaving the cursor at the correct position for the final render.
    let exit_flag = hooks.use_context::<ExitFlag>();
    if exit_flag.is_set() {
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
        let display = model_guard.get_display_activities();

        // Prune stale entries and compute total content height in a single lock
        let total_content_height: i32 = {
            let active_ids: std::collections::HashSet<u64> =
                display.iter().map(|da| da.activity.id).collect();
            let mut heights = activity_heights.write();
            heights.retain(|id, _| active_ids.contains(id));
            display
                .iter()
                .map(|da| activity_height(&heights, da.activity.id))
                .sum()
        };

        // Only enable ScrollView when content exceeds available terminal height.
        let available_height = terminal_height.saturating_sub(SUMMARY_BAR_HEIGHT) as i32;
        let scroll_handle_opt = if ui.fullscreen || total_content_height > available_height {
            Some(scroll_handle)
        } else {
            None
        };
        *scroll_view_active.write() = scroll_handle_opt.is_some();

        element! {
            ContextProvider(value: iocraft::Context::owned(activity_heights)) {
                View(width: terminal_width) {
                    #(vec![view(&model_guard, &ui, RenderContext::Normal, Some(ScrollState { handle: scroll_handle_opt, display_activities: display }), is_shutting_down).into()])
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
    shutdown: Arc<Shutdown>,
    config: Arc<TuiConfig>,
    command_tx: Option<mpsc::Sender<ProcessCommand>>,
    pre_expand_height: &mut u16,
    exit_flag: ExitFlag,
) -> std::io::Result<()> {
    // Copy view_mode in a block to ensure the guard is dropped before any await
    let view_mode = {
        let guard = ui_state.read().unwrap();
        guard.view_mode
    };

    match view_mode {
        ViewMode::Main => {
            if *pre_expand_height > 0 {
                let mut stderr = io::stderr();
                let _ = execute!(
                    stderr,
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
                                        ContextProvider(value: Context::owned(exit_flag.clone())) {
                                            MainView
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            };

            let fullscreen = ui_state.read().map(|ui| ui.fullscreen).unwrap_or(false);
            let mut render_loop = element.render_loop();
            if fullscreen {
                render_loop = render_loop.fullscreen();
            }

            render_loop.output(Output::Stderr).ignore_ctrl_c().await
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
                        #(vec![view(&model, &ui, RenderContext::Normal, None, shutdown.is_cancelled()).into()])
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
                                    ContextProvider(value: Context::owned(command_tx.clone())) {
                                        ContextProvider(value: Context::owned(exit_flag.clone())) {
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
            };

            element.fullscreen().ignore_ctrl_c().await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[test]
    fn test_request_interrupt_prompt_requires_native_process_manager() {
        let (tx, mut rx) = mpsc::channel(1);
        let ui_state = Arc::new(RwLock::new(UiState::new()));

        assert!(!request_interrupt_prompt(None, &ui_state));
        assert!(!ui_state.read().unwrap().interrupt_prompt_active());

        assert!(request_interrupt_prompt(Some(&tx), &ui_state));
        assert!(ui_state.read().unwrap().interrupt_prompt_active());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_interrupt_prompt_keys_dismiss_and_quit() {
        let ui_state = Arc::new(RwLock::new(UiState::new()));
        ui_state.write().unwrap().show_interrupt_prompt();
        let shutdown = tokio_shutdown::Shutdown::new();

        let dismiss = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('c'));
        assert!(handle_interrupt_prompt_key(&dismiss, &ui_state, &shutdown));
        assert!(!ui_state.read().unwrap().interrupt_prompt_active());
        assert!(!shutdown.is_cancelled());

        ui_state.write().unwrap().show_interrupt_prompt();
        let quit = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('q'));
        assert!(handle_interrupt_prompt_key(&quit, &ui_state, &shutdown));
        assert!(shutdown.is_cancelled());
    }

    #[test]
    fn test_user_interrupt_does_not_leave_final_snapshot() {
        let shutdown = tokio_shutdown::Shutdown::new();
        assert!(should_present_final_snapshot(&shutdown));

        shutdown.set_last_signal(tokio_shutdown::Signal::SIGINT);
        assert!(!should_present_final_snapshot(&shutdown));
    }

    #[test]
    fn test_main_view_scroll_action_for_key() {
        assert_eq!(
            main_view_scroll_action_for_key(&KeyEvent::new(KeyEventKind::Press, KeyCode::PageUp)),
            Some(MainViewScrollAction::UpPage)
        );
        assert_eq!(
            main_view_scroll_action_for_key(&KeyEvent::new(KeyEventKind::Press, KeyCode::PageDown)),
            Some(MainViewScrollAction::DownPage)
        );
        assert_eq!(
            main_view_scroll_action_for_key(&KeyEvent::new(KeyEventKind::Press, KeyCode::Home)),
            Some(MainViewScrollAction::Top)
        );
        assert_eq!(
            main_view_scroll_action_for_key(&KeyEvent::new(KeyEventKind::Press, KeyCode::End)),
            Some(MainViewScrollAction::Bottom)
        );
        assert_eq!(
            main_view_scroll_action_for_key(&KeyEvent::new(
                KeyEventKind::Press,
                KeyCode::Char('j')
            )),
            Some(MainViewScrollAction::DownLine)
        );
        assert_eq!(
            main_view_scroll_action_for_key(&KeyEvent::new(
                KeyEventKind::Press,
                KeyCode::Char('k')
            )),
            Some(MainViewScrollAction::UpLine)
        );

        let mut ctrl_j = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('j'));
        ctrl_j.modifiers = KeyModifiers::CONTROL;
        assert_eq!(main_view_scroll_action_for_key(&ctrl_j), None);
    }
}
