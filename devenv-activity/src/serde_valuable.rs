//! Bridge from serde to valuable without an intermediate value tree.
//!
//! Tracing's structured-field support accepts a [`valuable::Value`], which
//! must be produced at the call site before any layer has said what it wants.
//! [`SerdeValuable`] borrows a [`Serialize`] value and walks it lazily: when a
//! layer visits the field, the value's own `Serialize` impl drives the
//! [`Visit`] callbacks directly. Nothing is allocated at the producer, and a
//! layer that re-serializes the visited value (for example with
//! `valuable_serde`) produces exactly the JSON that `serde_json` would.

use std::fmt::{self, Display, Write as _};

use serde::ser::{
    self, Impossible, Serialize, SerializeMap, SerializeSeq, SerializeStruct,
    SerializeStructVariant, SerializeTuple, SerializeTupleStruct, SerializeTupleVariant,
    Serializer,
};
use valuable::{Listable, Mappable, Slice, Valuable, Value, Visit};

/// Borrow a serde-serializable value as a `valuable` value.
///
/// Field names, `#[serde(rename_all)]`, `#[serde(tag)]`, and
/// `#[serde(skip_serializing_if)]` all apply, because the value's `Serialize`
/// impl is what drives the visitor.
///
/// The borrowed value must serialize as a map (a struct, an internally tagged
/// enum, a map) or as a sequence. A top-level primitive is reported as
/// [`Value::Unit`]. Nested values of every kind are supported.
///
/// # Example
///
/// ```ignore
/// use devenv_activity::SerdeValuable;
///
/// tracing::debug!(event = SerdeValuable(&event).as_tracing_value());
/// ```
pub struct SerdeValuable<'a, T: ?Sized>(pub &'a T);

impl<T: Serialize + ?Sized> SerdeValuable<'_, T> {
    /// Borrow this value in the form accepted by tracing's structured-value
    /// field support. This keeps exported macros from requiring downstream
    /// crates to depend on or import `valuable` themselves.
    pub fn as_tracing_value(&self) -> Value<'_> {
        self.as_value()
    }
}

impl<T: Serialize + ?Sized> Valuable for SerdeValuable<'_, T> {
    fn as_value(&self) -> Value<'_> {
        match kind_of(self.0) {
            Kind::Map => Value::Mappable(self),
            Kind::Seq => Value::Listable(self),
            Kind::Primitive | Kind::Unit => Value::Unit,
        }
    }

    fn visit(&self, visit: &mut dyn Visit) {
        let _ = self.0.serialize(VisitSerializer { visit });
    }
}

impl<T: Serialize + ?Sized> Mappable for SerdeValuable<'_, T> {
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }
}

impl<T: Serialize + ?Sized> Listable for SerdeValuable<'_, T> {
    fn size_hint(&self) -> (usize, Option<usize>) {
        exact_len(self.0)
    }
}

// ---------------------------------------------------------------------------
// Errors and kind probing
// ---------------------------------------------------------------------------

/// The shape a value takes when serialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Unit,
    Primitive,
    Seq,
    Map,
}

/// Serializer error type.
///
/// `Handled` and `Kind` are control flow, not failures: they stop serde's
/// traversal early once the visitor has taken over a nested value, or once
/// the shape of a value is known.
#[derive(Debug)]
enum Error {
    Handled,
    Kind(Kind),
    Message(String),
}

