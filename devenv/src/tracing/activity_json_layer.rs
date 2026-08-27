use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;

use serde_json::{Map, Value, json};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use super::span_ids::SpanContext;

const ACTIVITY_TARGET: &str = "devenv_activity::events";
const ACTIVITY_EVENT_FIELD: &str = "devenv.activity.event";

fn now() -> Value {
    serde_json::to_value(devenv_activity::Timestamp::now()).unwrap_or(Value::Null)
}

#[derive(Default)]
struct JsonFields(BTreeMap<String, Value>);

impl JsonFields {
    fn remove(&mut self, name: &str) -> Option<Value> {
        self.0.remove(name)
    }

    fn into_map(self) -> Map<String, Value> {
        self.0.into_iter().collect()
    }
}

impl Visit for JsonFields {
    fn record_value(&mut self, field: &Field, value: valuable::Value<'_>) {
        if let Ok(value) = serde_json::to_value(valuable_serde::Serializable::new(value)) {
            self.0.insert(field.name().to_owned(), value);
        }
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.0.insert(field.name().to_owned(), value.into());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0.insert(field.name().to_owned(), value.into());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().to_owned(), value.into());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.insert(field.name().to_owned(), value.into());
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.into());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0
            .insert(field.name().to_owned(), format!("{value:?}").into());
    }
}

struct ActivitySpanState {
    start: Value,
    outcome: String,
}

/// JSON-specific adapter for the native tracing representation of activities.
///
/// Activity starts and completions are span lifecycle transitions, while
/// progress/log/effect updates are tracing events. JSONL replay expects a
/// uniform stream, so this export layer serializes those transitions as their
/// corresponding typed events. The first-party activity channel remains
/// completely independent of this layer.
pub(super) struct ActivityJsonLayer<W> {
    writer: W,
}

impl<W> ActivityJsonLayer<W> {
    pub(super) fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W> ActivityJsonLayer<W>
where
    W: for<'writer> MakeWriter<'writer>,
{
    fn write_record(
        &self,
        metadata: &Metadata<'_>,
        mut fields: Map<String, Value>,
        span_context: Option<Value>,
    ) {
        let has_tracked_caller = fields.contains_key("source.file");
        fields.entry("source.file".to_owned()).or_insert_with(|| {
            metadata
                .file()
                .map_or(Value::Null, |file| Value::String(file.to_owned()))
        });
        fields.entry("source.line".to_owned()).or_insert_with(|| {
            metadata
                .line()
                .map_or(Value::Null, |line| Value::Number(line.into()))
        });
        if !has_tracked_caller {
            fields.entry("source.module".to_owned()).or_insert_with(|| {
                metadata
                    .module_path()
                    .map_or(Value::Null, |module| Value::String(module.to_owned()))
            });
        }

        let mut record = json!({
            "timestamp": now(),
            "level": metadata.level().as_str().to_ascii_lowercase(),
            "target": ACTIVITY_TARGET,
            "fields": fields,
        });
        if let Some(span_context) = span_context {
            record["span_context"] = span_context;
        }

        let mut writer = self.writer.make_writer_for(metadata);
        if serde_json::to_writer(&mut writer, &record).is_ok() {
            let _ = writer.write_all(b"\n");
        }
    }
}

impl<S, W> Layer<S> for ActivityJsonLayer<W>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut fields = JsonFields::default();
        attrs.record(&mut fields);
        let Some(start) = fields.remove(ACTIVITY_EVENT_FIELD) else {
            return;
        };

        let Some(span) = ctx.span(id) else {
            return;
        };
        let span_context = span
            .extensions()
            .get::<SpanContext>()
            .and_then(|context| serde_json::to_value(context).ok());
        span.extensions_mut().insert(ActivitySpanState {
            start: start.clone(),
            outcome: "success".to_owned(),
        });

        self.write_record(
            attrs.metadata(),
            Map::from_iter([("event".to_owned(), start)]),
            span_context,
        );
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut fields = JsonFields::default();
        values.record(&mut fields);
        let outcome = fields.remove("devenv.outcome");
        let complete = matches!(
            fields.remove("devenv.activity.complete"),
            Some(Value::Bool(true))
        );

        let mut extensions = span.extensions_mut();
        let Some(state) = extensions.get_mut::<ActivitySpanState>() else {
            return;
        };
        if let Some(Value::String(outcome)) = outcome {
            state.outcome = outcome;
        }
        if !complete {
            return;
        }

        let Some(start) = state.start.as_object() else {
            return;
        };
        let (Some(kind), Some(id)) = (start.get("activity_kind"), start.get("id")) else {
            return;
        };
        let complete = json!({
            "activity_kind": kind,
            "event": "complete",
            "id": id,
            "outcome": state.outcome,
            "timestamp": now(),
        });
        drop(extensions);
        let span_context = span
            .extensions()
            .get::<SpanContext>()
            .and_then(|context| serde_json::to_value(context).ok());
        self.write_record(
            span.metadata(),
            Map::from_iter([("event".to_owned(), complete)]),
            span_context,
        );
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        if event.metadata().target() != ACTIVITY_TARGET {
            return;
        }
        let mut fields = JsonFields::default();
        event.record(&mut fields);
        let span_context = ctx.event_span(event).and_then(|span| {
            span.extensions()
                .get::<SpanContext>()
                .and_then(|context| serde_json::to_value(context).ok())
        });
        self.write_record(event.metadata(), fields.into_map(), span_context);
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else {
            return;
        };
        span.extensions_mut().remove::<ActivitySpanState>();
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

        tracing::subscriber::with_default(subscriber, || {
            let activity = start!(Activity::build("example").id(73));
            activity.progress(1, 2, None);
            activity.log("hello");
            activity.fail();
            drop(activity);
        });

        let output = String::from_utf8(output.0.lock().unwrap().clone()).unwrap();
        let events = output
            .lines()
            .map(|line| {
                let mut line: serde_json::Value = serde_json::from_str(line).unwrap();
                assert_eq!(line["target"], "devenv_activity::events");
                serde_json::from_value::<ActivityEvent>(line["fields"]["event"].take()).unwrap()
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
    }
}
