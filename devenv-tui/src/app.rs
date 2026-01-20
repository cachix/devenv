use crate::{
    expanded_view::ExpandedLogView,
    model::{ActivityModel, UiState, ViewMode},
    view::view,
};
use crossterm::{
    cursor, execute,
    style::{Color, ResetColor, SetForegroundColor},
    terminal,
};
use devenv_activity::{ActivityEvent, ActivityLevel};
use iocraft::prelude::*;
use std::io::{self, Write};
use std::sync::{Arc, RwLock};
use tokio::sync::{Notify, mpsc};
use tokio_shutdown::{Shutdown, Signal};
use tracing::debug;

/// Configuration for the TUI application.
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
        }
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

    /// Run the TUI application until the backend completes.
    ///
    /// The `backend_done` receiver signals when the backend has fully completed
    /// (including cleanup). The TUI will drain any remaining events and then exit.
    pub async fn run(
        self,
        backend_done: tokio::sync::oneshot::Receiver<()>,
    ) -> std::io::Result<()> {
        let config = Arc::new(self.config);
        let activity_model = Arc::new(RwLock::new(ActivityModel::with_config(config.clone())));
        let notify = Arc::new(Notify::new());
        let shutdown = self.shutdown;

        // Spawn event processor with batching for performance
        // This only writes to ActivityModel, never touches UiState
        // When backend_done fires, it drains remaining events and signals shutdown
        tokio::spawn({
            let activity_model = activity_model.clone();
            let notify = notify.clone();
            let shutdown = shutdown.clone();
            let event_batch_size = config.event_batch_size;
            let mut activity_rx = self.activity_rx;
            let mut backend_done = backend_done;
            async move {
                let mut batch = Vec::with_capacity(event_batch_size);

                loop {
                    tokio::select! {
                        event = activity_rx.recv() => {
                            let Some(event) = event else {
                                // Channel closed unexpectedly
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

                            break;
                        }
                    }
                }

                // Signal completion (idempotent - no-op if Ctrl+C already triggered).
                // Model writes above are visible to the final render because the RwLock
                // is released before this call.
                shutdown.shutdown();
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
                _ = shutdown.wait_for_shutdown() => {
                    // Shutdown triggered - but we need to wait for event processor to finish draining
                    // If this was Ctrl+C, event processor will see backend_done after backend cleanup
                    // If this was normal completion, event processor already drained and called shutdown
                    break;
                }

                _ = run_view(
                    activity_model.clone(),
                    ui_state.clone(),
                    notify.clone(),
                    shutdown.clone(),
                    config.clone(),
                    &mut pre_expand_height,
                ) => { }
            }
        }

        // Final render pass to ensure all drained events are displayed
        // This replaces the old is_done() check that triggered shutdown AFTER rendering
        //
        // On interrupt (Ctrl+C): clear the output so the user sees a clean terminal
        // On normal completion: clear previous render, then render final state
        {
            let ui = ui_state.read().unwrap();
            if let Ok(model_guard) = activity_model.read() {
                // Clear the previous inline render output
                let lines_to_clear = model_guard
                    .calculate_rendered_height(ui.selected_activity, ui.terminal_size.height);

                if lines_to_clear > 0 {
                    let mut stdout = io::stdout();
                    let _ = execute!(
                        stdout,
                        cursor::MoveToPreviousLine(lines_to_clear),
                        terminal::Clear(terminal::ClearType::FromCursorDown)
                    );
                }

                // On interrupt, don't render final state (user wants to exit quickly)
                // On normal completion, render the final state with all events processed
                if shutdown.last_signal().is_none() {
                    // Collect ALL errors for printing after TUI (including nested ones)
                    let all_errors: Vec<_> = model_guard
                        .get_all_error_messages()
                        .into_iter()
                        .map(|m| (m.text.clone(), m.details.clone()))
                        .collect();

                    let (terminal_width, _) = crossterm::terminal::size().unwrap_or((80, 24));
                    let mut element = element! {
                        View(width: terminal_width) {
                            #(vec![view(&model_guard, &ui).into()])
                        }
                    };
                    element.print();

                    // Print full error messages in red (not truncated by TUI width)
                    if !all_errors.is_empty() {
                        let mut stderr = io::stderr();
                        println!();
                        for (text, details) in all_errors {
                            let _ = execute!(stderr, SetForegroundColor(Color::AnsiValue(160)));
                            eprintln!("{}", text);
                            if let Some(details) = details {
                                eprintln!("{}", details);
                            }
                            let _ = execute!(stderr, ResetColor);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

/// Restore terminal to normal state.
/// Call this after the TUI has exited to ensure the terminal is usable.
pub fn restore_terminal() {
    let mut stdout = io::stdout();

    // Disable raw mode if it was enabled
    let _ = terminal::disable_raw_mode();

    // Show cursor (TUI may have hidden it)
    let _ = execute!(stdout, cursor::Show);

    // Ensure output is flushed
    let _ = stdout.flush();
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

    // Handle keyboard events - only UI state updates, no activity model writes
    hooks.use_terminal_events({
        let activity_model = activity_model.clone();
        let ui_state = ui_state.clone();
        let shutdown = shutdown.clone();

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
    let rendered = if let Ok(model_guard) = activity_model.read() {
        element! {
            View(width: terminal_width) {
                #(vec![view(&model_guard, &ui).into()])
            }
        }
    } else {
        element!(View(width: terminal_width))
    };

    rendered
}

async fn run_view(
    activity_model: Arc<RwLock<ActivityModel>>,
    ui_state: Arc<RwLock<UiState>>,
    notify: Arc<Notify>,
    shutdown: Arc<Shutdown>,
    config: Arc<TuiConfig>,
    pre_expand_height: &mut u16,
) -> std::io::Result<()> {
    // Copy view_mode in a block to ensure the guard is dropped before any await
    let view_mode = {
        let guard = ui_state.read().unwrap();
        guard.view_mode
    };

    match view_mode {
        ViewMode::Main => {
            if *pre_expand_height > 0 {
                let mut stdout = io::stdout();
                let _ = execute!(
                    stdout,
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
                                    MainView
                                }
                            }
                        }
                    }
                }
            };

            element.render_loop().ignore_ctrl_c().await
        }
        ViewMode::ExpandedLogs { activity_id } => {
            // Calculate height before switching to expanded view
            // Use a block to ensure guards are dropped before await
            *pre_expand_height = {
                let ui = ui_state.read().unwrap();
                let model = activity_model.read().unwrap();
                model.calculate_rendered_height(ui.selected_activity, ui.terminal_size.height)
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

            element.fullscreen().ignore_ctrl_c().await
        }
    }
}