impl ser::Error for Error {
    fn custom<T: Display>(msg: T) -> Self {
        Error::Message(msg.to_string())
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Handled => f.write_str("value handled by visitor"),
            Error::Kind(kind) => write!(f, "value kind probed: {kind:?}"),
            Error::Message(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for Error {}

fn kind_of<T: Serialize + ?Sized>(value: &T) -> Kind {
    match value.serialize(KindProbe) {
        Ok(kind) | Err(Error::Kind(kind)) => kind,
        Err(_) => Kind::Unit,
    }
}

/// Reports the shape of a value without walking into it.
struct KindProbe;

macro_rules! probe_primitive {
    ($($method:ident: $ty:ty),* $(,)?) => {
        $(
            fn $method(self, _: $ty) -> Result<Kind, Error> {
                Ok(Kind::Primitive)
            }
        )*
    };
}

impl Serializer for KindProbe {
    type Ok = Kind;
    type Error = Error;
    type SerializeSeq = Impossible<Kind, Error>;
    type SerializeTuple = Impossible<Kind, Error>;
    type SerializeTupleStruct = Impossible<Kind, Error>;
    type SerializeTupleVariant = Impossible<Kind, Error>;
    type SerializeMap = Impossible<Kind, Error>;
    type SerializeStruct = Impossible<Kind, Error>;
    type SerializeStructVariant = Impossible<Kind, Error>;

    probe_primitive! {
        serialize_bool: bool,
        serialize_i8: i8,
        serialize_i16: i16,
        serialize_i32: i32,
        serialize_i64: i64,
        serialize_i128: i128,
        serialize_u8: u8,
        serialize_u16: u16,
        serialize_u32: u32,
        serialize_u64: u64,
        serialize_u128: u128,
        serialize_f32: f32,
        serialize_f64: f64,
        serialize_char: char,
        serialize_str: &str,
    }

    fn serialize_bytes(self, _: &[u8]) -> Result<Kind, Error> {
        Err(Error::Kind(Kind::Seq))
    }

    fn collect_str<T: Display + ?Sized>(self, _: &T) -> Result<Kind, Error> {
        Ok(Kind::Primitive)
    }

    fn serialize_none(self) -> Result<Kind, Error> {
        Ok(Kind::Unit)
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Kind, Error> {
        value.serialize(KindProbe)
    }

    fn serialize_unit(self) -> Result<Kind, Error> {
        Ok(Kind::Unit)
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<Kind, Error> {
        Ok(Kind::Unit)
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<Kind, Error> {
        Ok(Kind::Primitive)
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<Kind, Error> {
        value.serialize(KindProbe)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<Kind, Error> {
        Err(Error::Kind(Kind::Map))
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        Err(Error::Kind(Kind::Seq))
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Error> {
        Err(Error::Kind(Kind::Seq))
    }

    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        Err(Error::Kind(Kind::Seq))
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        Err(Error::Kind(Kind::Map))
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Error> {
        Err(Error::Kind(Kind::Map))
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Error> {
        Err(Error::Kind(Kind::Map))
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        Err(Error::Kind(Kind::Map))
    }
}

/// The exact element count of a sequence value, as a `size_hint`.
///
/// `valuable-serde` announces `upper.unwrap_or(lower)` as the sequence length
/// to the target serializer, and serde_json closes an array announced as empty
/// before any element is written. The hint must therefore be exact.
fn exact_len<T: Serialize + ?Sized>(value: &T) -> (usize, Option<usize>) {
    let len = value.serialize(LenProbe).unwrap_or(0);
    (len, Some(len))
}

/// Counts the elements of a sequence without walking into them.
struct LenProbe;

/// Element counter for [`LenProbe`].
struct ElementCounter(usize);

impl SerializeSeq for ElementCounter {
    type Ok = usize;
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, _: &T) -> Result<(), Error> {
        self.0 += 1;
        Ok(())
    }

    fn end(self) -> Result<usize, Error> {
        Ok(self.0)
    }
}

impl SerializeTuple for ElementCounter {
    type Ok = usize;
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, _: &T) -> Result<(), Error> {
        self.0 += 1;
        Ok(())
    }

    fn end(self) -> Result<usize, Error> {
        Ok(self.0)
    }
}

impl SerializeTupleStruct for ElementCounter {
    type Ok = usize;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &T) -> Result<(), Error> {
        self.0 += 1;
        Ok(())
    }

    fn end(self) -> Result<usize, Error> {
        Ok(self.0)
    }
}

macro_rules! probe_not_a_sequence {
    ($($method:ident: $ty:ty),* $(,)?) => {
        $(
            fn $method(self, _: $ty) -> Result<usize, Error> {
                Err(Error::Kind(Kind::Primitive))
            }
        )*
    };
}

impl Serializer for LenProbe {
    type Ok = usize;
    type Error = Error;
    type SerializeSeq = ElementCounter;
    type SerializeTuple = ElementCounter;
    type SerializeTupleStruct = ElementCounter;
    type SerializeTupleVariant = Impossible<usize, Error>;
    type SerializeMap = Impossible<usize, Error>;
    type SerializeStruct = Impossible<usize, Error>;
    type SerializeStructVariant = Impossible<usize, Error>;

    probe_not_a_sequence! {
        serialize_bool: bool,
        serialize_i8: i8,
        serialize_i16: i16,
        serialize_i32: i32,
        serialize_i64: i64,
        serialize_i128: i128,
        serialize_u8: u8,
        serialize_u16: u16,
        serialize_u32: u32,
        serialize_u64: u64,
        serialize_u128: u128,
        serialize_f32: f32,
        serialize_f64: f64,
        serialize_char: char,
        serialize_str: &str,
        serialize_bytes: &[u8],
    }

    fn serialize_none(self) -> Result<usize, Error> {
        Err(Error::Kind(Kind::Unit))
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<usize, Error> {
        value.serialize(LenProbe)
    }

    fn serialize_unit(self) -> Result<usize, Error> {
        Err(Error::Kind(Kind::Unit))
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<usize, Error> {
        Err(Error::Kind(Kind::Unit))
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<usize, Error> {
        Err(Error::Kind(Kind::Primitive))
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<usize, Error> {
        value.serialize(LenProbe)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<usize, Error> {
        Err(Error::Kind(Kind::Map))
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        Ok(ElementCounter(0))
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Error> {
        Ok(ElementCounter(0))
    }

    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        Ok(ElementCounter(0))
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        Err(Error::Kind(Kind::Map))
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Error> {
        Err(Error::Kind(Kind::Map))
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Error> {
        Err(Error::Kind(Kind::Map))
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        Err(Error::Kind(Kind::Map))
    }
}

// ---------------------------------------------------------------------------
// Nested containers, borrowed from their parent
// ---------------------------------------------------------------------------

/// A nested value that serializes as a map: a struct, a map, or an enum
/// variant in serde's externally tagged form.
struct NestedMap<'a, T: ?Sized>(&'a T);

impl<T: Serialize + ?Sized> Valuable for NestedMap<'_, T> {
    fn as_value(&self) -> Value<'_> {
        Value::Mappable(self)
    }

    fn visit(&self, visit: &mut dyn Visit) {
        let _ = self.0.serialize(VisitSerializer { visit });
    }
}

impl<T: Serialize + ?Sized> Mappable for NestedMap<'_, T> {
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }
}

/// A nested value that serializes as a sequence.
struct NestedSeq<'a, T: ?Sized>(&'a T);

impl<T: Serialize + ?Sized> Valuable for NestedSeq<'_, T> {
    fn as_value(&self) -> Value<'_> {
        Value::Listable(self)
    }

    fn visit(&self, visit: &mut dyn Visit) {
        let _ = self.0.serialize(VisitSerializer { visit });
    }
}

impl<T: Serialize + ?Sized> Listable for NestedSeq<'_, T> {
    fn size_hint(&self) -> (usize, Option<usize>) {
        exact_len(self.0)
    }
}

/// A borrowed JSON value used for the fields of buffered enum variants.
///
/// Walking the value directly is important when serde_json's
/// `arbitrary_precision` feature is enabled. In that configuration,
/// `serde_json::Number` serializes through a private marker struct; feeding a
/// buffered `Value` back through [`VisitSerializer`] would expose that marker
/// in the resulting JSON instead of emitting a number.
struct BufferedJsonValue<'a>(&'a serde_json::Value);

impl Valuable for BufferedJsonValue<'_> {
    fn as_value(&self) -> Value<'_> {
        match self.0 {
            serde_json::Value::Null => Value::Unit,
            serde_json::Value::Bool(value) => Value::Bool(*value),
            serde_json::Value::Number(number) => {
                if let Some(value) = number.as_i64() {
                    Value::I64(value)
                } else if let Some(value) = number.as_u64() {
                    Value::U64(value)
                } else if let Some(value) = number.as_i128() {
                    Value::I128(value)
                } else if let Some(value) = number.as_u128() {
                    Value::U128(value)
                } else if let Some(value) = number.as_f64() {
                    Value::F64(value)
                } else {
                    Value::Unit
                }
            }
            serde_json::Value::String(value) => Value::String(value),
            serde_json::Value::Array(_) => Value::Listable(self),
            serde_json::Value::Object(_) => Value::Mappable(self),
        }
    }

    fn visit(&self, visit: &mut dyn Visit) {
        match self.0 {
            serde_json::Value::Array(items) => {
                for item in items {
                    let item = BufferedJsonValue(item);
                    visit.visit_value(item.as_value());
                }
            }
            serde_json::Value::Object(fields) => {
                for (key, value) in fields {
                    let value = BufferedJsonValue(value);
                    visit.visit_entry(Value::String(key), value.as_value());
                }
            }
            _ => visit.visit_value(self.as_value()),
        }
    }
}

impl Listable for BufferedJsonValue<'_> {
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self.0 {
            serde_json::Value::Array(items) => (items.len(), Some(items.len())),
            _ => (0, Some(0)),
        }
    }
}

