use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use devenv_activity::{
    Activity, ActivityEvent, Build, FetchKind, HttpProbe, PortBinding, ProcessStatus, ReadyProbe,
    start,
};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber, span};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{Layer, Registry};

#[derive(Debug, Default)]
struct RecordedFields {
    values: BTreeMap<&'static str, String>,
    /// Structured values, re-serialized to JSON the way an export layer would.
    valuable: BTreeMap<&'static str, serde_json::Value>,
}

impl Visit for RecordedFields {
    fn record_value(&mut self, field: &Field, value: valuable::Value<'_>) {
        let json = serde_json::to_value(valuable_serde::Serializable::new(value)).unwrap();
        self.valuable.insert(field.name(), json);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.values.insert(field.name(), value.to_owned());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.values.insert(field.name(), value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.values.insert(field.name(), format!("{value:?}"));
    }
}

#[derive(Debug)]
struct SpanRecord {
    id: u64,
    file: Option<&'static str>,
    line: Option<u32>,
    fields: RecordedFields,
}

#[derive(Debug)]
struct EventRecord {
    parent: Option<u64>,
    target: &'static str,
    fields: RecordedFields,
}

#[derive(Clone, Default)]
struct CaptureLayer {
    spans: Arc<Mutex<Vec<SpanRecord>>>,
    events: Arc<Mutex<Vec<EventRecord>>>,
}

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, _ctx: Context<'_, S>) {
        if attrs.metadata().name() != "activity" {
            return;
        }
        let mut fields = RecordedFields::default();
        attrs.record(&mut fields);
        self.spans.lock().unwrap().push(SpanRecord {
            id: id.clone().into_u64(),
            file: attrs.metadata().file(),
            line: attrs.metadata().line(),
            fields,
        });
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().target() != "devenv_activity::events" {
            return;
        }
        let mut fields = RecordedFields::default();
        event.record(&mut fields);
        self.events.lock().unwrap().push(EventRecord {
            parent: event.parent().map(|id| id.clone().into_u64()),
            target: event.metadata().target(),
            fields,
        });
    }

    fn on_record(&self, id: &span::Id, values: &span::Record<'_>, _ctx: Context<'_, S>) {
        let mut spans = self.spans.lock().unwrap();
        let Some(span) = spans
            .iter_mut()
            .find(|span| span.id == id.clone().into_u64())
        else {
            return;
        };
        values.record(&mut span.fields);
    }
}

#[test]
fn native_span_and_updates_preserve_activity_data_and_callers() {
    let capture = CaptureLayer::default();
    let subscriber = Registry::default().with(capture.clone());
    let dispatch = tracing::Dispatch::new(subscriber);
    let (mut receiver, handle) = devenv_activity::init();
    let _activity_guard = handle.install();

    let (start_line, progress_line, log_line) =
        tracing::dispatcher::with_default(&dispatch, || {
            let start_line = line!() + 1;
            let activity = start!(
                Activity::build("example")
                    .derivation_path("/nix/store/example.drv")
                    .id(42),
                test.callsite = "preserved"
            );
            let progress_line = line!() + 1;
            activity.progress(1, 2, None);
            let log_line = line!() + 1;
            activity.log("hello");
            drop(activity);
            (start_line, progress_line, log_line)
        });

    let spans = capture.spans.lock().unwrap();
    assert_eq!(spans.len(), 1);
    assert!(spans[0].file.unwrap().ends_with("tests/tracing.rs"));
    assert_eq!(spans[0].line, Some(start_line));
    assert_eq!(
        spans[0].fields.values.get("devenv.ui.message"),
        Some(&"example".to_owned())
    );
    assert_eq!(
        spans[0].fields.values.get("devenv.derivation_path"),
        Some(&"/nix/store/example.drv".to_string())
    );
    assert_eq!(
        spans[0].fields.values.get("test.callsite"),
        Some(&"preserved".to_string())
    );
    assert!(
        spans[0].fields.valuable.is_empty(),
        "activity spans carry scalar attributes only"
    );

    // Start, progress, log and complete are all events under the span.
    let events = capture.events.lock().unwrap();
    assert_eq!(events.len(), 4);
    for event in events.iter() {
        assert_eq!(event.parent, Some(spans[0].id));
        assert_eq!(event.target, "devenv_activity::events");
        assert!(event.fields.valuable.contains_key("event"));
        assert!(
            event
                .fields
                .values
                .get("source.file")
                .unwrap()
                .ends_with("tests/tracing.rs")
        );
    }
    let source_lines: Vec<_> = events
        .iter()
        .map(|event| event.fields.values.get("source.line").unwrap().clone())
        .collect();
    assert_eq!(
        source_lines,
        [
            start_line.to_string(),
            progress_line.to_string(),
            log_line.to_string(),
            // Completion is attributed to where the activity was started.
            start_line.to_string(),
        ]
    );

    // The traced payload is the channel event, byte for byte.
    let channel_events: Vec<ActivityEvent> =
        std::iter::from_fn(|| receiver.try_recv().ok()).collect();
    assert_eq!(channel_events.len(), 4);
    for (traced, sent) in events.iter().zip(&channel_events) {
        assert_eq!(
            traced.fields.valuable["event"],
            serde_json::to_value(sent).unwrap()
        );
    }
    assert!(matches!(
        channel_events[0],
        ActivityEvent::Build(Build::Start { id: 42, .. })
    ));
    assert!(matches!(
        channel_events[1],
        ActivityEvent::Build(Build::Progress {
            id: 42,
            done: 1,
            expected: 2,
            ..
        })
    ));
    assert!(matches!(
        &channel_events[2],
        ActivityEvent::Build(Build::Log { id: 42, line, .. }) if line == "hello"
    ));
    assert!(matches!(
        channel_events[3],
        ActivityEvent::Build(Build::Complete { id: 42, .. })
    ));
}

