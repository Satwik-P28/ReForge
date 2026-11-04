use criterion::{criterion_group, criterion_main, Criterion};
use indexmap::IndexMap;
use reforge::behavior::{replay_sequence, ActionCall, ActionSequence};
use reforge::types::*;

fn twin(_n: usize) -> BehavioralTwin {
    let mut states = IndexMap::new();
    states.insert(
        "s0".into(),
        TwinState {
            name: "s0".into(),
            terminal: false,
            description: None,
        },
    );
    let mut actions = IndexMap::new();
    actions.insert(
        "step".into(),
        TwinAction {
            name: "step".into(),
            parameters: IndexMap::new(),
            description: None,
        },
    );
    let mut variables = IndexMap::new();
    variables.insert(
        "counter".into(),
        StateVariable {
            name: "counter".into(),
            ty: TypeDesc::Int,
            initial: Some(TypedValue::Int(0)),
            required: true,
        },
    );
    BehavioralTwin {
        schema_version: SchemaVersion::current(),
        id: Id::new("crit").unwrap(),
        name: "crit".into(),
        description: None,
        initial_state: "s0".into(),
        states,
        variables,
        actions,
        transitions: vec![TwinTransition {
            id: Id::new("t0").unwrap(),
            from: "s0".into(),
            to: "s0".into(),
            action: "step".into(),
            guard: Some(serde_json::json!("true")),
            updates: IndexMap::from([(
                "counter".into(),
                serde_json::json!({"op":"add","left":{"op":"ref","root":"state","path":["counter"]},"right":{"op":"lit_int","value":1}}),
            )]),
            outputs: IndexMap::new(),
            events: vec![],
            error: None,
            certainty: Certainty::Specified,
            evidence_refs: vec![],
        }],
        invariants: vec![],
        unknowns: vec![],
        hypotheses: vec![],
        evidence_refs: vec![],
        coverage: CoverageReport::default(),
        certainty: Certainty::Specified,
    }
}

fn bench_replay(c: &mut Criterion) {
    let t = twin(1);
    let seq = ActionSequence {
        actions: (0..100)
            .map(|_| ActionCall {
                action: "step".into(),
                parameters: IndexMap::new(),
            })
            .collect(),
    };
    c.bench_function("replay_100_steps", |b| {
        b.iter(|| replay_sequence(t.clone(), &seq).unwrap())
    });
}

criterion_group!(benches, bench_replay);
criterion_main!(benches);