impl Mappable for BufferedJsonValue<'_> {
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self.0 {
            serde_json::Value::Object(fields) => (fields.len(), Some(fields.len())),
            _ => (0, Some(0)),
        }
    }
}

/// A byte string, visited as a primitive slice.
struct Bytes<'a>(&'a [u8]);

impl Valuable for Bytes<'_> {
    fn as_value(&self) -> Value<'_> {
        Value::Listable(self)
    }

    fn visit(&self, visit: &mut dyn Visit) {
        visit.visit_primitive_slice(Slice::U8(self.0));
    }
}

impl Listable for Bytes<'_> {
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.0.len(), Some(self.0.len()))
    }
}

/// Format a `Display` value into a stack buffer and hand it to `f`.
///
/// Values that do not fit fall back to a heap string. RFC 3339 timestamps
/// and level names, the usual `collect_str` callers, fit comfortably.
fn with_display_str<R>(value: &dyn Display, f: impl FnOnce(&str) -> R) -> R {
    struct StackBuf<const N: usize> {
        bytes: [u8; N],
        len: usize,
    }

    impl<const N: usize> fmt::Write for StackBuf<N> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            let end = self.len + s.len();
            if end > N {
                return Err(fmt::Error);
            }
            self.bytes[self.len..end].copy_from_slice(s.as_bytes());
            self.len = end;
            Ok(())
        }
    }

    let mut buf = StackBuf::<64> {
        bytes: [0; 64],
        len: 0,
    };
    // Only whole `&str` chunks are copied, so the prefix is valid UTF-8.
    match write!(buf, "{value}") {
        Ok(()) => f(std::str::from_utf8(&buf.bytes[..buf.len]).unwrap_or_default()),
        Err(fmt::Error) => f(&value.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Leaf serializer: one map entry or one sequence element
// ---------------------------------------------------------------------------

/// Serializer for a single entry or element.
///
/// Primitives are visited directly. Containers are handed to the visitor as
/// a borrowed [`NestedMap`] or [`NestedSeq`], and the traversal is stopped
/// with [`Error::Handled`] so the visitor walks the container instead.
struct Leaf<'v, 'd, 'k, T: ?Sized> {
    visit: &'v mut (dyn Visit + 'd),
    /// `Some` for a map entry, `None` for a sequence element.
    key: Option<&'k str>,
    value: &'k T,
}

