//! Stable projection of observable outputs for differential comparison.
//!
//! This is intentionally narrower than "serialize the value". Entity IDs,
//! allocator-selected fields and implementation counters are not behaviour.
//! List payloads are comparable only when every element is a scalar (for
//! example `available_actions`); lists of objects are reported as unsupported
//! instead of being compared by volatile contents.

use reforge_types::TypedValue;

/// Whether an output field is part of the stable comparison contract.
///
/// Identity fields are excluded by name as well as by the explicit legacy list.
/// `available_actions` is deliberately included: it is an observed, state-
/// dependent output and ignoring it was hiding real differences.
pub fn is_comparable_output_name(name: &str) -> bool {
    if matches!(
        name,
        "events"
            | "error"
            | "outcome"
            | "created_id"
            | "created_pool"
            | "selected"
            | "rejects"
            | "attempts"
            | "last_created_id"
    ) {
        return false;
    }
    if name == "ids" || name.ends_with("_id") || name.ends_with("_ids") {
        return false;
    }
    true
}

fn list_element(value: &TypedValue) -> Option<String> {
    match value {
        // Quote strings inside a list so a comma in an action name cannot
        // collide with the list separator.
        TypedValue::String(value) => Some(serde_json::to_string(value).ok()?),
        other => scalar(other),
    }
}

fn scalar(value: &TypedValue) -> Option<String> {
    match value {
        TypedValue::Null => Some("null".into()),
        TypedValue::Bool(value) => Some(value.to_string()),
        TypedValue::Int(value) => Some(value.to_string()),
        TypedValue::Float(value) => Some(value.to_string()),
        // Idempotence matters: extraction stores the canonical text in a twin
        // output and the differential harness may canonicalize that string
        // again. A scalar string is therefore its own canonical form.
        TypedValue::String(value) => Some(value.clone()),
        TypedValue::BytesHex(value) => Some(format!("bytes:{value}")),
        TypedValue::List(_) | TypedValue::Map(_) => None,
    }
}

/// Canonical text for a supported structured output, or `None` when the value
/// contains unsupported nesting. `None` is reported as unsupported, never as a
/// match.
pub fn stable_output_text(value: &TypedValue) -> Option<String> {
    if let Some(text) = scalar(value) {
        return Some(text);
    }
    match value {
        TypedValue::List(items) => {
            let mut parts = Vec::with_capacity(items.len());
            for item in items {
                parts.push(list_element(item)?);
            }
            Some(format!("list[{}]", parts.join(",")))
        }
        TypedValue::Map(map) => {
            let mut parts = Vec::with_capacity(map.len());
            for (key, value) in map {
                parts.push(format!(
                    "{}:{}",
                    serde_json::to_string(key).ok()?,
                    scalar(value)?
                ));
            }
            parts.sort();
            Some(format!("map{{{}}}", parts.join(",")))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    #[test]
    fn scalar_lists_are_stable_and_ordered() {
        let value = TypedValue::List(vec![
            TypedValue::String("add".into()),
            TypedValue::String("list".into()),
        ]);
        let canonical = stable_output_text(&value).unwrap();
        assert_eq!(canonical, "list[\"add\",\"list\"]");
        // Canonical strings are idempotent under repeated projection.
        assert_eq!(
            stable_output_text(&TypedValue::String(canonical.clone())).as_deref(),
            Some(canonical.as_str())
        );
    }

    #[test]
    fn object_payloads_are_not_compared_by_accident() {
        let value = TypedValue::List(vec![TypedValue::Map(IndexMap::from([(
            "id".into(),
            TypedValue::String("t1".into()),
        )]))]);
        assert_eq!(stable_output_text(&value), None);
    }

    #[test]
    fn identity_fields_are_excluded_but_capabilities_are_not() {
        assert!(!is_comparable_output_name("created_id"));
        assert!(!is_comparable_output_name("purged_ids"));
        assert!(!is_comparable_output_name("events"));
        assert!(is_comparable_output_name("available_actions"));
        assert!(is_comparable_output_name("queued"));
    }
}
