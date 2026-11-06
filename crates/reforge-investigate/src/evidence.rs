//! Evidence emission: turn observations and discovered behaviors into a
//! provenance graph, so every extracted claim can be traced to captures.

use crate::abstraction::AbstractionSummary;
use crate::model::TransitionRecord;
use indexmap::IndexMap;
use reforge_evidence::EvidenceGraph;
use reforge_observe::{Observation, ObservationLog, TargetIdentity};
use reforge_types::*;

/// Build an evidence graph from an observation log plus discovered behaviors.
///
/// * Every observation becomes a `DirectObservation` record whose payload is
///   the canonical JSON of the observation itself.
/// * Every discovered behavior becomes a claim with `Certainty::Observed`.
/// * Behaviors with conflicting evidence get both supporting and contradicting
///   links, which the graph reports as an unresolved contradiction.
pub fn build_evidence(
    log: &ObservationLog,
    abstraction: &AbstractionSummary,
    transitions: &[TransitionRecord],
    target: &TargetIdentity,
    tool: &str,
    tool_version: &str,
) -> Result<EvidenceGraph, String> {
    let mut graph = EvidenceGraph::new();
    // Map fingerprints back to abstract classes so evidence links line up with
    // the behavior model instead of with raw digests.
    let class_index = crate::abstraction::StateAbstraction::from_summary(abstraction);

    // observation id -> evidence id
    let mut evidence_by_obs: IndexMap<String, Id> = IndexMap::new();

    for obs in &log.observations {
        let bytes = serde_json::to_vec(obs).map_err(|e| e.to_string())?;
        let digest = graph.insert_payload(&bytes);
        let payload_ref = format!("payloads/{}", digest.hex);
        // Content-addressed id: independent of session/observation naming, so
        // the executable package never encodes where the evidence came from.
        let evidence_id =
            Id::new(format!("ev.{}", &digest.hex[..16])).map_err(|e| e.to_string())?;
        let record = EvidenceRecord {
            schema_version: SchemaVersion::current(),
            id: evidence_id.clone(),
            evidence_type: EvidenceType::Observation,
            source: EvidenceSource::Tool {
                name: tool.to_string(),
            },
            acquisition_method: AcquisitionMethod::RuntimeCapture,
            collection_context: Some(format!(
                "session {} step {} on {}",
                obs.session_id, obs.step_index, target.name
            )),
            timestamp: None,
            environment: Some(format!(
                "{:?}/{} viewport={:?}",
                obs.environment.platform, obs.environment.tool, obs.environment.viewport
            )),
            artifact_id: Some(target.id.clone()),
            content_digest: digest,
            payload_ref,
            related_claims: Vec::new(),
            tool: Some(tool.to_string()),
            tool_version: Some(tool_version.to_string()),
            integrity: IntegrityStatus::Verified,
            hierarchy: EvidenceHierarchy::DirectObservation,
            metadata: IndexMap::from([
                (
                    "reproducibility".to_string(),
                    format!("{:?}", obs.reproducibility),
                ),
                (
                    "completeness".to_string(),
                    format!("{:?}", obs.completeness),
                ),
            ]),
        };
        graph.add_evidence(record).map_err(|e| e.to_string())?;
        evidence_by_obs.insert(obs.id.to_string(), evidence_id);
    }

    // Which observations support which behavior.
    let mut supports: IndexMap<(String, String, String, String), Vec<Id>> = IndexMap::new();
    for obs in &log.observations {
        let Some(action) = &obs.action else { continue };
        let Some(after) = &obs.after else { continue };
        let outcome = crate::model::OutcomeKind::from_observation(obs);
        let class_before = class_index
            .find(&obs.before)
            .unwrap_or_else(|| obs.before.short());
        let class_after = class_index.find(after).unwrap_or_default();
        let key = (
            class_before,
            action.action.clone(),
            outcome.as_str().to_string(),
            class_after,
        );
        if let Some(ev) = evidence_by_obs.get(obs.id.as_str()) {
            supports.entry(key).or_default().push(ev.clone());
        }
    }

    for t in transitions {
        let claim_id = Id::new(format!(
            "claim.{}.{}.{}",
            t.class_before,
            t.action,
            t.outcome.as_str()
        ))
        .map_err(|e| e.to_string())?;
        let contradictory = t.samples > 1
            && transitions.iter().any(|o| {
                o.class_before == t.class_before && o.action == t.action && o.outcome != t.outcome
            });
        let claim = BehavioralClaim {
            schema_version: SchemaVersion::current(),
            id: claim_id.clone(),
            subject: format!("{}.{}", t.class_before, t.action),
            statement: format!(
                "In abstract state '{}', action '{}' was observed with outcome '{}'{}",
                t.class_before,
                t.action,
                t.outcome.as_str(),
                if t.class_after.is_empty() {
                    String::new()
                } else {
                    format!(" transitioning to '{}'", t.class_after)
                }
            ),
            certainty: Certainty::Observed,
            supporting_evidence: Vec::new(),
            contradicting_evidence: Vec::new(),
            derived_from: Vec::new(),
            verified_by_tests: Vec::new(),
            notes: Some(if contradictory {
                format!(
                    "samples: {}; conflicting observations exist for this cell (see contradicting links)",
                    t.samples
                )
            } else {
                format!("samples: {}", t.samples)
            }),
        };
        graph.add_claim(claim);

        // Link every observation of this cell as supporting evidence.
        for (key, ids) in &supports {
            if key.1 == t.action && key.0 == t.class_before {
                for id in ids {
                    let relation = if key.2 == t.outcome.as_str() {
                        EvidenceRelation::Supports {
                            evidence_id: id.clone(),
                            claim_id: claim_id.clone(),
                        }
                    } else {
                        EvidenceRelation::Contradicts {
                            evidence_id: id.clone(),
                            claim_id: claim_id.clone(),
                        }
                    };
                    let _ = graph.link(relation);
                }
            }
        }
    }

    Ok(graph)
}