impl<'v, 'd, 'k, T: ?Sized> Leaf<'v, 'd, 'k, T> {
    fn emit(self, value: Value<'_>) -> Result<(), Error> {
        emit(self.visit, self.key, value);
        Ok(())
    }

    fn descend<U: ?Sized>(self, value: &'k U) -> Leaf<'v, 'd, 'k, U> {
        Leaf {
            visit: self.visit,
            key: self.key,
            value,
        }
    }

    /// Hand the whole value to the visitor as a nested map and stop serde's
    /// traversal of it.
    fn nested_map<X>(self) -> Result<X, Error>
    where
        T: Serialize,
    {
        let Leaf { visit, key, value } = self;
        emit(visit, key, Value::Mappable(&NestedMap(value)));
        Err(Error::Handled)
    }

    /// Hand the whole value to the visitor as a nested sequence and stop
    /// serde's traversal of it.
    fn nested_seq<X>(self) -> Result<X, Error>
    where
        T: Serialize,
    {
        let Leaf { visit, key, value } = self;
        emit(visit, key, Value::Listable(&NestedSeq(value)));
        Err(Error::Handled)
    }
}

fn emit(visit: &mut dyn Visit, key: Option<&str>, value: Value<'_>) {
    match key {
        Some(key) => visit.visit_entry(Value::String(key), value),
        None => visit.visit_value(value),
    }
}

fn emit_entry<T: Serialize + ?Sized>(
    visit: &mut dyn Visit,
    key: &str,
    value: &T,
) -> Result<(), Error> {
    finish_leaf(value.serialize(Leaf {
        visit,
        key: Some(key),
        value,
    }))
}

fn emit_element<T: Serialize + ?Sized>(visit: &mut dyn Visit, value: &T) -> Result<(), Error> {
    finish_leaf(value.serialize(Leaf {
        visit,
        key: None,
        value,
    }))
}

fn finish_leaf(result: Result<(), Error>) -> Result<(), Error> {
    match result {
        Ok(()) | Err(Error::Handled) => Ok(()),
        Err(error) => Err(error),
    }
}

macro_rules! leaf_primitive {
    ($($method:ident: $ty:ty => $variant:ident),* $(,)?) => {
        $(
            fn $method(self, v: $ty) -> Result<(), Error> {
                self.emit(Value::$variant(v))
            }
        )*
    };
}

impl<'k, T: Serialize + ?Sized> Serializer for Leaf<'_, '_, 'k, T> {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = Impossible<(), Error>;
    type SerializeTuple = Impossible<(), Error>;
    type SerializeTupleStruct = Impossible<(), Error>;
    type SerializeTupleVariant = Impossible<(), Error>;
    type SerializeMap = Impossible<(), Error>;
    type SerializeStruct = Impossible<(), Error>;
    type SerializeStructVariant = Impossible<(), Error>;

