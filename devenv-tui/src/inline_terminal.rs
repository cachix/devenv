use crate::config::ViewportPlacement;
use crossterm::{
    cursor,
    event::{self, Event},
    execute, queue,
    style::Print,
    terminal,
};
use futures::StreamExt;
use iocraft::{Canvas, ElementExt, MockTerminalConfig, TerminalEvent};
use std::io::{self, IsTerminal, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Viewport {
    placement: ViewportPlacement,
    top: u16,
    height: u16,
    screen_width: u16,
    screen_height: u16,
    claimed: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ViewportUpdate {
    scroll_up: u16,
    clear_screen: bool,
    redraw_all: bool,
    clear_from: u16,
    clear_to: u16,
}

impl Viewport {
    fn update(
        &mut self,
        width: u16,
        height: u16,
        content_height: u16,
        resize_anchor: Option<u16>,
    ) -> ViewportUpdate {
        let previous_height = self.height;
        let resized = (width, height) != (self.screen_width, self.screen_height);

        self.height = content_height.min(height);
        self.screen_width = width;
        self.screen_height = height;

        if !self.claimed && self.height > 0 {
            self.claimed = true;
            return match self.placement {
                ViewportPlacement::Inline => {
                    let scroll_up = self.overflow();
                    self.top = self.top.saturating_sub(scroll_up);
                    ViewportUpdate {
                        scroll_up,
                        redraw_all: true,
                        ..Default::default()
                    }
                }
                ViewportPlacement::Top => {
                    let scroll_up = self.top;
                    self.top = 0;
                    ViewportUpdate {
                        scroll_up,
                        clear_screen: true,
                        redraw_all: true,
                        ..Default::default()
                    }
                }
            };
        }

        if resized && self.claimed && self.placement == ViewportPlacement::Top {
            self.top = 0;
            return ViewportUpdate {
                clear_screen: true,
                redraw_all: true,
                ..Default::default()
            };
        }

        if resized {
            self.top = resize_anchor
                .unwrap_or(self.top)
                .min(height.saturating_sub(1));
        }

        let scroll_up = self.overflow();
        self.top = self.top.saturating_sub(scroll_up);
        let available_height = height.saturating_sub(self.top);
        let (clear_from, clear_to) = if resized {
            (0, available_height)
        } else {
            (
                self.height.min(previous_height),
                previous_height.min(available_height),
            )
        };

        ViewportUpdate {
            scroll_up,
            redraw_all: resized || scroll_up > 0,
            clear_from,
            clear_to,
            ..Default::default()
        }
    }

    fn overflow(&self) -> u16 {
        self.top
            .saturating_add(self.height)
            .saturating_sub(self.screen_height)
    }
}

pub struct InlineTerminal {
    stderr: io::Stderr,
    viewport: Viewport,
    previous: Option<Canvas>,
    keyboard_enhancement: bool,
    keyboard_enhancement_active: bool,
    raw_mode_active: bool,
    cursor_hidden: bool,
    track_resize_cursor: bool,
}

impl InlineTerminal {
    pub fn new(placement: ViewportPlacement) -> io::Result<Self> {
        let screen = terminal::size()?;
        let keyboard_enhancement = terminal::supports_keyboard_enhancement().unwrap_or(false);
        let stdout_is_terminal = io::stdout().is_terminal();
        let cursor_position = if stdout_is_terminal {
            cursor::position().ok()
        } else {
            cursor_position_on_stderr().ok()
        };
        let (column, mut row) = cursor_position.unwrap_or((0, screen.1));
        let mut stderr = io::stderr();

        if column > 0 {
            execute!(stderr, Print("\r\n"))?;
            row = row.saturating_add(1).min(screen.1.saturating_sub(1));
        }
        let mut terminal = Self {
            stderr,
            viewport: Viewport {
                placement,
                top: row,
                height: 0,
                screen_width: screen.0,
                screen_height: screen.1,
                claimed: false,
            },
            previous: None,
            keyboard_enhancement,
            keyboard_enhancement_active: false,
            raw_mode_active: false,
            cursor_hidden: false,
            track_resize_cursor: stdout_is_terminal && cursor_position.is_some(),
        };
        terminal.resume()?;
        Ok(terminal)
    }

    pub async fn render_loop<E>(&mut self, mut element: E) -> io::Result<()>
    where
        E: ElementExt + Send + 'static,
    {
        let initial_size = terminal::size()?;
        let (frame_tx, mut frame_rx) = tokio::sync::mpsc::channel(1);
        let (control_tx, control_rx) = futures::channel::mpsc::unbounded();
        let (resize_request_tx, resize_request_rx) = std::sync::mpsc::channel();
        let rendered_size = Arc::new(AtomicU32::new(pack_size(initial_size)));
        let rendered_size_for_thread = rendered_size.clone();
        let resize_anchor = Arc::new(AtomicU64::new(pack_resize_anchor(initial_size, None)));
        let resize_anchor_for_thread = resize_anchor.clone();
        let track_resize_cursor = self.track_resize_cursor;
        let renderer = std::thread::Builder::new()
            .name("devenv-tui-renderer".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                let (terminal_event_tx, terminal_events) = futures::channel::mpsc::unbounded();
                let input_stopped = Arc::new(AtomicBool::new(false));
                let input_stop = input_stopped.clone();
                let input_size = rendered_size_for_thread.clone();
                let input_anchor = resize_anchor_for_thread.clone();
                let input = std::thread::Builder::new()
                    .name("devenv-tui-input".to_string())
                    .spawn(move || {
                        let input_result = (|| {
                            while !input_stop.load(Ordering::Acquire) {
                                if let Ok(size) = resize_request_rx.try_recv() {
                                    let event = record_resize(
                                        size,
                                        track_resize_cursor,
                                        &input_size,
                                        &input_anchor,
                                    );
                                    if terminal_event_tx.unbounded_send(event).is_err() {
                                        break;
                                    }
                                    continue;
                                }
                                if !event::poll(Duration::from_millis(50))? {
                                    continue;
                                }
                                let event = match event::read()? {
                                    Event::Key(event) => forward_key_event(event),
                                    Event::Mouse(event) => TerminalEvent::FullscreenMouse(
                                        iocraft::FullscreenMouseEvent::new(
                                            event.kind,
                                            event.column,
                                            event.row,
                                        ),
                                    ),
                                    Event::Resize(width, height) => record_resize(
                                        (width, height),
                                        track_resize_cursor,
                                        &input_size,
                                        &input_anchor,
                                    ),
                                    _ => continue,
                                };
                                if terminal_event_tx.unbounded_send(event).is_err() {
                                    break;
                                }
                            }
                            io::Result::Ok(())
                        })();
                        if input_result.is_err() {
                            let _ = terminal_event_tx.unbounded_send(renderer_stop_event());
                        }
                        input_result
                    })
                    .map_err(io::Error::other)?;
                let local = tokio::task::LocalSet::new();
                let render_result = runtime.block_on(local.run_until(async move {
                    let forced_size = rendered_size_for_thread.clone();
                    let forced_events = control_rx.map(move |event| {
                        if let TerminalEvent::Resize(width, height) = event {
                            forced_size.store(pack_size((width, height)), Ordering::Release);
                        }
                        event
                    });
                    let events = futures::stream::select(terminal_events, forced_events);
                    let frames =
                        element.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
                    futures::pin_mut!(frames);

                    while let Some(canvas) = frames.next().await {
                        let size = unpack_size(rendered_size_for_thread.load(Ordering::Acquire));
                        let (anchor_size, anchor) =
                            unpack_resize_anchor(resize_anchor_for_thread.load(Ordering::Acquire));
                        let anchor = (anchor_size == size).then_some(anchor).flatten();
                        if frame_tx.send((canvas, size, anchor)).await.is_err() {
                            break;
                        }
                    }

                    io::Result::Ok(())
                }));
                input_stopped.store(true, Ordering::Release);
                let input_result = input
                    .join()
                    .map_err(|_| io::Error::other("TUI input thread panicked"))?;
                render_result.and(input_result)
            })
            .map_err(io::Error::other)?;

        let mut draw_result = Ok(());
        let mut requested_size = None;
        while let Some((canvas, frame_size, resize_anchor)) = frame_rx.recv().await {
            let physical_size = match terminal::size() {
                Ok(size) => size,
                Err(error) => {
                    draw_result = Err(error);
                    break;
                }
            };
            if frame_size != physical_size || canvas.width() != usize::from(physical_size.0) {
                if requested_size != Some(physical_size) {
                    if resize_request_tx.send(physical_size).is_err() {
                        draw_result = Err(io::Error::other("TUI renderer stopped during resize"));
                        break;
                    }
                    requested_size = Some(physical_size);
                }
                continue;
            }
            requested_size = None;
            if let Err(error) = self.draw_with_size(&canvas, physical_size, resize_anchor) {
                draw_result = Err(error);
                break;
            }
        }
        frame_rx.close();
        if draw_result.is_err() {
            let _ = control_tx.unbounded_send(renderer_stop_event());
        }

        let renderer_result = tokio::task::spawn_blocking(move || renderer.join())
            .await
            .map_err(io::Error::other)?
            .map_err(|_| io::Error::other("TUI renderer thread panicked"))?;
        draw_result.and(renderer_result)
    }

    pub fn invalidate(&mut self) {
        self.previous = None;
    }

    pub fn suspend(&mut self) -> io::Result<()> {
        if !self.keyboard_enhancement_active && !self.raw_mode_active && !self.cursor_hidden {
            return Ok(());
        }

        let mut first_error = None;
        if self.raw_mode_active {
            match terminal::disable_raw_mode() {
                Ok(()) => self.raw_mode_active = false,
                Err(error) => first_error = Some(error),
            }
        }
        if self.keyboard_enhancement_active {
            match execute!(self.stderr, event::PopKeyboardEnhancementFlags) {
                Ok(()) => self.keyboard_enhancement_active = false,
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }
        if self.cursor_hidden {
            match execute!(self.stderr, terminal::EnableLineWrap, cursor::Show) {
                Ok(()) => self.cursor_hidden = false,
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }
        if let Err(error) = self.stderr.flush()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        first_error.map_or(Ok(()), Err)
    }

    pub fn resume(&mut self) -> io::Result<()> {
        if self.raw_mode_active
            && self.cursor_hidden
            && (!self.keyboard_enhancement || self.keyboard_enhancement_active)
        {
            return Ok(());
        }
        self.suspend()?;

        if self.keyboard_enhancement {
            self.keyboard_enhancement_active = true;
            if let Err(error) = execute!(
                self.stderr,
                event::PushKeyboardEnhancementFlags(
                    event::KeyboardEnhancementFlags::REPORT_EVENT_TYPES,
                )
            ) {
                let _ = self.suspend();
                return Err(error);
            }
        }
        if let Err(error) = terminal::enable_raw_mode() {
            let _ = self.suspend();
            return Err(error);
        }
        self.raw_mode_active = true;
        self.cursor_hidden = true;
        if let Err(error) = execute!(self.stderr, cursor::Hide) {
            let _ = self.suspend();
            return Err(error);
        }
        Ok(())
    }

    pub fn commit(&mut self, canvas: &Canvas) -> io::Result<()> {
        self.resume()?;
        let (width, height) = terminal::size()?;
        let content_height = u16::try_from(canvas_content_height(canvas)).unwrap_or(u16::MAX);
        let cursor_bottom = ((width, height)
            != (self.viewport.screen_width, self.viewport.screen_height)
            && self.track_resize_cursor)
            .then(|| cursor::position().ok())
            .flatten()
            .map(|(_, row)| row);
        let resize_anchor = self.resize_top(width, height, cursor_bottom);
        let update = self
            .viewport
            .update(width, height, content_height, resize_anchor);
        let rows = ansi_rows(canvas)?;

        queue!(
            self.stderr,
            terminal::BeginSynchronizedUpdate,
            terminal::DisableLineWrap
        )?;
        let write_result = (|| {
            if update.scroll_up > 0 {
                queue!(self.stderr, terminal::ScrollUp(update.scroll_up))?;
            }
            match self.viewport.placement {
                ViewportPlacement::Inline => {
                    for row in update.clear_from..update.clear_to {
                        queue!(
                            self.stderr,
                            cursor::MoveTo(0, self.viewport.top.saturating_add(row)),
                            terminal::Clear(terminal::ClearType::CurrentLine)
                        )?;
                    }
                }
                ViewportPlacement::Top => {
                    queue!(
                        self.stderr,
                        cursor::MoveTo(0, 0),
                        terminal::Clear(terminal::ClearType::All)
                    )?;
                }
            }
            if self.viewport.height == 0 {
                return Ok(());
            }
            for (row_index, row) in rows
                .iter()
                .take(usize::from(self.viewport.height))
                .enumerate()
            {
                let screen_row = self
                    .viewport
                    .top
                    .saturating_add(u16::try_from(row_index).unwrap_or(u16::MAX));
                queue!(
                    self.stderr,
                    cursor::MoveTo(0, screen_row),
                    terminal::Clear(terminal::ClearType::CurrentLine),
                    Print("\x1b[0m")
                )?;
                self.stderr.write_all(row)?;
                queue!(self.stderr, Print("\r\n"))?;
            }
            Ok(())
        })();
        let end_result = queue!(
            self.stderr,
            Print("\x1b[0m"),
            terminal::EnableLineWrap,
            terminal::EndSynchronizedUpdate
        );
        let flush_result = self.stderr.flush();
        write_result.and(end_result).and(flush_result)?;

        self.previous = None;
        self.viewport.height = 0;
        Ok(())
    }

    fn draw_with_size(
        &mut self,
        canvas: &Canvas,
        (width, height): (u16, u16),
        cursor_bottom: Option<u16>,
    ) -> io::Result<()> {
        let content_height = u16::try_from(canvas_content_height(canvas)).unwrap_or(u16::MAX);
        let resize_anchor = self.resize_top(width, height, cursor_bottom);
        let update = self
            .viewport
            .update(width, height, content_height, resize_anchor);
        let rows = ansi_rows(canvas)?;

        queue!(
            self.stderr,
            terminal::BeginSynchronizedUpdate,
            terminal::DisableLineWrap
        )?;

        let draw_result = (|| {
            if update.scroll_up > 0 {
                queue!(self.stderr, terminal::ScrollUp(update.scroll_up))?;
            }
            if update.clear_screen {
                queue!(
                    self.stderr,
                    cursor::MoveTo(0, 0),
                    terminal::Clear(terminal::ClearType::All)
                )?;
            } else {
                for row in update.clear_from..update.clear_to {
                    queue!(
                        self.stderr,
                        cursor::MoveTo(0, self.viewport.top.saturating_add(row)),
                        terminal::Clear(terminal::ClearType::CurrentLine)
                    )?;
                }
            }

            for row in 0..self.viewport.height {
                let row_index = usize::from(row);
                let changed = update.redraw_all
                    || self
                        .previous
                        .as_ref()
                        .is_none_or(|previous| !rows_equal(previous, canvas, row_index));
                if !changed {
                    continue;
                }

                queue!(
                    self.stderr,
                    cursor::MoveTo(0, self.viewport.top.saturating_add(row)),
                    terminal::Clear(terminal::ClearType::CurrentLine),
                    Print("\x1b[0m")
                )?;
                self.stderr
                    .write_all(rows.get(row_index).map_or(&[], Vec::as_slice))?;
                if self.viewport.top.saturating_add(row).saturating_add(1) < height {
                    queue!(self.stderr, Print("\r\n"))?;
                }
            }

            if self.viewport.height > 0 {
                queue!(
                    self.stderr,
                    cursor::MoveTo(
                        width.saturating_sub(1),
                        self.viewport
                            .top
                            .saturating_add(self.viewport.height.saturating_sub(1))
                    )
                )?;
            }

            Ok(())
        })();

        let end_result = queue!(
            self.stderr,
            terminal::EnableLineWrap,
            terminal::EndSynchronizedUpdate
        );
        let flush_result = self.stderr.flush();
        draw_result.and(end_result).and(flush_result)?;
        self.previous = Some(canvas.clone());
        Ok(())
    }

    fn resize_top(&self, width: u16, height: u16, cursor_bottom: Option<u16>) -> Option<u16> {
        if (width, height) == (self.viewport.screen_width, self.viewport.screen_height)
            || self.viewport.placement == ViewportPlacement::Top
        {
            return cursor_bottom;
        }
        let reflowed_height = self
            .previous
            .as_ref()
            .map(|canvas| reflowed_canvas_height(canvas, self.viewport.height, width))
            .unwrap_or(self.viewport.height)
            .min(height);
        Some(cursor_bottom.map_or_else(
            || {
                self.viewport
                    .top
                    .min(height.saturating_sub(reflowed_height))
            },
            |bottom| bottom.saturating_add(1).saturating_sub(reflowed_height),
        ))
    }
}

#[cfg(unix)]
fn cursor_position_on_stderr() -> io::Result<(u16, u16)> {
    if !io::stderr().is_terminal() || !io::stdin().is_terminal() {
        return Err(io::Error::other("terminal input or output is unavailable"));
    }
    let was_raw = terminal::is_raw_mode_enabled()?;
    if !was_raw {
        terminal::enable_raw_mode()?;
    }
    let result = (|| {
        let mut stderr = io::stderr();
        stderr.write_all(b"\x1b[6n")?;
        stderr.flush()?;
        read_cursor_position(Duration::from_secs(2))
    })();
    if !was_raw {
        terminal::disable_raw_mode()?;
    }
    result
}

#[cfg(unix)]
fn read_cursor_position(timeout: Duration) -> io::Result<(u16, u16)> {
    let deadline = Instant::now() + timeout;
    let mut input = Vec::with_capacity(32);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "terminal cursor position query timed out",
            ));
        }
        let mut descriptor = libc::pollfd {
            fd: libc::STDIN_FILENO,
            events: libc::POLLIN,
            revents: 0,
        };
        let timeout_ms = remaining.as_millis().min(i32::MAX as u128) as i32;
        let ready = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
        if ready == 0 {
            continue;
        }
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        let mut byte = 0u8;
        let read =
            unsafe { libc::read(libc::STDIN_FILENO, std::ptr::from_mut(&mut byte).cast(), 1) };
        if read < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "terminal closed during cursor position query",
            ));
        }
        input.push(byte);
        if let Some(position) = parse_cursor_position(&input) {
            return Ok(position);
        }
        if input.len() > 64 {
            input.drain(..input.len() - 64);
        }
    }
}

