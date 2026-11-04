use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Typed values used by contracts, state, actions, and interfaces.
///
/// Phase 1 deliberately excludes arbitrary callbacks and binary blobs
/// inside expressions. Binary content belongs in evidence storage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TypedValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    BytesHex(String),
    List(Vec<TypedValue>),
    Map(IndexMap<String, TypedValue>),
}

impl TypedValue {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::String(_) => "string",
            Self::BytesHex(_) => "bytes",
            Self::List(_) => "list",
            Self::Map(_) => "map",
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(v) => Some(v),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

impl fmt::Display for TypedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => write!(f, "null"),
            Self::Bool(v) => write!(f, "{v}"),
            Self::Int(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::String(v) => write!(f, "{v:?}"),
            Self::BytesHex(v) => write!(f, "bytes:{v}"),
            Self::List(v) => write!(f, "list[{}]", v.len()),
            Self::Map(v) => write!(f, "map{{{}}}", v.len()),
        }
    }
}

/// Declarative type descriptors for interfaces and state schemas.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeDesc {
    Null,
    Bool,
    Int,
    Float,
    String,
    Bytes,
    List { item: Box<TypeDesc> },
    Map { value: Box<TypeDesc> },
    Object { fields: IndexMap<String, FieldDesc> },
    Enum { variants: Vec<String> },
    Optional { inner: Box<TypeDesc> },
    Union { variants: Vec<TypeDesc> },
    Ref { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDesc {
    pub ty: TypeDesc,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl TypeDesc {
    /// Structural assignability for Phase 1 (no subtyping beyond Optional/Union).
    pub fn is_compatible_with(&self, required: &TypeDesc) -> bool {
        use TypeDesc::*;
        match (self, required) {
            (a, b) if a == b => true,
            (_, Optional { inner }) => self.is_compatible_with(inner) || matches!(self, Null),
            (Optional { inner }, required) => inner.is_compatible_with(required),
            (Union { variants }, required) => {
                variants.iter().all(|v| v.is_compatible_with(required))
            }
            (provided, Union { variants }) => {
                variants.iter().any(|v| provided.is_compatible_with(v))
            }
            (List { item: a }, List { item: b }) => a.is_compatible_with(b),
            (Map { value: a }, Map { value: b }) => a.is_compatible_with(b),
            (Object { fields: a }, Object { fields: b }) => b.iter().all(|(name, req_field)| {
                if let Some(prov) = a.get(name) {
                    prov.ty.is_compatible_with(&req_field.ty)
                } else {
                    !req_field.required
                }
            }),
            (Enum { variants: a }, Enum { variants: b }) => b.iter().all(|v| a.contains(v)),
            (Ref { name: a }, Ref { name: b }) => a == b,
            (Null, Null)
            | (Bool, Bool)
            | (Int, Int)
            | (Float, Float)
            | (String, String)
            | (Bytes, Bytes) => true,
            // Int may satisfy Float requirements at the interface level.
            (Int, Float) => true,
            _ => false,
        }
    }
}

/// Uncertainty classification for behavioral claims and transitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Certainty {
    /// Directly observed under a recorded environment.
    Observed,
    /// Asserted by a trusted or user-provided specification.
    Specified,
    /// Inferred from incomplete evidence.
    Inferred,
    /// Explicitly hypothetical / candidate.
    Hypothetical,
    /// Known unknown — default when certainty is unspecified.
    #[default]
    Unknown,
    /// Competing evidence exists.
    Contradictory,
    /// Verified under a specified test environment (not universal proof).
    VerifiedUnderTest,
}

impl Certainty {
    pub fn is_definitive(self) -> bool {
        matches!(
            self,
            Self::Observed | Self::Specified | Self::VerifiedUnderTest
        )
    }
}