    leaf_primitive! {
        serialize_bool: bool => Bool,
        serialize_i8: i8 => I8,
        serialize_i16: i16 => I16,
        serialize_i32: i32 => I32,
        serialize_i64: i64 => I64,
        serialize_i128: i128 => I128,
        serialize_u8: u8 => U8,
        serialize_u16: u16 => U16,
        serialize_u32: u32 => U32,
        serialize_u64: u64 => U64,
        serialize_u128: u128 => U128,
        serialize_f32: f32 => F32,
        serialize_f64: f64 => F64,
        serialize_char: char => Char,
        serialize_str: &str => String,
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<(), Error> {
        self.emit(Value::Listable(&Bytes(v)))
    }

    fn collect_str<U: Display + ?Sized>(self, value: &U) -> Result<(), Error> {
        with_display_str(&value, |s| self.emit(Value::String(s)))
    }

    fn serialize_none(self) -> Result<(), Error> {
        self.emit(Value::Unit)
    }

    fn serialize_some<U: Serialize + ?Sized>(self, value: &U) -> Result<(), Error> {
        value.serialize(self.descend(value))
    }

    fn serialize_unit(self) -> Result<(), Error> {
        self.emit(Value::Unit)
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<(), Error> {
        self.emit(Value::Unit)
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<(), Error> {
        self.emit(Value::String(variant))
    }

    fn serialize_newtype_struct<U: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &U,
    ) -> Result<(), Error> {
        value.serialize(self.descend(value))
    }

    fn serialize_newtype_variant<U: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &U,
    ) -> Result<(), Error> {
        self.nested_map()
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        self.nested_seq()
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Error> {
        self.nested_seq()
    }

    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        self.nested_seq()
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        self.nested_map()
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Error> {
        self.nested_map()
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Error> {
        self.nested_map()
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        self.nested_map()
    }
}

// ---------------------------------------------------------------------------
// Container serializer: drives the visitor for one map or sequence
// ---------------------------------------------------------------------------

/// Serializer for the value a visitor is currently walking.
///
/// Entries and elements go through [`Leaf`]. Enum variants take serde's
/// externally tagged form, `{ "Variant": ... }`, which is what `serde_json`
/// produces for enums without a `#[serde(tag)]` attribute.
struct VisitSerializer<'v, 'd> {
    visit: &'v mut (dyn Visit + 'd),
}

macro_rules! visit_primitive {
    ($($method:ident: $ty:ty => $variant:ident),* $(,)?) => {
        $(
            fn $method(self, v: $ty) -> Result<(), Error> {
                self.visit.visit_value(Value::$variant(v));
                Ok(())
            }
        )*
    };
}

impl<'v, 'd> Serializer for VisitSerializer<'v, 'd> {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = Elements<'v, 'd>;
    type SerializeTuple = Elements<'v, 'd>;
    type SerializeTupleStruct = Elements<'v, 'd>;
    type SerializeTupleVariant = BufferedVariant<'v, 'd>;
    type SerializeMap = Entries<'v, 'd>;
    type SerializeStruct = Entries<'v, 'd>;
    type SerializeStructVariant = BufferedVariant<'v, 'd>;

    visit_primitive! {
        serialize_bool: bool => Bool,
        serialize_i8: i8 => I8,
        serialize_i16: i16 => I16,
        serialize_i32: i32 => I32,
        serialize_i64: i64 => I64,
        serialize_i128: i128 => I128,
        serialize_u8: u8 => U8,
        serialize_u16: u16 => U16,
        serialize_u32: u32 => U32,
        serialize_u64: u64 => U64,
        serialize_u128: u128 => U128,
        serialize_f32: f32 => F32,
        serialize_f64: f64 => F64,
        serialize_char: char => Char,
        serialize_str: &str => String,
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<(), Error> {
        self.visit.visit_primitive_slice(Slice::U8(v));
        Ok(())
    }

    fn collect_str<T: Display + ?Sized>(self, value: &T) -> Result<(), Error> {
        with_display_str(&value, |s| self.visit.visit_value(Value::String(s)));
        Ok(())
    }

    fn serialize_none(self) -> Result<(), Error> {
        self.visit.visit_value(Value::Unit);
        Ok(())
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), Error> {
        self.visit.visit_value(Value::Unit);
        Ok(())
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<(), Error> {
        self.visit.visit_value(Value::Unit);
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<(), Error> {
        self.visit.visit_value(Value::String(variant));
        Ok(())
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        emit_entry(self.visit, variant, value)
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        Ok(Elements { visit: self.visit })
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Error> {
        Ok(Elements { visit: self.visit })
    }

    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        Ok(Elements { visit: self.visit })
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        Ok(BufferedVariant {
            visit: self.visit,
            variant,
            buffer: serde_json::Value::Array(Vec::with_capacity(len)),
        })
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Error> {
        Ok(Entries {
            visit: self.visit,
            pending_key: None,
        })
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Error> {
        Ok(Entries {
            visit: self.visit,
            pending_key: None,
        })
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        Ok(BufferedVariant {
            visit: self.visit,
            variant,
            buffer: serde_json::Value::Object(serde_json::Map::new()),
        })
    }
}

/// Sequence elements, visited one at a time.
struct Elements<'v, 'd> {
    visit: &'v mut (dyn Visit + 'd),
}

impl SerializeSeq for Elements<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        emit_element(self.visit, value)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl SerializeTuple for Elements<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        emit_element(self.visit, value)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl SerializeTupleStruct for Elements<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        emit_element(self.visit, value)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

/// Map entries and struct fields, visited one at a time.
struct Entries<'v, 'd> {
    visit: &'v mut (dyn Visit + 'd),
    /// Key buffered by a split `serialize_key` / `serialize_value` pair.
    /// Serde's own impls use `serialize_entry`, which never buffers.
    pending_key: Option<String>,
}

impl SerializeMap for Entries<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        self.pending_key = Some(key.serialize(KeySerializer(|key: &str| Ok(key.to_owned())))?);
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let key = self
            .pending_key
            .take()
            .ok_or_else(|| ser::Error::custom("map value without a key"))?;
        emit_entry(self.visit, &key, value)
    }

    fn serialize_entry<K: Serialize + ?Sized, V: Serialize + ?Sized>(
        &mut self,
        key: &K,
        value: &V,
    ) -> Result<(), Error> {
        let visit = &mut *self.visit;
        key.serialize(KeySerializer(|key: &str| emit_entry(visit, key, value)))
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl SerializeStruct for Entries<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        emit_entry(self.visit, key, value)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

/// Externally tagged struct and tuple variants.
///
/// Serde pushes their fields one at a time, but the visitor needs the whole
/// variant body as one nested value. These variants are buffered. Devenv's
/// own event types use internally tagged enums and never reach this path.
struct BufferedVariant<'v, 'd> {
    visit: &'v mut (dyn Visit + 'd),
    variant: &'static str,
    buffer: serde_json::Value,
}

impl BufferedVariant<'_, '_> {
    fn finish(self) -> Result<(), Error> {
        let buffer = BufferedJsonValue(&self.buffer);
        self.visit
            .visit_entry(Value::String(self.variant), buffer.as_value());
        Ok(())
    }
}

impl SerializeTupleVariant for BufferedVariant<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        if let serde_json::Value::Array(items) = &mut self.buffer {
            items.push(serde_json::to_value(value).map_err(ser::Error::custom)?);
        }
        Ok(())
    }

    fn end(self) -> Result<(), Error> {
        self.finish()
    }
}

impl SerializeStructVariant for BufferedVariant<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        if let serde_json::Value::Object(fields) = &mut self.buffer {
            fields.insert(
                key.to_owned(),
                serde_json::to_value(value).map_err(ser::Error::custom)?,
            );
        }
        Ok(())
    }

