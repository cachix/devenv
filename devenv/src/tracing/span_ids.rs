use serde::Serialize;
use tracing::{Subscriber, span};
use tracing_subscriber::{Layer, layer, registry::LookupSpan};

/// Span context for JSON serialization (OTEL-aligned field names)
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SpanContext {
    /// The ID of this span
    span_id: u64,
    /// The ID of the parent span, if any
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_id: Option<u64>,
}

/// Layer that captures span IDs and stores them as extensions
pub(crate) struct SpanIdLayer;

impl<S> Layer<S> for SpanIdLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        _attrs: &span::Attributes<'_>,
        id: &span::Id,
        ctx: layer::Context<'_, S>,
    ) {
        let span = ctx.span(id).expect("Span not found in context");

        let parent_id = span.parent().map(|parent| parent.id().into_u64());

        let span_context = SpanContext {
            span_id: id.into_u64(),
            parent_id,
        };

        let mut extensions = span.extensions_mut();
        extensions.insert(span_context);
    }
}
