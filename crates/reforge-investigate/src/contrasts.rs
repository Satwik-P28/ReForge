//! State-diverse contrasts derived from typed candidate differences.
//!
//! The scheduler can only separate hypotheses that some reachable state makes
//! them disagree on. This module turns the *kind* of difference between two
//! candidate programs into a bounded action sequence that reaches a state where
//! the difference is observable:
//!
//! - a candidate that treats an action as a no-op needs the action exercised
//!   from a state where its effect would be visible;
//! - a candidate that replaces an ordered field instead of extending it needs a
//!   pre-existing value in that field before the action runs;
//! - a candidate that treats integer identities as compact positions needs a
//!   create/delete/create cycle that reveals whether a locator is reused.
//!
//! Nothing here names a target application, an action, or an expected answer.
//! Sequences are built from the shapes of the competing programs and from the
//! actions that create and remove records in the affected collection.

use serde::{Deserialize, Serialize};

use crate::data::{
    CollectionLayout, DataProgram, Effect, FieldUpdate, IdentityStrategy, ProgramCandidate,
    RuleBranch,
};

/// The structural difference a contrast is meant to expose.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContrastKind {
    /// The candidate removes every effect of an action that the base program
    /// says mutates state.
    StateNoOp { action: String },
    /// The candidate writes the whole array where the base program appends to
    /// it, or the reverse.
    AppendVsReplace { action: String, field: String },
    /// The candidate treats integer locators as display positions rather than
    /// stable identities.
    IdentityCompaction { collection: String },
}

/// A bounded, executable contrast. `actions` is the full sequence executed
/// after a reset; the last action is the discriminating one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContrastExperiment {
    pub id: String,
    pub kind: ContrastKind,
    pub actions: Vec<String>,
    pub candidate_ids: Vec<String>,
    pub reason: String,
}

impl ContrastExperiment {
    pub fn setup(&self) -> &[String] {
        let split = self.actions.len().saturating_sub(1);
        &self.actions[..split]
    }

    pub fn target(&self) -> Option<&str> {
        self.actions.last().map(String::as_str)
    }
}

fn creators(program: &DataProgram, collection: &str) -> Vec<String> {
    let mut out = Vec::new();
    for rule in &program.rules {
        if rule.branch != RuleBranch::Success {
            continue;
        }
        let creates = rule.effects.iter().any(|effect| match effect {
            Effect::CreateEntity {
                collection: name, ..
            } => name == collection,
            _ => false,
        });
        if creates && !out.contains(&rule.action) {
            out.push(rule.action.clone());
        }
    }
    out.sort();
    out
}

fn removers(program: &DataProgram, collection: &str) -> Vec<String> {
    let mut out = Vec::new();
    for rule in &program.rules {
        if rule.branch != RuleBranch::Success {
            continue;
        }
        let removes = rule.effects.iter().any(|effect| match effect {
            Effect::DeleteEntity {
                collection: name, ..
            }
            | Effect::DeleteWhere {
                collection: name, ..
            } => name == collection,
            _ => false,
        });
        if removes && !out.contains(&rule.action) {
            out.push(rule.action.clone());
        }
    }
    out.sort();
    out
}

/// Collections an action's successful effects touch.
fn affected_collections(program: &DataProgram, action: &str) -> Vec<String> {
    let mut out = Vec::new();
    for rule in &program.rules {
        if rule.branch != RuleBranch::Success || rule.action != action {
            continue;
        }
        for effect in &rule.effects {
            let name = match effect {
                Effect::CreateEntity { collection, .. }
                | Effect::DeleteEntity { collection, .. }
                | Effect::UpdateEntity { collection, .. }
                | Effect::UpdateWhere { collection, .. }
                | Effect::DeleteWhere { collection, .. }
                | Effect::ClearCollection { collection } => Some(collection.clone()),
                _ => None,
            };
            if let Some(name) = name {
                if !out.contains(&name) {
                    out.push(name);
                }
            }
        }
    }
    out.sort();
    out
}