    fn end(self) -> Result<(), Error> {
        self.finish()
    }
}

/// Serializer for map keys.
///
/// Keys are handed to the continuation as text, matching `serde_json`, which
/// only accepts keys that render as strings.
struct KeySerializer<F>(F);

macro_rules! key_display {
    ($($method:ident: $ty:ty),* $(,)?) => {
        $(
            fn $method(self, v: $ty) -> Result<R, Error> {
                with_display_str(&v, self.0)
            }
        )*
    };
}

impl<R, F: FnOnce(&str) -> Result<R, Error>> Serializer for KeySerializer<F> {
    type Ok = R;
    type Error = Error;
    type SerializeSeq = Impossible<R, Error>;
    type SerializeTuple = Impossible<R, Error>;
    type SerializeTupleStruct = Impossible<R, Error>;
    type SerializeTupleVariant = Impossible<R, Error>;
    type SerializeMap = Impossible<R, Error>;
    type SerializeStruct = Impossible<R, Error>;
    type SerializeStructVariant = Impossible<R, Error>;

    key_display! {
        serialize_bool: bool,
        serialize_i8: i8,
        serialize_i16: i16,
        serialize_i32: i32,
        serialize_i64: i64,
        serialize_i128: i128,
        serialize_u8: u8,
        serialize_u16: u16,
        serialize_u32: u32,
        serialize_u64: u64,
        serialize_u128: u128,
        serialize_f32: f32,
        serialize_f64: f64,
        serialize_char: char,
    }

    fn serialize_str(self, v: &str) -> Result<R, Error> {
        (self.0)(v)
    }

    fn collect_str<T: Display + ?Sized>(self, value: &T) -> Result<R, Error> {
        with_display_str(&value, self.0)
    }

    fn serialize_bytes(self, _: &[u8]) -> Result<R, Error> {
        Err(key_error())
    }

