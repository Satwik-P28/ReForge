use indexmap::IndexMap;
use reforge_behavior::{assert_deterministic, TwinEngine};
use reforge_types::*;

fn tiny_twin() -> BehavioralTwin {
    let mut states = IndexMap::new();
    states.insert(
        "a".into(),
        TwinState {
            name: "a".into(),
            terminal: false,
            description: None,
        },
    );
    states.insert(
        "b".into(),
        TwinState {
            name: "b".into(),
            terminal: true,
            description: None,
        },
    );
    let mut actions = IndexMap::new();
    actions.insert(
        "go".into(),
        TwinAction {
            name: "go".into(),
            parameters: IndexMap::new(),
            description: None,
        },
    );
    let mut variables = IndexMap::new();
    variables.insert(
        "x".into(),
        StateVariable {
            name: "x".into(),
            ty: TypeDesc::Int,
            initial: Some(TypedValue::Int(0)),
            required: true,
        },
    );
    BehavioralTwin {
        schema_version: SchemaVersion::current(),
        id: Id::new("tiny").unwrap(),
        name: "tiny".into(),
        description: None,
        initial_state: "a".into(),
        states,
        variables,
        actions,
        transitions: vec![
            TwinTransition {
                id: Id::new("t1").unwrap(),
                from: "a".into(),
                to: "a".into(),
                action: "go".into(),
                guard: Some(serde_json::json!("state.x < 2")),
                updates: IndexMap::from([(
                    "x".into(),
                    serde_json::json!({"op":"add","left":{"op":"ref","root":"state","path":["x"]},"right":{"op":"lit_int","value":1}}),
                )]),
                outputs: IndexMap::new(),
                events: vec!["ticked".into()],
                error: None,
                certainty: Certainty::Specified,
                evidence_refs: vec![],
            },
            TwinTransition {
                id: Id::new("t2").unwrap(),
                from: "a".into(),
                to: "b".into(),
                action: "go".into(),
                guard: Some(serde_json::json!("state.x >= 2")),
                updates: IndexMap::new(),
                outputs: IndexMap::new(),
                events: vec![],
                error: None,
                certainty: Certainty::Specified,
                evidence_refs: vec![],
            },
        ],
        invariants: vec![InvariantDecl {
            id: Id::new("x-nonneg").unwrap(),
            expression: serde_json::json!("state.x >= 0"),
            description: None,
        }],
        unknowns: vec![],
        hypotheses: vec![HypothesisDecl {
            id: Id::new("h1").unwrap(),
            description: "maybe later".into(),
            candidate_transition_ids: vec![],
            supporting_evidence: vec![],
            contradicting_evidence: vec![],
            status: HypothesisStatus::Open,
        }],
        evidence_refs: vec![],
        coverage: CoverageReport::default(),
        certainty: Certainty::Specified,
    }
}

#[test]
fn valid_and_terminal() {
    let mut eng = TwinEngine::new(tiny_twin()).unwrap();
    assert!(matches!(
        eng.execute("go", IndexMap::new()).result,
        StepResult::Success
    ));
    assert!(matches!(
        eng.execute("go", IndexMap::new()).result,
        StepResult::Success
    ));
    assert!(matches!(
        eng.execute("go", IndexMap::new()).result,
        StepResult::Success
    ));
    assert_eq!(eng.runtime().current_state, "b");
    let rejected = eng.execute("go", IndexMap::new());
    assert!(matches!(rejected.result, StepResult::Rejected { .. }));
}

#[test]
fn unknown_and_hypothesis() {
    let mut twin = tiny_twin();
    twin.transitions.push(TwinTransition {
        id: Id::new("hyp-a").unwrap(),
        from: "a".into(),
        to: "a".into(),
        action: "maybe".into(),
        guard: Some(serde_json::json!("true")),
        updates: IndexMap::new(),
        outputs: IndexMap::new(),
        events: vec![],
        error: None,
        certainty: Certainty::Hypothetical,
        evidence_refs: vec![],
    });
    twin.transitions.push(TwinTransition {
        id: Id::new("hyp-b").unwrap(),
        from: "a".into(),
        to: "b".into(),
        action: "maybe".into(),
        guard: Some(serde_json::json!("true")),
        updates: IndexMap::new(),
        outputs: IndexMap::new(),
        events: vec![],
        error: None,
        certainty: Certainty::Hypothetical,
        evidence_refs: vec![],
    });
    twin.actions.insert(
        "maybe".into(),
        TwinAction {
            name: "maybe".into(),
            parameters: IndexMap::new(),
            description: None,
        },
    );
    twin.hypotheses.push(HypothesisDecl {
        id: Id::new("compete").unwrap(),
        description: "two maybes".into(),
        candidate_transition_ids: vec![Id::new("hyp-a").unwrap(), Id::new("hyp-b").unwrap()],
        supporting_evidence: vec![],
        contradicting_evidence: vec![],
        status: HypothesisStatus::Open,
    });
    let mut eng = TwinEngine::new(twin).unwrap();
    let step = eng.execute("maybe", IndexMap::new());
    assert!(matches!(step.result, StepResult::HypothesisConflict { .. }));
}

#[test]
fn state_preserved_on_reject() {
    let mut eng = TwinEngine::new(tiny_twin()).unwrap();
    let before = eng.runtime().clone();
    let step = eng.execute("nope", IndexMap::new());
    assert!(matches!(step.result, StepResult::Unknown { .. }));
    assert_eq!(eng.runtime(), &before);
}

#[test]
fn deterministic_replay() {
    let twin = tiny_twin();
    let seq = reforge_behavior::ActionSequence {
        actions: vec![
            reforge_behavior::ActionCall {
                action: "go".into(),
                parameters: IndexMap::new(),
            },
            reforge_behavior::ActionCall {
                action: "go".into(),
                parameters: IndexMap::new(),
            },
        ],
    };
    assert_deterministic(twin, &seq).unwrap();
}