fn ordered_fields(program: &DataProgram) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for rule in &program.rules {
        for effect in &rule.effects {
            if let Effect::UpdateEntity { fields, .. } | Effect::UpdateWhere { fields, .. } = effect
            {
                for update in fields {
                    let field = match update {
                        FieldUpdate::AppendItem { field, .. }
                        | FieldUpdate::Append { field, .. }
                        | FieldUpdate::Set { field, .. } => field.clone(),
                        _ => continue,
                    };
                    let entry = (rule.action.clone(), field);
                    if !out.contains(&entry) {
                        out.push(entry);
                    }
                }
            }
        }
    }
    out.sort();
    out
}

/// Build the state-diverse contrasts implied by one candidate's differences
/// from the selected program. The result is deterministic and bounded.
pub fn candidate_contrasts(
    base: &DataProgram,
    candidate: &ProgramCandidate,
    limit: usize,
) -> Vec<ContrastExperiment> {
    let mut out: Vec<ContrastExperiment> = Vec::new();
    if candidate.id == "base" {
        return out;
    }
    let program = &candidate.program;

    for base_rule in &base.rules {
        if base_rule.branch != RuleBranch::Success {
            continue;
        }
        let Some(other) = program
            .rules
            .iter()
            .find(|rule| rule.id == base_rule.id && rule.branch == RuleBranch::Success)
        else {
            continue;
        };
        // A candidate that drops every effect claims the action is inert. The
        // discriminating state is one where the base rule's effect would show.
        if !base_rule.effects.is_empty() && other.effects.is_empty() {
            let collections = affected_collections(base, &base_rule.action);
            let mut actions: Vec<String> = Vec::new();
            if let Some(collection) = collections.first() {
                if let Some(creator) = creators(base, collection).first() {
                    if creator != &base_rule.action {
                        actions.push(creator.clone());
                    }
                }
            }
            actions.push(base_rule.action.clone());
            out.push(ContrastExperiment {
                id: format!("contrast:noop:{}:{}", base_rule.action, actions.join(">")),
                kind: ContrastKind::StateNoOp {
                    action: base_rule.action.clone(),
                },
                actions,
                candidate_ids: vec![candidate.id.clone()],
                reason: "distinguish a state-changing action from a no-op reading".into(),
            });
        }
        // A candidate that writes an array a base program extends (or the
        // reverse) only differs once the field already holds a value.
        for (effect_index, effect) in base_rule.effects.iter().enumerate() {
            let (Effect::UpdateEntity { fields, .. } | Effect::UpdateWhere { fields, .. }) = effect
            else {
                continue;
            };
            let Some(other_effect) = other.effects.get(effect_index) else {
                continue;
            };
            let (Effect::UpdateEntity {
                fields: other_fields,
                ..
            }
            | Effect::UpdateWhere {
                fields: other_fields,
                ..
            }) = other_effect
            else {
                continue;
            };
            for (field_index, update) in fields.iter().enumerate() {
                let (FieldUpdate::AppendItem { field, .. } | FieldUpdate::Append { field, .. }) =
                    update
                else {
                    continue;
                };
                let Some(FieldUpdate::Set {
                    field: other_field, ..
                }) = other_fields.get(field_index)
                else {
                    continue;
                };
                if field != other_field {
                    continue;
                }
                let collections = affected_collections(base, &base_rule.action);
                let mut actions: Vec<String> = Vec::new();
                if let Some(collection) = collections.first() {
                    if let Some(creator) = creators(base, collection).first() {
                        if creator != &base_rule.action {
                            actions.push(creator.clone());
                        }
                    }
                }
                actions.push(base_rule.action.clone());
                let sequence = actions.join(">");
                out.push(ContrastExperiment {
                    id: format!(
                        "contrast:replace:{}:{field}:{sequence}",
                        base_rule.action
                    ),
                    kind: ContrastKind::AppendVsReplace {
                        action: base_rule.action.clone(),
                        field: field.clone(),
                    },
                    actions: actions.clone(),
                    candidate_ids: vec![candidate.id.clone()],
                    reason: format!(
                        "distinguish extending '{field}' from replacing it, from a state that already holds a value"
                    ),
                });
                // Repeating the action separates append from replace a second
                // time and covers the case where the first call only seeds it.
                let mut repeated = actions.clone();
                repeated.push(base_rule.action.clone());
                out.push(ContrastExperiment {
                    id: format!(
                        "contrast:replace:{}:{field}:{}",
                        base_rule.action,
                        repeated.join(">")
                    ),
                    kind: ContrastKind::AppendVsReplace {
                        action: base_rule.action.clone(),
                        field: field.clone(),
                    },
                    actions: repeated,
                    candidate_ids: vec![candidate.id.clone()],
                    reason: format!(
                        "observe a second write to '{field}' to separate append from replace"
                    ),
                });
            }
        }
    }

    for base_collection in &base.collections {
        if base_collection.layout != CollectionLayout::List
            || base_collection.id_strategy != IdentityStrategy::Persistent
        {
            continue;
        }
        let Some(other) = program
            .collections
            .iter()
            .find(|collection| collection.name == base_collection.name)
        else {
            continue;
        };
        if !matches!(other.id_strategy, IdentityStrategy::Positional { .. }) {
            continue;
        }
        let creates = creators(base, &base_collection.name);
        let removes = removers(base, &base_collection.name);
        let (Some(create), Some(remove)) = (creates.first(), removes.first()) else {
            continue;
        };
        if create == remove {
            continue;
        }
        let cycle = vec![
            create.clone(),
            create.clone(),
            remove.clone(),
            create.clone(),
        ];
        out.push(ContrastExperiment {
            id: format!(
                "contrast:identity:{}:{}",
                base_collection.name,
                cycle.join(">")
            ),
            kind: ContrastKind::IdentityCompaction {
                collection: base_collection.name.clone(),
            },
            actions: cycle,
            candidate_ids: vec![candidate.id.clone()],
            reason: format!(
                "separate stable identities from compact positions in '{}' by creating, removing and creating again",
                base_collection.name
            ),
        });
    }

    out.truncate(limit);
    out
}