    fn serialize_none(self) -> Result<R, Error> {
        Err(key_error())
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<R, Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<R, Error> {
        Err(key_error())
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<R, Error> {
        Err(key_error())
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<R, Error> {
        (self.0)(variant)
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<R, Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<R, Error> {
        Err(key_error())
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        Err(key_error())
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Error> {
        Err(key_error())
    }

    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        Err(key_error())
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        Err(key_error())
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Error> {
        Err(key_error())
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Error> {
        Err(key_error())
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        Err(key_error())
    }
}

fn key_error() -> Error {
    ser::Error::custom("map key must render as a string")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::SystemTime;

    use serde::Serialize;
    use serde_json::json;
    use valuable_serde::Serializable;

    use super::*;
    use crate::events::{
        ActivityEvent, ActivityLevel, ActivityOutcome, Build, EvalOp, Evaluate, Fetch, FetchKind,
        HttpProbe, Message, Process, ProcessStatus, ReadyProbe, Shell, Task, TaskInfo,
    };
    use crate::{PortBinding, Timestamp};

    /// Serialize through the visitor and through serde_json directly.
    fn via_visitor<T: Serialize>(value: &T) -> serde_json::Value {
        serde_json::to_value(Serializable::new(SerdeValuable(value).as_value())).unwrap()
    }

    fn assert_same_json<T: Serialize>(value: &T) {
        let expected = serde_json::to_value(value).unwrap();
        assert_eq!(via_visitor(value), expected);
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Everything {
        unsigned: u64,
        signed: i32,
        wide: u128,
        float: f64,
        flag: bool,
        letter: char,
        text: String,
        present: Option<u8>,
        absent: Option<u8>,
        #[serde(skip_serializing_if = "Option::is_none")]
        skipped: Option<u8>,
        unit: (),
        newtype: Meters,
        pair: (u8, String),
        list: Vec<Inner>,
        string_map: BTreeMap<String, u32>,
        numeric_map: BTreeMap<u32, String>,
        external: External,
        internal: Internal,
        adjacent: Adjacent,
        untagged: Untagged,
        bytes: Vec<u8>,
        timestamp: Timestamp,
        level: ActivityLevel,
        path: std::path::PathBuf,
    }

    #[derive(Serialize)]
    struct Meters(u32);

    #[derive(Serialize)]
    struct Inner {
        name: String,
        nested: Vec<Vec<u8>>,
    }

    #[derive(Serialize)]
    enum External {
        Unit,
        Newtype(Inner),
        Tuple(u8, bool),
        Struct { a: u8, b: Vec<String> },
    }

    #[derive(Serialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum Internal {
        Alpha { value: u8 },
        Beta(Inner),
    }

    #[derive(Serialize)]
    #[serde(tag = "t", content = "c")]
    enum Adjacent {
        One(u8),
    }

    #[derive(Serialize)]
    #[serde(untagged)]
    enum Untagged {
        Text(String),
    }

    fn everything() -> Everything {
        Everything {
            unsigned: 42,
            signed: -7,
            // `serde_json::Value` only holds numbers up to 64 bits.
            wide: 1 << 40,
            float: 1.5,
            flag: true,
            letter: 'x',
            text: "hello".into(),
            present: Some(1),
            absent: None,
            skipped: None,
            unit: (),
            newtype: Meters(12),
            pair: (3, "three".into()),
            list: vec![
                Inner {
                    name: "a".into(),
                    nested: vec![vec![1, 2], vec![]],
                },
                Inner {
                    name: "b".into(),
                    nested: vec![],
                },
            ],
            string_map: BTreeMap::from([("k1".into(), 1), ("k2".into(), 2)]),
            numeric_map: BTreeMap::from([(10, "ten".into()), (20, "twenty".into())]),
            external: External::Struct {
                a: 1,
                b: vec!["x".into()],
            },
            internal: Internal::Beta(Inner {
                name: "beta".into(),
                nested: vec![vec![9]],
            }),
            adjacent: Adjacent::One(5),
            untagged: Untagged::Text("plain".into()),
            bytes: vec![0, 255],
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
            level: ActivityLevel::Warn,
            path: "/nix/store/abc-foo".into(),
        }
    }

    #[test]
    fn matches_serde_json_for_every_serde_shape() {
        assert_same_json(&everything());
    }

    #[test]
    fn matches_serde_json_for_internal_struct_variants() {
        #[derive(Serialize)]
        struct Wrapper {
            internal: Internal,
        }
        assert_same_json(&Wrapper {
            internal: Internal::Alpha { value: 3 },
        });
    }

    #[test]
    fn matches_serde_json_for_external_variants() {
        for external in [
            External::Unit,
            External::Newtype(Inner {
                name: "n".into(),
                nested: vec![vec![1]],
            }),
            External::Tuple(1, false),
            External::Struct { a: 2, b: vec![] },
        ] {
            #[derive(Serialize)]
            struct Wrapper {
                external: External,
            }
            assert_same_json(&Wrapper { external });
        }
    }

    #[test]
    fn matches_serde_json_for_activity_events() {
        let timestamp = Timestamp(SystemTime::UNIX_EPOCH);
        let events = vec![
            ActivityEvent::Build(Build::Start {
                id: 1,
                name: "pkg".into(),
                parent: Some(7),
                derivation_path: Some("/nix/store/abc-pkg.drv".into()),
                timestamp,
            }),
            ActivityEvent::Build(Build::Log {
                id: 1,
                line: "compiling".into(),
                is_error: false,
                timestamp,
            }),
            ActivityEvent::Fetch(Fetch::Start {
                id: 2,
                kind: FetchKind::Download,
                name: "pkg".into(),
                parent: None,
                url: None,
                timestamp,
            }),
            ActivityEvent::Fetch(Fetch::Progress {
                id: 2,
                current: 10,
                total: None,
                timestamp,
            }),
            ActivityEvent::Evaluate(Evaluate::Op {
                id: 3,
                op: EvalOp::HashFile {
                    source: "/p/file".into(),
                    algorithm: "sha256".into(),
                },
                timestamp,
            }),
            ActivityEvent::Task(Task::Hierarchy {
                tasks: vec![TaskInfo {
                    id: 4,
                    name: "t".into(),
                    show_output: true,
                    is_process: false,
                }],
                edges: vec![(4, 5)],
                timestamp,
            }),
            ActivityEvent::Process(Process::Start {
                id: 5,
                name: "web".into(),
                parent: None,
                command: Some("serve".into()),
                ports: vec![PortBinding {
                    name: "http".into(),
                    port: 8080,
                }],
                urls: vec!["http://web.demo.localhost".into()],
                ready_probe: Some(ReadyProbe::Http(Box::new(HttpProbe {
                    host: "localhost".into(),
                    port: 8080,
                    path: "/health".into(),
                }))),
                level: ActivityLevel::Info,
                timestamp,
            }),
            ActivityEvent::Process(Process::Status {
                id: 5,
                status: ProcessStatus::GaveUp,
                timestamp,
            }),
            ActivityEvent::Process(Process::Exited {
                id: 5,
                success: false,
                timestamp,
            }),
            ActivityEvent::Message(Message {
                id: 6,
                level: ActivityLevel::Error,
                text: "boom".into(),
                details: Some("trace".into()),
                parent: None,
                timestamp,
            }),
            ActivityEvent::Shell(Shell::Output {
                id: 7,
                data: vec![27, 91, 109],
                timestamp,
            }),
            ActivityEvent::Operation(crate::Operation::Complete {
                id: 8,
                outcome: ActivityOutcome::DependencyFailed,
                timestamp,
            }),
        ];
        for event in &events {
            assert_same_json(event);
        }
    }

    #[test]
    fn keeps_serde_shape_when_replayed_through_serde() {
        let event = ActivityEvent::Build(Build::Phase {
            id: 9,
            phase: "configure".into(),
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
        });
        let json = via_visitor(&event);
        assert_eq!(json["activity_kind"], "build");
        assert_eq!(json["event"], "phase");
        assert_eq!(json["timestamp"], "1970-01-01T00:00:00.000000000Z");
        let parsed: ActivityEvent = serde_json::from_value(json).unwrap();
        assert!(matches!(
            parsed,
            ActivityEvent::Build(Build::Phase { id: 9, phase, .. }) if phase == "configure"
        ));
    }

    #[test]
    fn top_level_sequence_and_primitive() {
        assert_eq!(via_visitor(&vec![1u8, 2, 3]), json!([1, 2, 3]));
        assert_eq!(via_visitor(&7u8), serde_json::Value::Null);
    }

    /// `serde_json::to_string` honours sequence length hints, unlike
    /// `serde_json::to_value`.
    #[test]
    fn sequences_serialize_to_valid_json_text() {
        #[derive(Serialize)]
        struct Item {
            id: u64,
        }
        #[derive(Serialize)]
        struct Lists {
            items: Vec<Item>,
            empty: Vec<Item>,
            pairs: Vec<(u64, u64)>,
        }
        let value = Lists {
            items: vec![Item { id: 1 }, Item { id: 2 }],
            empty: vec![],
            pairs: vec![(1, 2)],
        };
        let text =
            serde_json::to_string(&Serializable::new(SerdeValuable(&value).as_value())).unwrap();
        assert_eq!(text, serde_json::to_string(&value).unwrap());

        let event = ActivityEvent::Task(Task::Hierarchy {
            tasks: vec![
                TaskInfo {
                    id: 1,
                    name: "devenv:files".into(),
                    show_output: false,
                    is_process: false,
                },
                TaskInfo {
                    id: 2,
                    name: "devenv:enterShell".into(),
                    show_output: true,
                    is_process: false,
                },
            ],
            edges: vec![(2, 1)],
            timestamp: Timestamp(SystemTime::UNIX_EPOCH),
        });
        let text =
            serde_json::to_string(&Serializable::new(SerdeValuable(&event).as_value())).unwrap();
        assert_eq!(text, serde_json::to_string(&event).unwrap());
    }

    #[test]
    fn long_display_values_fall_back_to_the_heap() {
        #[derive(Serialize)]
        struct Long {
            #[serde(serialize_with = "long")]
            value: (),
        }
        fn long<S: Serializer>(_: &(), serializer: S) -> Result<S::Ok, S::Error> {
            serializer.collect_str(&"x".repeat(500))
        }
        assert_eq!(
            via_visitor(&Long { value: () }),
            json!({ "value": "x".repeat(500) })
        );
    }
}
