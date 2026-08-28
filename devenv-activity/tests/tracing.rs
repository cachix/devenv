use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use devenv_activity::{Activity, ActivityEvent, Build, FetchKind, ProcessStatus, start};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber, span};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{Layer, Registry};

#[derive(Debug, Default)]
struct RecordedFields {
    values: BTreeMap<&'static str, String>,
    valuable: Vec<&'static str>,
}

impl Visit for RecordedFields {
    fn record_value(&mut self, field: &Field, _value: valuable::Value<'_>) {
        self.valuable.push(field.name());
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
    assert!(spans[0].fields.valuable.contains(&"devenv.activity.event"));
    assert_eq!(
        spans[0].fields.values.get("devenv.derivation_path"),
        Some(&"/nix/store/example.drv".to_string())
    );
    assert_eq!(
        spans[0].fields.values.get("test.callsite"),
        Some(&"preserved".to_string())
    );

    let events = capture.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    for event in events.iter() {
        assert_eq!(event.parent, Some(spans[0].id));
        assert_eq!(event.target, "devenv_activity::events");
        assert!(event.fields.valuable.contains(&"event"));
        assert!(
            event
                .fields
                .values
                .get("source.file")
                .unwrap()
                .ends_with("tests/tracing.rs")
        );
    }
    assert_eq!(
        events[0].fields.values.get("source.line"),
        Some(&progress_line.to_string())
    );
    assert_eq!(
        events[1].fields.values.get("source.line"),
        Some(&log_line.to_string())
    );
    drop(events);
    drop(spans);

    assert!(matches!(
        receiver.try_recv().unwrap(),
        ActivityEvent::Build(Build::Start { id: 42, .. })
    ));
    assert!(matches!(
        receiver.try_recv().unwrap(),
        ActivityEvent::Build(Build::Progress {
            id: 42,
            done: 1,
            expected: 2,
            ..
        })
    ));
    assert!(matches!(
        receiver.try_recv().unwrap(),
        ActivityEvent::Build(Build::Log { id: 42, line, .. }) if line == "hello"
    ));
    assert!(matches!(
        receiver.try_recv().unwrap(),
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
                .ports(vec!["http:8080".to_string(), "admin:9000".to_string()])
                .ready_probe("http: localhost:8080/health"),
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
    saw_serialized_start: Arc<AtomicBool>,
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
        if attrs.metadata().name() != "activity" {
            return;
        }
        self.saw_activity_span.store(true, Ordering::Relaxed);
        let mut fields = RecordedFields::default();
        attrs.record(&mut fields);
        self.saw_serialized_start.store(
            fields.valuable.contains(&"devenv.activity.event"),
            Ordering::Relaxed,
        );
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().target() == "devenv_activity::events" {
            self.activity_events.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[test]
fn disabled_payload_target_skips_structured_values() {
    let capture = NoExportLayer::default();
    let subscriber = Registry::default().with(capture.clone());

    tracing::subscriber::with_default(subscriber, || {
        let activity = start!(Activity::build("example"));
        activity.progress(1, 2, None);
        activity.log("hello");
    });

    assert!(capture.saw_activity_span.load(Ordering::Relaxed));
    assert!(!capture.saw_serialized_start.load(Ordering::Relaxed));
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
