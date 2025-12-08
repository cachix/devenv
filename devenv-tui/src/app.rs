use crate::{
    expanded_view::ExpandedLogView,
    model::{Model, ViewMode},
    view::view,
};
use crossterm::{cursor, execute, terminal};
use devenv_activity::ActivityEvent;
use iocraft::prelude::*;
use std::io::{self, Write};
use std::sync::{Arc, RwLock};
use tokio::sync::{mpsc, Notify};
use tokio_shutdown::Shutdown;
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
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            event_batch_size: 64,
            max_log_messages: 1000,
            max_log_lines_per_build: 1000,
            log_viewport_collapsed: 10,
            max_fps: 30,
        }
    }
}

/// Builder for creating and running the TUI application.
pub struct TuiApp {
    config: TuiConfig,
    activity_rx: mpsc::Receiver<ActivityEvent>,
    shutdown: Arc<Shutdown>,
}

impl TuiApp {
    /// Create a new TUI application with required dependencies.
    pub fn new(activity_rx: mpsc::Receiver<ActivityEvent>, shutdown: Arc<Shutdown>) -> Self {
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

    /// Run the TUI application.
    pub async fn run(self) -> std::io::Result<()> {
        let config = Arc::new(self.config);
        let model = Arc::new(RwLock::new(Model::with_config(config.clone())));
        let notify = Arc::new(Notify::new());
        let shutdown = self.shutdown;

        // Spawn event processor with batching for performance
        tokio::spawn({
            let model = model.clone();
            let notify = notify.clone();
            let event_batch_size = config.event_batch_size;
            let mut activity_rx = self.activity_rx;
            async move {
                let mut batch = Vec::with_capacity(event_batch_size);

                while let Some(event) = activity_rx.recv().await {
                    batch.push(event);
                    while let Ok(event) = activity_rx.try_recv() {
                        batch.push(event);
                        if batch.len() >= event_batch_size {
                            break;
                        }
                    }

                    if let Ok(mut m) = model.write() {
                        for event in batch.drain(..) {
                            m.apply_activity_event(event);
                        }
                    }

                    notify.notify_waiters();
                }
            }
        });

        // Track height to clear when returning from expanded view
        let mut pre_expand_height: u16 = 0;

        loop {
            tokio::select! {
                _ = shutdown.wait_for_shutdown() => {
                    break;
                }

                _ = run_view(
                    model.clone(),
                    notify.clone(),
                    shutdown.clone(),
                    config.clone(),
                    &mut pre_expand_height,
                ) => { }
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
    let model = hooks.use_context::<Arc<RwLock<Model>>>();
    let notify = hooks.use_context::<Arc<Notify>>();
    let (terminal_width, terminal_height) = hooks.use_terminal_size();
    let mut should_exit = hooks.use_state(|| false);
    let shutdown = hooks.use_context::<Arc<Shutdown>>();
    let mut system = hooks.use_context_mut::<SystemContext>();

    // Redraw when notified of model changes (throttled)
    let redraw = hooks.use_state(|| 0u64);
    hooks.use_future({
        let notify = notify.clone();
        let max_fps = config.max_fps;
        async move {
            crate::throttled_notify_loop(notify, redraw, max_fps).await;
        }
    });

    // Track terminal size changes
    let mut prev_size = hooks.use_state(crate::TerminalSize::default);
    let current_size = crate::TerminalSize {
        width: terminal_width,
        height: terminal_height,
    };
    if current_size != prev_size.get() {
        prev_size.set(current_size);
        if let Ok(mut m) = model.write() {
            m.set_terminal_size(current_size.width, current_size.height);
        }
    }

    // Handle keyboard events
    hooks.use_terminal_events({
        let model = model.clone();
        let shutdown = shutdown.clone();

        move |event| {
            if let TerminalEvent::Key(key_event) = event
                && key_event.kind != KeyEventKind::Release
            {
                debug!("Key event: {:?}", key_event);
                match key_event.code {
                    KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        shutdown.shutdown();
                    }
                    KeyCode::Char('e') => {
                        if let Ok(mut m) = model.write() {
                            if let Some(activity_id) = m.ui.selected_activity {
                                m.ui.view_mode = ViewMode::ExpandedLogs {
                                    activity_id,
                                    scroll_offset: 0,
                                };
                                should_exit.set(true);
                            }
                        }
                    }
                    KeyCode::Down => {
                        if let Ok(mut m) = model.write() {
                            m.select_next_activity();
                        }
                    }
                    KeyCode::Up => {
                        if let Ok(mut m) = model.write() {
                            m.select_previous_activity();
                        }
                    }
                    KeyCode::Esc => {
                        if let Ok(mut m) = model.write() {
                            m.ui.selected_activity = None;
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    if should_exit.get() || shutdown.is_cancelled() {
        system.exit();
    }

    // Render the view
    if let Ok(model_guard) = model.read() {
        element! {
            View(width: terminal_width) {
                #(vec![view(&model_guard).into()])
            }
        }
    } else {
        element!(View(width: terminal_width))
    }
}

async fn run_view(
    model: Arc<RwLock<Model>>,
    notify: Arc<Notify>,
    shutdown: Arc<Shutdown>,
    config: Arc<TuiConfig>,
    pre_expand_height: &mut u16,
) -> std::io::Result<()> {
    let view_mode = { model.read().unwrap().ui.view_mode.clone() };

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
                            ContextProvider(value: Context::owned(model.clone())) {
                                MainView
                            }
                        }
                    }
                }
            };

            element.render_loop().ignore_ctrl_c().await
        }
        ViewMode::ExpandedLogs { .. } => {
            *pre_expand_height = model.read().unwrap().calculate_rendered_height();

            let mut element = element! {
                ContextProvider(value: Context::owned(config.clone())) {
                    ContextProvider(value: Context::owned(shutdown.clone())) {
                        ContextProvider(value: Context::owned(notify.clone())) {
                            ContextProvider(value: Context::owned(model.clone())) {
                                ExpandedLogView
                            }
                        }
                    }
                }
            };

            element.fullscreen().ignore_ctrl_c().await
        }
    }
}
