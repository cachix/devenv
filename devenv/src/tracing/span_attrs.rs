use serde::Serialize;
use std::{fmt, collections::HashMap};
use tracing::{span, field::{Field, Visit}, Subscriber};
use tracing_subscriber::{Layer, layer, registry::LookupSpan};


/// Stores span attributes for serialization
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SpanAttributes {
    fields: HashMap<String, String>,
}

/// Layer that captures span attributes and stores them as extensions
pub(crate) struct SpanAttributesLayer;

impl<S> Layer<S> for SpanAttributesLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx.span(id).expect("Span not found in context");

        // Collect all span attributes
        struct AttrVisitor {
            fields: HashMap<String, String>,
        }

        impl Visit for AttrVisitor {
            fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
                self.fields
                    .insert(field.name().to_string(), format!("{:?}", value));
            }

            fn record_str(&mut self, field: &Field, value: &str) {
                self.fields
                    .insert(field.name().to_string(), value.to_string());
            }

            fn record_u64(&mut self, field: &Field, value: u64) {
                self.fields
                    .insert(field.name().to_string(), value.to_string());
            }

            fn record_i64(&mut self, field: &Field, value: i64) {
                self.fields
                    .insert(field.name().to_string(), value.to_string());
            }

            fn record_bool(&mut self, field: &Field, value: bool) {
                self.fields
                    .insert(field.name().to_string(), value.to_string());
            }
        }

        let mut visitor = AttrVisitor {
            fields: HashMap::new(),
        };
        attrs.record(&mut visitor);

        // Store span attributes as extension
        let span_attrs = SpanAttributes {
            fields: visitor.fields,
        };
        let mut extensions = span.extensions_mut();
        extensions.insert(span_attrs);
    }
}