#[cfg(unix)]
fn parse_cursor_position(input: &[u8]) -> Option<(u16, u16)> {
    input.windows(2).enumerate().find_map(|(start, prefix)| {
        if prefix != b"\x1b[" {
            return None;
        }
        let response = &input[start + 2..];
        let end = response.iter().position(|byte| *byte == b'R')?;
        let response = std::str::from_utf8(&response[..end]).ok()?;
        let (row, column) = response.split_once(';')?;
        let row = row.parse::<u16>().ok()?.checked_sub(1)?;
        let column = column.parse::<u16>().ok()?.checked_sub(1)?;
        Some((column, row))
    })
}

#[cfg(not(unix))]
fn cursor_position_on_stderr() -> io::Result<(u16, u16)> {
    Err(io::Error::other(
        "cursor queries through stderr are unsupported on this platform",
    ))
}

fn pack_size((width, height): (u16, u16)) -> u32 {
    u32::from(width) << 16 | u32::from(height)
}

fn unpack_size(size: u32) -> (u16, u16) {
    ((size >> 16) as u16, size as u16)
}

fn pack_resize_anchor((width, height): (u16, u16), row: Option<u16>) -> u64 {
    u64::from(width) << 32 | u64::from(height) << 16 | u64::from(row.unwrap_or(u16::MAX))
}