/// Contrasts for every candidate, deduplicated by experiment id. When two
/// candidates imply the same sequence the ids are merged so the scheduler sees
/// one experiment that can separate both.
pub fn contrast_lattice(
    base: &DataProgram,
    candidates: &[ProgramCandidate],
    limit: usize,
) -> Vec<ContrastExperiment> {
    let mut out: Vec<ContrastExperiment> = Vec::new();
    for candidate in candidates {
        for contrast in candidate_contrasts(base, candidate, limit) {
            match out.iter_mut().find(|existing| existing.id == contrast.id) {
                Some(existing) => {
                    for id in contrast.candidate_ids {
                        if !existing.candidate_ids.contains(&id) {
                            existing.candidate_ids.push(id);
                        }
                    }
                }
                None => out.push(contrast),
            }
        }
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    out.truncate(limit);
    out
}

/// Every ordered field that admits an append/replace ambiguity, used by the
/// scheduler audit to explain which hypotheses had no reachable contrast.
pub fn unresolved_ordered_fields(
    base: &DataProgram,
    candidates: &[ProgramCandidate],
) -> Vec<String> {
    let mut out = Vec::new();
    let present: Vec<String> = candidates
        .iter()
        .flat_map(|candidate| candidate_contrasts(base, candidate, 64))
        .map(|contrast| contrast.id)
        .collect();
    for (action, field) in ordered_fields(base) {
        let covered = present
            .iter()
            .any(|id| id.starts_with(&format!("contrast:replace:{action}:{field}:")));
        if !covered {
            out.push(format!("{action}:{field}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn program(value: serde_json::Value) -> DataProgram {
        serde_json::from_value(value).expect("program")
    }

    fn base_fixture() -> DataProgram {
        program(json!({
            "schema_version": "1",
            "target_id": "fixture",
            "initial_state": {"items": [], "counter": 0},
            "collections": [{
                "name": "items",
                "layout": "list",
                "id_field": "id",
                "id_kind": "integer",
                "id_strategy": "persistent"
            }],
            "derived": [],
            "rules": [
                {
                    "id": "create",
                    "action": "create",
                    "branch": "success",
                    "support": 1,
                    "reproduced": 1,
                    "effects": [
                        {"kind": "create_entity", "collection": "items",
                         "id": {"kind": "next_identity", "prefix": ""}, "fields": {}},
                        {"kind": "increment_state", "field": "counter",
                         "by": {"kind": "literal", "value": 1}}
                    ]
                },
                {
                    "id": "tag",
                    "action": "tag",
                    "branch": "success",
                    "support": 1,
                    "reproduced": 1,
                    "effects": [{"kind": "update_entity", "collection": "items",
                                 "entity": {"param": "id"},
                                 "fields": [{"kind": "append_item", "field": "tags",
                                             "value": {"kind": "param", "name": "tag"}}]}]
                },
                {
                    "id": "remove",
                    "action": "remove",
                    "branch": "success",
                    "support": 1,
                    "reproduced": 1,
                    "effects": [{"kind": "delete_entity", "collection": "items",
                                 "entity": {"param": "id"}}]
                }
            ]
        }))
    }

    #[test]
    fn no_op_candidate_yields_a_state_contrast_with_a_prerequisite() {
        let base = base_fixture();
        let mut candidate = base.clone();
        candidate.rules[1].effects.clear();
        let contrast = candidate_contrasts(
            &base,
            &ProgramCandidate {
                id: "noop:tag".into(),
                description: "tag is inert".into(),
                program: candidate,
            },
            8,
        );
        let entry = contrast.first().expect("contrast");
        assert_eq!(entry.actions, vec!["create".to_string(), "tag".to_string()]);
        assert!(matches!(entry.kind, ContrastKind::StateNoOp { .. }));
        assert_eq!(entry.setup(), ["create".to_string()]);
        assert_eq!(entry.target(), Some("tag"));
    }

    #[test]
    fn replace_candidate_yields_seeded_and_repeated_contrasts() {
        let base = base_fixture();
        let mut candidate = base.clone();
        candidate.rules[1].effects = vec![Effect::UpdateEntity {
            collection: "items".into(),
            entity: crate::relational::EntityRef::Param("id".into()),
            fields: vec![FieldUpdate::Set {
                field: "tags".into(),
                value: crate::data::ValueExpr::Param { name: "tag".into() },
            }],
        }];
        let contrasts = candidate_contrasts(
            &base,
            &ProgramCandidate {
                id: "replace:tag:tags".into(),
                description: "tag replaces tags".into(),
                program: candidate.clone(),
            },
            8,
        );
        assert_eq!(contrasts.len(), 2);
        assert_eq!(contrasts[0].actions, vec!["create", "tag"]);
        assert_eq!(contrasts[1].actions, vec!["create", "tag", "tag"]);
        assert!(contrasts
            .iter()
            .all(|contrast| contrast.candidate_ids == vec!["replace:tag:tags".to_string()]));
    }

    #[test]
    fn positional_candidate_yields_create_delete_create_cycle() {
        let base = base_fixture();
        let mut candidate = base.clone();
        candidate.collections[0].id_strategy = IdentityStrategy::Positional { start: 1 };
        let contrasts = candidate_contrasts(
            &base,
            &ProgramCandidate {
                id: "positional:items".into(),
                description: "positions".into(),
                program: candidate,
            },
            8,
        );
        let entry = contrasts.first().expect("identity contrast");
        assert_eq!(entry.actions, vec!["create", "create", "remove", "create"]);
        assert!(matches!(
            entry.kind,
            ContrastKind::IdentityCompaction { .. }
        ));
    }

    #[test]
    fn base_candidate_yields_no_contrast() {
        let base = base_fixture();
        let contrasts = candidate_contrasts(
            &base,
            &ProgramCandidate {
                id: "base".into(),
                description: "selected".into(),
                program: base.clone(),
            },
            8,
        );
        assert!(contrasts.is_empty());
    }

    #[test]
    fn lattice_merges_identical_sequences_across_candidates() {
        let base = base_fixture();
        let mut positioned = base.clone();
        positioned.collections[0].id_strategy = IdentityStrategy::Positional { start: 1 };
        let mut inert = base.clone();
        inert.rules[1].effects.clear();
        let candidates = vec![
            ProgramCandidate {
                id: "positional:items".into(),
                description: "positions".into(),
                program: positioned,
            },
            ProgramCandidate {
                id: "noop:tag".into(),
                description: "inert".into(),
                program: inert,
            },
        ];
        let lattice = contrast_lattice(&base, &candidates, 16);
        assert_eq!(lattice.len(), 2);
        let ids: Vec<&str> = lattice.iter().map(|entry| entry.id.as_str()).collect();
        assert!(ids.iter().any(|id| id.starts_with("contrast:identity:")));
        assert!(ids.iter().any(|id| id.starts_with("contrast:noop:")));
    }
}
