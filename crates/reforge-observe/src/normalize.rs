//! Evidence normalization.
//!
//! Normalization exists so that two observations of the same behavior compare
//! equal. It is restricted on purpose:
//!
//! * Only **declared** volatile rules may rewrite content.
//! * Structurally meaningful fields (`outcome`, `error`, `kind`, `status`)
//!   are never rewritten, so a failure can never be normalized into a success.
//! * Every rewrite is recorded in a [`NormalizationRecord`] that travels with
//!   the observation bundle, so a reviewer can tell normalized from raw.
//!
//! Anything not matched by an explicit rule is left byte-identical.

use indexmap::IndexMap;
use reforge_types::TypedValue;
use serde::{Deserialize, Serialize};

/// Fields whose values carry behavioral meaning and are therefore immutable
/// under normalization.
pub const PROTECTED_KEYS: &[&str] = &[
    "outcome",
    "error",
    "errors",
    "kind",
    "status",
    "result",
    "exit_code",
    "count",
    "state",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VolatileRule {
    /// Replace UUID-shaped substrings with `<uuid>`.
    UuidLike,
    /// Replace ISO-8601 timestamps with `<timestamp>`.
    TimestampLike,
    /// Replace long decimal runs with `<digits>`.
    DigitsRun,
    /// Replace values of the named keys with `<redacted>`.
    KeyNames { keys: Vec<String> },
}

impl VolatileRule {
    pub fn id(&self) -> String {
        match self {
            VolatileRule::UuidLike => "uuid_like".into(),
            VolatileRule::TimestampLike => "timestamp_like".into(),
            VolatileRule::DigitsRun => "digits_run".into(),
            VolatileRule::KeyNames { keys } => format!("key_names:{}", keys.join(",")),
        }
    }
}

/// Audit trail of what normalization did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NormalizationRecord {
    #[serde(default)]
    pub rules_applied: Vec<String>,
    #[serde(default)]
    pub fields_rewritten: Vec<String>,
    #[serde(default)]
    pub protected_fields_skipped: Vec<String>,
    #[serde(default)]
    pub note: String,
}

impl NormalizationRecord {
    pub fn untouched() -> Self {
        Self {
            note: "no normalization applied".into(),
            ..Self::default()
        }
    }

    pub fn applied(&self) -> bool {
        !self.rules_applied.is_empty()
    }
}

/// Deterministic normalizer with an explicit, auditable rule set.
#[derive(Clone, Debug, Default)]
pub struct Normalizer {
    rules: Vec<VolatileRule>,
}

impl Normalizer {
    pub fn new(rules: Vec<VolatileRule>) -> Self {
        Self { rules }
    }

    /// Default rules for black-box investigation: ids, timestamps, digit runs.
    ///
    /// Integers appearing under protected keys are *not* affected because
    /// only string values are rewritten.
    pub fn black_box_default() -> Self {
        Self::new(vec![
            VolatileRule::UuidLike,
            VolatileRule::TimestampLike,
            VolatileRule::DigitsRun,
        ])
    }

    pub fn rules(&self) -> &[VolatileRule] {
        &self.rules
    }

    /// Normalize a value map, refusing to rewrite protected keys.
    pub fn normalize_map(
        &self,
        input: &IndexMap<String, TypedValue>,
    ) -> (IndexMap<String, TypedValue>, NormalizationRecord) {
        let mut record = NormalizationRecord::default();
        let mut out = IndexMap::new();
        for (key, value) in input {
            let (normalized, rewritten) = if is_protected(key) {
                if contains_volatile(value) {
                    record.protected_fields_skipped.push(key.clone());
                }
                (value.clone(), false)
            } else {
                self.normalize_value(value, key, &mut record)
            };
            if rewritten && !record.fields_rewritten.contains(&key.to_string()) {
                record.fields_rewritten.push(key.clone());
            }
            out.insert(key.clone(), normalized);
        }
        if record.rules_applied.is_empty() && record.fields_rewritten.is_empty() {
            record.note = "no volatile content found".into();
        } else {
            record.note = format!(
                "rewrote {} field(s) using {} rule(s)",
                record.fields_rewritten.len(),
                record.rules_applied.len()
            );
        }
        (out, record)
    }

