use super::span_timings::SpanTimings;

use console::style;
use std::{
    fmt,
    sync::atomic::{AtomicBool, Ordering},
};
use tracing::{Event, Subscriber, field::Field, span};
use tracing_subscriber::{
    field::Visit,
    fmt::{FmtContext, FormatEvent, FormatFields, format::Writer},
    layer,
    registry::LookupSpan,
};

/// Capture additional context during a span.
#[derive(Debug)]
struct SpanContext {
    /// The user message associated with the span.
    msg: String,
    /// Whether the span has an error event.
    has_error: bool,
    /// Span timings
    timings: SpanTimings,
}

/// A helper to create child events from a span.
/// Borrowed from [tracing_subscriber].
macro_rules! with_event_from_span {
    ($id:ident, $span:ident, $($field:literal = $value:expr),*, |$event:ident| $code:block) => {
        let meta = $span.metadata();
        let cs = meta.callsite();
        let fs = tracing::field::FieldSet::new(&[$($field),*], cs);
        #[allow(unused)]
        let mut iter = fs.iter();
        let v = [$(
            (&iter.next().unwrap(), ::core::option::Option::Some(&$value as &dyn tracing::field::Value)),
        )*];
        let vs = fs.value_set(&v);
        let $event = Event::new_child_of($id, meta, &vs);
        $code
    };
}

/// Span lifecycle layer.
///
/// Emits synthetic events when activity spans (those carrying a
/// `devenv.ui.message` attribute) open and close. Each event sets
/// `devenv.span_end = false` (Start) or `true` (End); `--trace-to` exporters
/// (JSON / pretty / OTLP) use these to surface activity boundaries with their
/// user-friendly message and total duration. The default stderr CLI is
/// rendered by the activity channel consumer
/// ([`crate::console::ConsoleOutput`]); [`DevenvFormat`] filters synthetic
/// events out by detecting `devenv.span_end`.
///
/// Field convention (each name has a single type, no overloading):
/// - `devenv.ui.message` (String, span attribute): the activity's display name.
///   Set on the span by `Activity::start!`. Read here to populate
///   [`SpanContext::msg`].
/// - `devenv.span_end` (bool, event field): emitted only on synthetic events
///   from this layer (false = Start, true = End). Presence signals
///   "do not render to the CLI".
///
/// User-facing one-shot messages from non-activity code use
/// `devenv_activity::message(level, text)` and flow through the activity
/// channel — they are not represented as a tracing field here.
#[derive(Default)]
pub struct DevenvLayer {
    /// Whether the span has an error event.
    has_error: AtomicBool,
}

impl DevenvLayer {
    pub fn new() -> Self {
        Self {
            has_error: AtomicBool::new(false),
        }
    }
}

impl<S> layer::Layer<S> for DevenvLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx.span(id).expect("Span not found in context");

        #[derive(Default)]
        struct UserMessageVisitor {
            user_message: Option<String>,
        }

        impl Visit for UserMessageVisitor {
            fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}

            fn record_str(&mut self, field: &Field, value: &str) {
                if field.name() == "devenv.ui.message" {
                    self.user_message = Some(value.to_string());
                }
            }
        }

        let mut visitor = UserMessageVisitor::default();
        attrs.record(&mut visitor);

        let mut ext = span.extensions_mut();

        // Activity spans carry `devenv.ui.message`; plain `#[instrument]` spans
        // don't. Fall back to the span name so every span emits start/end events
        // — `json_subscriber` doesn't synthesize span lifecycle on its own.
        let msg = visitor
            .user_message
            .unwrap_or_else(|| span.metadata().name().to_string());

        ext.insert(SpanContext {
            msg: msg.clone(),
            has_error: false,
            timings: SpanTimings::new(),
        });

        with_event_from_span!(
            id,
            span,
            "message" = msg,
            "devenv.span_end" = false,
            "devenv.span_has_error" = false,
            |event| {
                drop(ext);
                drop(span);
                ctx.event(&event);
            }
        );
    }

    fn on_enter(&self, id: &span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx.span(id).expect("Span not found in context");
        let mut extensions = span.extensions_mut();
        if let Some(span_ctx) = extensions.get_mut::<SpanContext>() {
            span_ctx.timings.enter();
        }
    }

    fn on_exit(&self, id: &span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx.span(id).expect("Span not found in context");
        let mut extensions = span.extensions_mut();
        if let Some(span_ctx) = extensions.get_mut::<SpanContext>() {
            span_ctx.timings.exit();
        }
    }

    fn on_close(&self, id: span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx.span(&id).expect("Span not found in context");
        let mut extensions = span.extensions_mut();

        if let Some(span_ctx) = extensions.get_mut::<SpanContext>() {
            span_ctx.timings.enter();

            let has_error = self.has_error.load(Ordering::SeqCst);
            if has_error {
                span_ctx.has_error = true;
            }

            let msg = span_ctx.msg.clone();
            let time_total = format!("{}", span_ctx.timings.total_duration());

            // Emit the final message event
            with_event_from_span!(
                id,
                span,
                "message" = msg,
                "devenv.span_end" = true,
                "devenv.span_has_error" = has_error,
                "devenv.time_total" = time_total,
                |event| {
                    drop(extensions);
                    drop(span);
                    ctx.event(&event);
                }
            );
        }
    }

    // Track if any error events are emitted.
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: layer::Context<'_, S>) {
        if event.metadata().level() == &tracing::Level::ERROR {
            self.has_error.store(true, Ordering::SeqCst);
        }
    }
}

/// Renders plain `tracing` events to stderr.
///
/// Activity start/complete output is emitted via the activity channel and
/// rendered by [`crate::console::ConsoleOutput`] (or the TUI). Synthetic span
/// events emitted by [`DevenvLayer`] are skipped here — they exist for the
/// `--trace-to` exporters only.
#[derive(Default)]
pub struct DevenvFormat {
    pub verbose: bool,
}

impl<S, F> FormatEvent<S, F> for DevenvFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    F: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, F>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        #[derive(Default)]
        struct EventVisitor {
            message: Option<String>,
            is_span_synthetic: bool,
        }

        impl Visit for EventVisitor {
            fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
                if field.name() == "message" {
                    self.message = Some(format!("{value:?}"));
                }
            }

            fn record_str(&mut self, field: &Field, value: &str) {
                if field.name() == "message" {
                    self.message = Some(value.to_string());
                }
            }

            fn record_bool(&mut self, field: &Field, _value: bool) {
                if field.name() == "devenv.span_end" {
                    self.is_span_synthetic = true;
                }
            }
        }

        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);

        // Synthetic span events are for trace exporters only — the channel
        // consumer renders activities to the terminal.
        if visitor.is_span_synthetic {
            return Ok(());
        }

        let Some(msg) = visitor.message else {
            return Ok(());
        };
        let level = *event.metadata().level();

        // Only show errors/warnings by default; verbose mode shows everything.
        // User-facing one-shot messages go through `devenv_activity::message`,
        // not via this formatter.
        if !self.verbose && !matches!(level, tracing::Level::ERROR | tracing::Level::WARN) {
            return Ok(());
        }

        let ansi = writer.has_ansi_escapes();
        if ansi && !self.verbose {
            match level {
                tracing::Level::ERROR => write!(writer, "{} ", style("✖").red())?,
                tracing::Level::WARN => write!(writer, "{} ", style("•").yellow())?,
                _ => {}
            }
        }

        writeln!(writer, "{msg}")
    }
}
