use std::fmt;
use std::io::Write;

use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing::{Event, Metadata, Subscriber};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use super::span_ids::SpanContext;

const ACTIVITY_TARGET: &str = "devenv_activity::events";

/// JSONL export of activity events.
///
/// Every activity transition, including start and completion, reaches
/// tracing as a `devenv_activity::events` event whose `event` field borrows
/// the typed [`devenv_activity::ActivityEvent`]. This layer streams that
/// field straight into one JSON line, so the payload is serialized exactly
/// once, here, and never at the producer. The first-party activity channel
/// remains completely independent of this layer.
pub(super) struct ActivityJsonLayer<W> {
    writer: W,
}

impl<W> ActivityJsonLayer<W> {
    pub(super) fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<S, W> Layer<S> for ActivityJsonLayer<W>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let metadata = event.metadata();
        if metadata.target() != ACTIVITY_TARGET {
            return;
        }

        let mut record = JsonRecord::begin(metadata);
        event.record(&mut record);

        let span = ctx.event_span(event);
        let extensions = span.as_ref().map(|span| span.extensions());
        let span_context = extensions
            .as_ref()
            .and_then(|extensions| extensions.get::<SpanContext>());
        let line = record.finish(metadata, span_context);

        let mut writer = self.writer.make_writer_for(metadata);
        let _ = writer.write_all(&line);
    }
}

/// One JSONL record, written field by field as tracing visits them.
///
/// The record is assembled in memory and written with a single call, so
/// records from concurrent events never interleave.
struct JsonRecord {
    buf: Vec<u8>,
    first_field: bool,
    saw_source_file: bool,
    saw_source_line: bool,
}

impl JsonRecord {
    fn begin(metadata: &Metadata<'_>) -> Self {
        let mut record = Self {
            buf: Vec::with_capacity(256),
            first_field: true,
            saw_source_file: false,
            saw_source_line: false,
        };
        record.buf.extend_from_slice(b"{\"timestamp\":");
        record.raw(&devenv_activity::Timestamp::now());
        record.buf.extend_from_slice(b",\"level\":");
        record.raw(&metadata.level().as_str().to_ascii_lowercase());
        record.buf.extend_from_slice(b",\"target\":");
        record.raw(ACTIVITY_TARGET);
        record.buf.extend_from_slice(b",\"fields\":{");
        record
    }

    /// Append a JSON value. On a serialization error the partial output is
    /// discarded and `null` is written instead, keeping the line well formed.
    fn raw<T: Serialize + ?Sized>(&mut self, value: &T) {
        let start = self.buf.len();
        if serde_json::to_writer(&mut self.buf, value).is_err() {
            self.buf.truncate(start);
            self.buf.extend_from_slice(b"null");
        }
    }

    fn field<T: Serialize + ?Sized>(&mut self, name: &str, value: &T) {
        match name {
            "source.file" => self.saw_source_file = true,
            "source.line" => self.saw_source_line = true,
            _ => {}
        }
        if !self.first_field {
            self.buf.push(b',');
        }
        self.first_field = false;
        self.raw(name);
        self.buf.push(b':');
        self.raw(value);
    }

    fn finish(mut self, metadata: &Metadata<'_>, span_context: Option<&SpanContext>) -> Vec<u8> {
        // Events emitted through the tracked public API carry their caller.
        // Fall back to the callsite metadata for anything else.
        let has_tracked_caller = self.saw_source_file;
        if !self.saw_source_file {
            self.field("source.file", &metadata.file());
        }
        if !self.saw_source_line {
            self.field("source.line", &metadata.line());
        }
        if !has_tracked_caller {
            self.field("source.module", &metadata.module_path());
        }
        self.buf.push(b'}');
        if let Some(span_context) = span_context {
            self.buf.extend_from_slice(b",\"span_context\":");
            self.raw(span_context);
        }
        self.buf.extend_from_slice(b"}\n");
        self.buf
    }
}

impl Visit for JsonRecord {
    fn record_value(&mut self, field: &Field, value: valuable::Value<'_>) {
        self.field(field.name(), &valuable_serde::Serializable::new(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.field(field.name(), &value);
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.field(field.name(), &value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.field(field.name(), &value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.field(field.name(), &value);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.field(field.name(), value);
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.field(field.name(), &format_args!("{value:?}"));
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex, MutexGuard};

    use devenv_activity::{Activity, ActivityEvent, ActivityOutcome, Build, start};
    use tracing_subscriber::Registry;
    use tracing_subscriber::layer::SubscriberExt;

    use super::ActivityJsonLayer;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    struct BufferGuard<'a>(MutexGuard<'a, Vec<u8>>);

    impl io::Write for BufferGuard<'_> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buffer {
        type Writer = BufferGuard<'a>;

        fn make_writer(&'a self) -> Self::Writer {
            BufferGuard(self.0.lock().unwrap())
        }
    }

    #[test]
    fn exports_native_activity_span_and_updates_as_replayable_events() {
        let output = Buffer::default();
        let subscriber = Registry::default().with(ActivityJsonLayer::new(output.clone()));

        let start_line = tracing::subscriber::with_default(subscriber, || {
            let start_line = line!() + 1;
            let activity = start!(Activity::build("example").id(73));
            activity.progress(1, 2, None);
            activity.log("hello");
            activity.fail();
            drop(activity);
            start_line
        });

        let output = String::from_utf8(output.0.lock().unwrap().clone()).unwrap();
        let records = output
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        let events = records
            .iter()
            .map(|record| {
                assert_eq!(record["target"], "devenv_activity::events");
                assert_eq!(record["level"], "debug");
                assert!(record["timestamp"].is_string());
                assert!(
                    record["fields"]["source.file"]
                        .as_str()
                        .unwrap()
                        .ends_with("activity_json_layer.rs")
                );
                assert!(record["fields"].get("source.module").is_none());
                serde_json::from_value::<ActivityEvent>(record["fields"]["event"].clone()).unwrap()
            })
            .collect::<Vec<_>>();

        assert_eq!(events.len(), 4);
        assert!(matches!(
            &events[0],
            ActivityEvent::Build(Build::Start { id: 73, name, .. }) if name == "example"
        ));
        assert!(matches!(
            events[1],
            ActivityEvent::Build(Build::Progress {
                id: 73,
                done: 1,
                expected: 2,
                ..
            })
        ));
        assert!(matches!(
            &events[2],
            ActivityEvent::Build(Build::Log { id: 73, line, .. }) if line == "hello"
        ));
        assert!(matches!(
            events[3],
            ActivityEvent::Build(Build::Complete {
                id: 73,
                outcome: ActivityOutcome::Failed,
                ..
            })
        ));

        // Start and completion both point at the line that started the activity.
        assert_eq!(records[0]["fields"]["source.line"], start_line);
        assert_eq!(records[3]["fields"]["source.line"], start_line);
    }
}
