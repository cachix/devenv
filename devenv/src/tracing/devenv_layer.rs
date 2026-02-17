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

/// The kind of span event based on its lifecycle.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum SpanKind {
    /// Marks the start of a span.
    /// Equivalent to [tracing_subscriber::fmt::format::FmtSpan::NEW].
    Start = 0,
    /// Marks the end of a span.
    /// Equivalent to [tracing_subscriber::fmt::format::FmtSpan::CLOSE].
    End = 1,
}

impl TryFrom<u8> for SpanKind {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, ()> {
        match value {
            0 => Ok(SpanKind::Start),
            1 => Ok(SpanKind::End),
            _ => Err(()),
        }
    }
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
                if field.name() == "devenv.user_message" {
                    self.user_message = Some(value.to_string());
                }
            }
        }

        let mut visitor = UserMessageVisitor::default();
        attrs.record(&mut visitor);

        let mut ext = span.extensions_mut();

        if let Some(msg) = visitor.user_message {
            ext.insert(SpanContext {
                msg: msg.clone(),
                has_error: false,
                timings: SpanTimings::new(),
            });

            // Emit a start event to show the initial message
            let msg = msg.clone();
            with_event_from_span!(
                id,
                span,
                "message" = msg,
                "devenv.ui.message" = true,
                "devenv.span_event_kind" = SpanKind::Start as u8,
                "devenv.span_has_error" = false,
                |event| {
                    drop(ext);
                    drop(span);
                    ctx.event(&event);
                }
            );
        }
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
                "devenv.ui.message" = true,
                "devenv.span_event_kind" = SpanKind::End as u8,
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
        ctx: &FmtContext<'_, S, F>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        #[derive(Debug, Default)]
        struct EventVisitor {
            message: Option<String>,
            is_user_message: bool,
            span_event_kind: Option<SpanKind>,
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

            fn record_bool(&mut self, field: &Field, value: bool) {
                if field.name() == "devenv.ui.message" {
                    self.is_user_message = value;
                }
            }

            fn record_u64(&mut self, field: &Field, value: u64) {
                if field.name() == "devenv.span_event_kind" {
                    self.span_event_kind = SpanKind::try_from(value as u8).ok()
                }
            }
        }

        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);

        if let Some(span_kind) = visitor.span_event_kind
            && let Some(span) = ctx.parent_span()
        {
            let ext = span.extensions();

            if let Some(span_ctx) = ext.get::<SpanContext>()
                && visitor.is_user_message
            {
                let ansi = writer.has_ansi_escapes();
                let time_total = format!("{}", span_ctx.timings.total_duration());
                let has_error = span_ctx.has_error;
                let msg = &span_ctx.msg;
                match span_kind {
                    SpanKind::Start => {
                        if ansi {
                            write!(writer, "{prefix} ", prefix = style("•").blue())?;
                        }
                        return writeln!(writer, "{msg}");
                    }

                    SpanKind::End => {
                        if ansi {
                            let prefix = if has_error {
                                style("✖").red()
                            } else {
                                style("✓").green()
                            };
                            write!(writer, "{prefix} ")?;
                        }
                        return writeln!(writer, "{msg} in {time_total}");
                    }
                }
            }
        }
        if let Some(msg) = visitor.message {
            if visitor.is_user_message {
                let meta = event.metadata();
                let ansi = writer.has_ansi_escapes();

                if ansi && !self.verbose {
                    let level = meta.level();
                    match *level {
                        tracing::Level::ERROR => {
                            write!(writer, "{} ", style("✖").red())?;
                        }
                        tracing::Level::WARN => {
                            write!(writer, "{} ", style("•").yellow())?;
                        }
                        tracing::Level::INFO => {
                            write!(writer, "{} ", style("•").blue())?;
                        }
                        tracing::Level::DEBUG => {
                            write!(writer, "{} ", style("•").italic())?;
                        }
                        _ => {}
                    }
                }
            }

            writeln!(writer, "{msg}")?;
        };

        Ok(())
    }
}
