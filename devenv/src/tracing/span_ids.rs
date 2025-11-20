use serde::Serialize;
use tracing_subscriber::{layer, registry::LookupSpan, Layer };
use tracing::{Subscriber, span};

/// Span ID information for JSON serialization
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SpanIds {
    /// The ID of this span
    span_id: String,
    /// The ID of the parent span, if any
    parent_span_id: Option<String>,
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

        // Get parent span ID if it exists
        let parent_span_id = span.parent().map(|parent| format!("{:?}", parent.id()));

        let span_ids = SpanIds {
            span_id: format!("{:?}", id),
            parent_span_id,
        };

        let mut extensions = span.extensions_mut();
        extensions.insert(span_ids);
    }
}