fn unpack_resize_anchor(value: u64) -> ((u16, u16), Option<u16>) {
    let width = (value >> 32) as u16;
    let height = (value >> 16) as u16;
    let row = value as u16;
    ((width, height), (row != u16::MAX).then_some(row))
}

fn record_resize(
    size: (u16, u16),
    track_cursor: bool,
    rendered_size: &AtomicU32,
    resize_anchor: &AtomicU64,
) -> TerminalEvent {
    rendered_size.store(pack_size(size), Ordering::Release);
    let row = track_cursor
        .then(|| cursor::position().ok())
        .flatten()
        .map(|(_, row)| row);
    resize_anchor.store(pack_resize_anchor(size, row), Ordering::Release);
    TerminalEvent::Resize(size.0, size.1)
}

impl Drop for InlineTerminal {
    fn drop(&mut self) {
        let _ = self.suspend();
    }
}

fn forward_key_event(event: event::KeyEvent) -> TerminalEvent {
    let kind = if event.kind == event::KeyEventKind::Press
        && event.code == event::KeyCode::Char('c')
        && event.modifiers.contains(event::KeyModifiers::CONTROL)
    {
        event::KeyEventKind::Repeat
    } else {
        event.kind
    };
    let mut key = iocraft::KeyEvent::new(kind, event.code);
    key.modifiers = event.modifiers;
    TerminalEvent::Key(key)
}