    fn normalize_value(
        &self,
        value: &TypedValue,
        key: &str,
        record: &mut NormalizationRecord,
    ) -> (TypedValue, bool) {
        match value {
            TypedValue::String(s) => {
                let mut out = s.clone();
                for rule in &self.rules {
                    let next = apply_string_rule(rule, &out, key);
                    if next != out {
                        push_unique(&mut record.rules_applied, rule.id());
                        out = next;
                    }
                }
                (TypedValue::String(out.clone()), out != *s)
            }
            TypedValue::List(items) => {
                let mut rewritten = false;
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    let (v, changed) = self.normalize_value(item, key, record);
                    rewritten |= changed;
                    out.push(v);
                }
                (TypedValue::List(out), rewritten)
            }
            TypedValue::Map(map) => {
                let (out, rec) = self.normalize_map(map);
                let rewritten = rec.applied() || !rec.fields_rewritten.is_empty();
                record.rules_applied.extend(rec.rules_applied);
                record.fields_rewritten.extend(rec.fields_rewritten);
                record
                    .protected_fields_skipped
                    .extend(rec.protected_fields_skipped);
                (TypedValue::Map(out), rewritten)
            }
            other => (other.clone(), false),
        }
    }

    /// Normalize free text (visible text, log lines) using the same rules.
    pub fn normalize_text(&self, text: &str) -> (String, NormalizationRecord) {
        let mut record = NormalizationRecord::default();
        let mut out = text.to_string();
        for rule in &self.rules {
            let next = apply_string_rule(rule, &out, "");
            if next != out {
                push_unique(&mut record.rules_applied, rule.id());
                out = next;
            }
        }
        record.note = if record.applied() {
            format!(
                "normalized text with {} rule(s)",
                record.rules_applied.len()
            )
        } else {
            "no volatile content found".into()
        };
        (out, record)
    }
}

fn is_protected(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    PROTECTED_KEYS.iter().any(|p| lower == *p)
}

fn contains_volatile(value: &TypedValue) -> bool {
    match value {
        TypedValue::String(s) => s.chars().any(|c| c.is_ascii_digit()),
        TypedValue::List(items) => items.iter().any(contains_volatile),
        TypedValue::Map(map) => map.values().any(contains_volatile),
        _ => false,
    }
}

fn push_unique(vec: &mut Vec<String>, item: String) {
    if !vec.contains(&item) {
        vec.push(item);
    }
}

fn apply_string_rule(rule: &VolatileRule, input: &str, key: &str) -> String {
    match rule {
        VolatileRule::UuidLike => replace_uuid(input),
        VolatileRule::TimestampLike => replace_timestamps(input),
        VolatileRule::DigitsRun => replace_digits(input),
        VolatileRule::KeyNames { keys } => {
            if keys.iter().any(|k| k.eq_ignore_ascii_case(key)) && !input.is_empty() {
                "<redacted>".to_string()
            } else {
                input.to_string()
            }
        }
    }
}