#[test]
fn activity_spans_export_borrowed_semantic_fields() {
    let capture = CaptureLayer::default();
    let subscriber = Registry::default().with(capture.clone());
    let dispatch = tracing::Dispatch::new(subscriber);

    tracing::dispatcher::with_default(&dispatch, || {
        drop(start!(
            Activity::fetch(FetchKind::Download, "source").url("https://example.test/source")
        ));
        drop(start!(
            Activity::command("execute command").command("echo hello")
        ));
        drop(start!(Activity::task("project:build")));
        let process = start!(
            Activity::process("web")
                .command("run-web")
                .ports(vec![
                    PortBinding {
                        name: "http".to_string(),
                        port: 8080,
                    },
                    PortBinding {
                        name: "admin".to_string(),
                        port: 9000,
                    },
                ])
                .ready_probe(ReadyProbe::Http(Box::new(HttpProbe {
                    host: "localhost".to_string(),
                    port: 8080,
                    path: "/health".to_string(),
                }))),
            devenv.process.status = tracing::field::Empty
        );
        process.set_status(ProcessStatus::Ready);
        drop(process);
        drop(start!(
            Activity::operation("Reloading shell").detail("devenv.nix")
        ));
    });

    let spans = capture.spans.lock().unwrap();
    assert_eq!(spans.len(), 5);
    assert_eq!(
        spans[0].fields.values.get("devenv.fetch.kind"),
        Some(&"download".to_string())
    );
    assert_eq!(
        spans[0].fields.values.get("devenv.url"),
        Some(&"https://example.test/source".to_string())
    );
    assert_eq!(
        spans[1].fields.values.get("devenv.command"),
        Some(&"echo hello".to_string())
    );
    assert_eq!(
        spans[2].fields.values.get("devenv.task.name"),
        Some(&"project:build".to_string())
    );
    assert_eq!(
        spans[3].fields.values.get("devenv.process.name"),
        Some(&"web".to_string())
    );
    assert_eq!(
        spans[3].fields.values.get("devenv.process.port_count"),
        Some(&"2".to_string())
    );
    assert_eq!(
        spans[3].fields.values.get("devenv.process.ready_probe"),
        Some(&"http: localhost:8080/health".to_string())
    );
    assert_eq!(
        spans[3].fields.values.get("devenv.process.status"),
        Some(&"ready".to_string())
    );
    assert_eq!(
        spans[4].fields.values.get("devenv.operation.detail"),
        Some(&"devenv.nix".to_string())
    );
}

#[derive(Clone, Default)]
struct NoExportLayer {
    saw_activity_span: Arc<AtomicBool>,
    activity_events: Arc<AtomicUsize>,
}

#[derive(Clone, Default)]
struct DisableActivitySpans;

impl<S> Layer<S> for DisableActivitySpans
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn enabled(&self, metadata: &tracing::Metadata<'_>, _ctx: Context<'_, S>) -> bool {
        metadata.target() != "devenv_activity::spans"
    }
}

impl<S> Layer<S> for NoExportLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn enabled(&self, metadata: &tracing::Metadata<'_>, _ctx: Context<'_, S>) -> bool {
        !matches!(
            metadata.target(),
            "devenv_activity::events" | "devenv_activity::replay"
        )
    }

    fn on_new_span(&self, attrs: &span::Attributes<'_>, _id: &span::Id, _ctx: Context<'_, S>) {
        if attrs.metadata().name() == "activity" {
            self.saw_activity_span.store(true, Ordering::Relaxed);
        }
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().target() == "devenv_activity::events" {
            self.activity_events.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[test]
fn disabled_events_target_emits_no_payloads() {
    let capture = NoExportLayer::default();
    let subscriber = Registry::default().with(capture.clone());

    tracing::subscriber::with_default(subscriber, || {
        let activity = start!(Activity::build("example"));
        activity.progress(1, 2, None);
        activity.log("hello");
    });

    assert!(capture.saw_activity_span.load(Ordering::Relaxed));
    assert_eq!(capture.activity_events.load(Ordering::Relaxed), 0);
}

#[test]
fn disabled_activity_spans_do_not_evaluate_export_only_fields() {
    let evaluated = Cell::new(false);
    let subscriber = Registry::default().with(DisableActivitySpans);

    tracing::subscriber::with_default(subscriber, || {
        drop(start!(
            Activity::operation("disabled span"),
            test.export_only = {
                evaluated.set(true);
                "unused"
            }
        ));
    });

    assert!(!evaluated.get());
}