fn renderer_stop_event() -> TerminalEvent {
    let mut key = iocraft::KeyEvent::new(event::KeyEventKind::Press, event::KeyCode::Char('c'));
    key.modifiers = event::KeyModifiers::CONTROL;
    TerminalEvent::Key(key)
}

fn ansi_rows(canvas: &Canvas) -> io::Result<Vec<Vec<u8>>> {
    let mut output = Vec::new();
    canvas.write_ansi(&mut output)?;
    Ok(output
        .split(|byte| *byte == b'\n')
        .take(canvas.height())
        .map(|row| row.strip_suffix(b"\r").unwrap_or(row).to_vec())
        .collect())
}

fn rows_equal(previous: &Canvas, current: &Canvas, row: usize) -> bool {
    previous.width() == current.width()
        && (0..current.width())
            .all(|column| previous.cell(column, row) == current.cell(column, row))
}

fn canvas_content_height(canvas: &Canvas) -> usize {
    (0..canvas.height())
        .rfind(|row| {
            (0..canvas.width()).any(|column| {
                canvas
                    .cell(column, *row)
                    .and_then(|cell| cell.text())
                    .is_some()
            })
        })
        .map_or(0, |row| row + 1)
}

fn reflowed_canvas_height(canvas: &Canvas, height: u16, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    (0..usize::from(height))
        .map(|row| {
            let row_width = if row + 1 == usize::from(height) {
                canvas.width()
            } else {
                (0..canvas.width())
                    .rfind(|column| {
                        canvas
                            .cell(*column, row)
                            .and_then(|cell| cell.text())
                            .is_some()
                    })
                    .map_or(1, |column| column + 1)
            };
            u16::try_from(row_width.div_ceil(width)).unwrap_or(u16::MAX)
        })
        .fold(0, u16::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iocraft::prelude::*;

    #[cfg(unix)]
    #[test]
    fn parses_cursor_position_responses_amid_unrelated_input() {
        assert_eq!(parse_cursor_position(b"x\x1b[6;1R"), Some((0, 5)));
        assert_eq!(parse_cursor_position(b"\x1b[24;80R"), Some((79, 23)));
        assert_eq!(parse_cursor_position(b"\x1b[0;1R"), None);
        assert_eq!(parse_cursor_position(b"\x1b[6;R"), None);
    }

    #[test]
    fn inline_viewport_starts_at_the_cursor_when_content_fits() {
        let mut viewport = Viewport {
            placement: ViewportPlacement::Inline,
            top: 9,
            height: 0,
            screen_width: 80,
            screen_height: 24,
            claimed: false,
        };

        let update = viewport.update(80, 24, 8, None);

        assert_eq!(update.scroll_up, 0);
        assert!(!update.clear_screen);
        assert!(update.redraw_all);
        assert_eq!(viewport.top, 9);
        assert_eq!(viewport.height, 8);
        assert!(viewport.claimed);
    }

    #[test]
    fn inline_viewport_scrolls_only_content_past_the_bottom() {
        let mut viewport = Viewport {
            placement: ViewportPlacement::Inline,
            top: 20,
            height: 0,
            screen_width: 80,
            screen_height: 24,
            claimed: false,
        };

        let update = viewport.update(80, 24, 8, None);

        assert_eq!(update.scroll_up, 4);
        assert_eq!(viewport.top, 16);
        assert_eq!(viewport.height, 8);
    }

    #[test]
    fn inline_viewport_growth_scrolls_by_the_new_overflow() {
        let mut viewport = Viewport {
            placement: ViewportPlacement::Inline,
            top: 10,
            height: 8,
            screen_width: 80,
            screen_height: 24,
            claimed: true,
        };

        let update = viewport.update(80, 24, 16, None);

        assert_eq!(update.scroll_up, 2);
        assert!(update.redraw_all);
        assert!(!update.clear_screen);
        assert_eq!(viewport.top, 8);
        assert_eq!(viewport.height, 16);
    }

    #[test]
    fn inline_viewport_shrink_only_clears_released_rows() {
        let mut viewport = Viewport {
            placement: ViewportPlacement::Inline,
            top: 10,
            height: 8,
            screen_width: 80,
            screen_height: 24,
            claimed: true,
        };

        let update = viewport.update(80, 24, 3, None);

        assert_eq!(update.scroll_up, 0);
        assert!(!update.clear_screen);
        assert_eq!(viewport.top, 10);
        assert_eq!(update.clear_from, 3);
        assert_eq!(update.clear_to, 8);
    }

    #[test]
    fn inline_viewport_resize_keeps_the_content_visible() {
        let mut viewport = Viewport {
            placement: ViewportPlacement::Inline,
            top: 20,
            height: 4,
            screen_width: 80,
            screen_height: 24,
            claimed: true,
        };

        let update = viewport.update(80, 10, 4, None);

        assert_eq!(update.scroll_up, 3);
        assert!(update.redraw_all);
        assert!(!update.clear_screen);
        assert_eq!(viewport.top, 6);
        assert_eq!(viewport.height, 4);
        assert_eq!(update.clear_from, 0);
        assert_eq!(update.clear_to, 4);
    }

    #[test]
    fn inline_viewport_resize_uses_the_reflowed_top() {
        let mut viewport = Viewport {
            placement: ViewportPlacement::Inline,
            top: 20,
            height: 4,
            screen_width: 80,
            screen_height: 24,
            claimed: true,
        };

        let update = viewport.update(80, 10, 4, Some(4));

        assert_eq!(update.scroll_up, 0);
        assert!(update.redraw_all);
        assert_eq!(viewport.top, 4);
        assert_eq!(update.clear_from, 0);
        assert_eq!(update.clear_to, 6);
    }

    #[test]
    fn top_viewport_claims_and_replaces_the_visible_screen() {
        let mut viewport = Viewport {
            placement: ViewportPlacement::Top,
            top: 20,
            height: 0,
            screen_width: 100,
            screen_height: 30,
            claimed: false,
        };

        let claim = viewport.update(100, 30, 8, None);

        assert_eq!(claim.scroll_up, 20);
        assert!(claim.clear_screen);
        assert_eq!(viewport.top, 0);
        assert_eq!(viewport.height, 8);

        let resize = viewport.update(80, 24, 8, Some(12));

        assert_eq!(resize.scroll_up, 0);
        assert!(resize.clear_screen);
        assert!(resize.redraw_all);
        assert_eq!(viewport.top, 0);
    }

    #[test]
    fn canvas_rows_are_compared_independently() {
        let first = element!(View(width: 10) { Text(content: "one\ntwo") }).render(Some(10));
        let second = element!(View(width: 10) { Text(content: "one\nthree") }).render(Some(10));

        assert!(rows_equal(&first, &second, 0));
        assert!(!rows_equal(&first, &second, 1));

        let rows = ansi_rows(&second).unwrap();
        assert_eq!(rows.len(), second.height());
    }

    #[test]
    fn background_only_rows_do_not_expand_the_inline_viewport() {
        let canvas = element!(View(height: 12, background_color: Color::Blue) {
            Text(content: "content")
        })
        .render(Some(20));

        assert_eq!(canvas.height(), 12);
        assert_eq!(canvas_content_height(&canvas), 1);
    }

    #[test]
    fn reflowed_height_counts_each_hard_row() {
        let canvas = element!(View(width: 12) { Text(content: "abcdefgh\nxy") }).render(Some(12));

        assert_eq!(canvas_content_height(&canvas), 2);
        assert_eq!(reflowed_canvas_height(&canvas, 2, 6), 4);
    }

    #[test]
    fn ctrl_c_reaches_component_event_handlers() {
        let event = event::KeyEvent::new(event::KeyCode::Char('c'), event::KeyModifiers::CONTROL);

        let TerminalEvent::Key(event) = forward_key_event(event) else {
            panic!("expected key event");
        };

        assert_eq!(event.kind, event::KeyEventKind::Repeat);
        assert_eq!(event.code, event::KeyCode::Char('c'));
        assert_eq!(event.modifiers, event::KeyModifiers::CONTROL);
    }
}