/// Replace canonical 8-4-4-4-12 hex UUIDs.
fn replace_uuid(input: &str) -> String {
    let bytes: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if let Some(len) = uuid_len_at(&bytes, i) {
            out.push_str("<uuid>");
            i += len;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn uuid_len_at(b: &[char], start: usize) -> Option<usize> {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    let mut i = start;
    for (gi, group) in GROUPS.iter().enumerate() {
        if gi > 0 {
            if b.get(i) != Some(&'-') {
                return None;
            }
            i += 1;
        }
        for _ in 0..*group {
            match b.get(i) {
                Some(c) if c.is_ascii_hexdigit() => i += 1,
                _ => return None,
            }
        }
    }
    Some(i - start)
}

/// Replace `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM:SS(.sss)?(Z|±HH:MM)?` and `HH:MM:SS`.
fn replace_timestamps(input: &str) -> String {
    let bytes: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if let Some(len) = date_len_at(&bytes, i) {
            out.push_str("<timestamp>");
            i += len;
        } else if let Some(len) = clock_len_at(&bytes, i) {
            out.push_str("<timestamp>");
            i += len;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn date_len_at(b: &[char], start: usize) -> Option<usize> {
    let digits = |from: usize, n: usize| -> bool {
        (0..n).all(|k| matches!(b.get(from + k), Some(c) if c.is_ascii_digit()))
    };
    if !digits(start, 4) || b.get(start + 4) != Some(&'-') || !digits(start + 5, 2) {
        return None;
    }
    if b.get(start + 7) != Some(&'-') || !digits(start + 8, 2) {
        return None;
    }
    let mut i = start + 10;
    // Optional time part.
    if b.get(i) == Some(&'T') || b.get(i) == Some(&' ') {
        let mut j = i + 1;
        if digits(j, 2) && b.get(j + 2) == Some(&':') && digits(j + 3, 2) {
            j += 5;
            if b.get(j) == Some(&':') && digits(j + 1, 2) {
                j += 3;
                if b.get(j) == Some(&'.') {
                    let mut k = j + 1;
                    while matches!(b.get(k), Some(c) if c.is_ascii_digit()) {
                        k += 1;
                    }
                    if k > j + 1 {
                        j = k;
                    }
                }
                if matches!(b.get(j), Some('Z') | Some('z')) {
                    j += 1;
                } else if matches!(b.get(j), Some('+') | Some('-'))
                    && digits(j + 1, 2)
                    && b.get(j + 3) == Some(&':')
                    && digits(j + 4, 2)
                {
                    j += 6;
                }
                i = j;
            }
        }
    }
    Some(i - start)
}

fn clock_len_at(b: &[char], start: usize) -> Option<usize> {
    let digits = |from: usize, n: usize| -> bool {
        (0..n).all(|k| matches!(b.get(from + k), Some(c) if c.is_ascii_digit()))
    };
    if digits(start, 2)
        && b.get(start + 2) == Some(&':')
        && digits(start + 3, 2)
        && b.get(start + 5) == Some(&':')
        && digits(start + 6, 2)
    {
        return Some(8);
    }
    None
}

/// Replace runs of 4+ digits with `<digits>` (id suffixes, embedded counters).
fn replace_digits(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut run = String::new();
    for c in input.chars() {
        if c.is_ascii_digit() {
            run.push(c);
        } else {
            flush_digit_run(&mut run, &mut out);
            out.push(c);
        }
    }
    flush_digit_run(&mut run, &mut out);
    out
}

fn flush_digit_run(run: &mut String, out: &mut String) {
    if run.len() >= 4 {
        out.push_str("<digits>");
    } else {
        out.push_str(run);
    }
    run.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_are_replaced() {
        let n = Normalizer::black_box_default();
        let (out, rec) = n.normalize_text("task 3f2504e0-4f89-11d3-9a0c-0305e82c3301 created");
        assert_eq!(out, "task <uuid> created");
        assert!(rec.applied());
    }

    #[test]
    fn timestamps_are_replaced() {
        let n = Normalizer::black_box_default();
        let (out, _) = n.normalize_text("created 2026-11-04T12:30:01.123Z by user");
        assert_eq!(out, "created <timestamp> by user");
        let (out2, _) = n.normalize_text("at 2026-11-04");
        assert_eq!(out2, "at <timestamp>");
    }

    #[test]
    fn protected_fields_never_rewritten() {
        let n = Normalizer::black_box_default();
        let mut map = IndexMap::new();
        map.insert("outcome".to_string(), TypedValue::String("rejected".into()));
        map.insert(
            "error".to_string(),
            TypedValue::String("task 1234 not found on 2026-11-04".into()),
        );
        let (out, rec) = n.normalize_map(&map);
        assert_eq!(
            out.get("error").and_then(|v| v.as_str()),
            Some("task 1234 not found on 2026-11-04")
        );
        assert!(rec.protected_fields_skipped.contains(&"error".to_string()));
    }

    #[test]
    fn short_numbers_survive() {
        let n = Normalizer::black_box_default();
        let (out, _) = n.normalize_text("count=42 total=7");
        assert_eq!(out, "count=42 total=7");
    }

    #[test]
    fn key_rules_redact_declared_keys() {
        let n = Normalizer::new(vec![VolatileRule::KeyNames {
            keys: vec!["session_token".into()],
        }]);
        let mut map = IndexMap::new();
        map.insert(
            "session_token".to_string(),
            TypedValue::String("abc".into()),
        );
        let (out, rec) = n.normalize_map(&map);
        assert_eq!(
            out.get("session_token").and_then(|v| v.as_str()),
            Some("<redacted>")
        );
        assert!(rec.applied());
    }
}