/// Convenience: evidence ids that mention a given observation.
pub fn evidence_for_observation(log: &ObservationLog, observation: &Observation) -> Option<Id> {
    let stored = log.observations.iter().find(|o| o.id == observation.id)?;
    let bytes = serde_json::to_vec(stored).ok()?;
    let digest = reforge_canon::digest_of_raw(&bytes);
    Id::new(format!("ev.{}", &digest.hex[..16])).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abstraction::{AbstractionProfile, ClassSummary};
    use crate::model::OutcomeKind;
    use reforge_observe::*;
    use reforge_types::TypedValue;

    fn summary() -> AbstractionSummary {
        AbstractionSummary {
            profile: AbstractionProfile::with_keys(vec!["view".into()]),
            classes: vec![
                ClassSummary {
                    id: "s0".into(),
                    projection: "view=empty".into(),
                    samples: 1,
                },
                ClassSummary {
                    id: "s1".into(),
                    projection: "view=list".into(),
                    samples: 1,
                },
            ],
            refinements: vec![],
        }
    }

    fn fp(view: &str) -> StateFingerprint {
        let mut features = std::collections::BTreeMap::new();
        features.insert("view".to_string(), view.to_string());
        StateFingerprint::from_features(FingerprintKind::Structured, features)
    }

    fn env() -> EnvironmentInfo {
        EnvironmentInfo {
            platform: PlatformKind::Cli,
            os: "linux".into(),
            tool: "test".into(),
            tool_version: "0".into(),
            viewport: None,
            fixture_digest: None,
            clock: "deterministic".into(),
        }
    }

    fn obs(id: &str, action: &str, outcome: &str) -> Observation {
        let mut outputs = IndexMap::new();
        outputs.insert("outcome".into(), TypedValue::String(outcome.into()));
        Observation {
            schema_version: IrVersion::current(),
            id: Id::new(id).unwrap(),
            session_id: Id::new("s1").unwrap(),
            step_index: 1,
            target: Id::new("t1").unwrap(),
            baseline: false,
            action: Some(ActionInstance::new(action)),
            before: fp("empty"),
            after: Some(fp("list")),
            outputs,
            state_document: None,
            visible_text: None,
            side_effects: vec![],
            errors: vec![],
            timing: TimingInfo::default(),
            environment: env(),
            payloads: vec![],
            evidence_refs: vec![],
            completeness: Completeness::Complete,
            uncertainty: vec![],
            reproducibility: Reproducibility::Deterministic,
            inferred_hidden_state: vec![],
            reset: ResetProvenance::assumed(ResetPolicy::FreshEnvironment),
        }
    }

    #[test]
    fn observations_and_claims_are_linked_with_provenance() {
        let mut log = ObservationLog::new();
        log.append(obs("o1", "delete", "rejected"), None).unwrap();
        let target = TargetIdentity {
            id: Id::new("cli-taskboard").unwrap(),
            name: "taskboard".into(),
            platform: PlatformKind::Cli,
            version: "1".into(),
            authorization: Authorization::LocalFixture,
        };
        let transitions = vec![TransitionRecord {
            class_before: "s0".into(),
            action: "delete".into(),
            outcome: OutcomeKind::Rejected,
            class_after: String::new(),
            samples: 1,
            error_codes: vec!["no_such_task".into()],
            params: Default::default(),
            events: Vec::new(),
            outputs: Default::default(),
        }];
        let graph = build_evidence(
            &log,
            &summary(),
            &transitions,
            &target,
            "cli-adapter",
            "0.2.0",
        )
        .unwrap();
        assert_eq!(graph.evidence.len(), 1);
        assert_eq!(graph.claims.len(), 1);
        let explanation = graph.explain("claim.s0.delete.rejected").unwrap();
        assert!(!explanation.supporting.is_empty());
        assert!(!explanation.unresolved_contradiction);
    }
}
