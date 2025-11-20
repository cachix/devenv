use std::{collections::HashSet, fmt, sync::RwLock};
use tracing::{field::{Field, Visit}, span, Event, Subscriber};
use tracing_subscriber::{fmt::FormatFields, registry::LookupSpan, Layer, layer};

// Re-export
pub(crate) use tracing_indicatif::IndicatifLayer;

/// A filter layer that wraps IndicatifLayer and only shows progress bars for spans with `devenv.user_message`
pub(crate) struct DevenvIndicatifFilter<S, F> {
    inner: IndicatifLayer<S, F>,
    user_message_spans: RwLock<HashSet<span::Id>>,
}

impl<S, F> DevenvIndicatifFilter<S, F> {
    pub fn new(inner: IndicatifLayer<S, F>) -> Self {
        Self {
            inner,
            user_message_spans: RwLock::new(HashSet::new()),
        }
    }
}

impl<S, F> Layer<S> for DevenvIndicatifFilter<S, F>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    F: for<'writer> FormatFields<'writer> + 'static,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: layer::Context<'_, S>) {
        // Check if this span has devenv.user_message field and extract the message
        #[derive(Default)]
        struct UserMessageVisitor {
            user_message: Option<String>,
            no_spinner: bool,
        }

        impl Visit for UserMessageVisitor {
            fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}

            fn record_str(&mut self, field: &Field, value: &str) {
                if field.name() == "devenv.user_message" {
                    self.user_message = Some(value.to_string());
                }
            }

            fn record_bool(&mut self, field: &Field, value: bool) {
                if field.name() == "devenv.no_spinner" {
                    self.no_spinner = value;
                }
            }
        }

        let mut visitor = UserMessageVisitor::default();
        attrs.record(&mut visitor);

        if let Some(_user_message) = visitor.user_message {
            // This span has a user message, check if spinner should be disabled
            if !visitor.no_spinner {
                // Show progress bar only if spinner is not disabled
                if let Ok(mut spans) = self.user_message_spans.write() {
                    spans.insert(id.clone());
                }

                // Forward the span to IndicatifLayer - it will show devenv.user_message in {span_fields}
                self.inner.on_new_span(attrs, id, ctx);
            }
        }
    }

    fn on_enter(&self, id: &span::Id, ctx: layer::Context<'_, S>) {
        // Only forward if this is a user message span
        if let Ok(spans) = self.user_message_spans.read()
            && spans.contains(id)
        {
            self.inner.on_enter(id, ctx);
        }
    }

    fn on_exit(&self, id: &span::Id, ctx: layer::Context<'_, S>) {
        // Only forward if this is a user message span
        if let Ok(spans) = self.user_message_spans.read()
            && spans.contains(id)
        {
            self.inner.on_exit(id, ctx);
        }
    }

    fn on_close(&self, id: span::Id, ctx: layer::Context<'_, S>) {
        // Only forward if this is a user message span
        let should_forward = if let Ok(mut spans) = self.user_message_spans.write() {
            let contained = spans.contains(&id);
            spans.remove(&id); // Clean up
            contained
        } else {
            false
        };

        if should_forward {
            self.inner.on_close(id, ctx);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: layer::Context<'_, S>) {
        // Forward all events to IndicatifLayer so they appear above progress bars without interruption
        self.inner.on_event(event, ctx);
    }
}

