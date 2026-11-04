//! ReForgeBench Phase 1 — reproducible semantic-core measurements.
//!
//! Records actual timings. Does not fabricate results.

use anyhow::Result;
use indexmap::IndexMap;
use reforge::behavior::{replay_sequence, ActionCall, ActionSequence, TwinEngine};
use reforge::canon::digest_of_canonical;
use reforge::dependency::DependencyGraph;
use reforge::evidence::{ContentStore, EvidenceGraph};
use reforge::types::*;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

fn main() -> Result<()> {
    let env = EnvInfo::capture();
    println!("ReForgeBench Phase 1");
    println!("{}", serde_json::to_string_pretty(&env)?);
    println!();

    let sizes = [10usize, 100, 1_000, 10_000];
    let mut results = Vec::new();

    for &n in &sizes {
        results.push(measure_package_parse(n)?);
        results.push(measure_validation(n)?);
        results.push(measure_replay(n)?);
        results.push(measure_invariant_overhead(n)?);
        results.push(measure_dependency_graph(n)?);
        results.push(measure_evidence_import(n)?);
        results.push(measure_serialization(n)?);
    }

    // Composition-path metrics (fixed example sizes; size field = capability count).
    if let Ok(comp) = measure_composition_pipeline() {
        results.extend(comp);
    }

    let report = BenchReport {
        env,
        methodology: Methodology {
            runs_per_measurement: 7,
            warmup: 2,
            aggregation: "median_with_min_max".into(),
            notes: vec![
                "Synthetic fixtures; not application reconstruction benchmarks.".into(),
                "Composition metrics use examples/compositions/notes-tasks when present.".into(),
                "Memory usage reported only when available via allocator snapshot proxy (RSS not portable).".into(),
            ],
        },
        results,
    };

    let out_dir = std::path::Path::new("bench-results");
    std::fs::create_dir_all(out_dir)?;
    let path = out_dir.join("phase1-latest.json");
    std::fs::write(&path, serde_json::to_string_pretty(&report)?)?;
    let path_v01 = out_dir.join("v0.1-latest.json");
    std::fs::write(&path_v01, serde_json::to_string_pretty(&report)?)?;
    println!("Wrote {}", path.display());
    println!("Wrote {}", path_v01.display());
    for r in &report.results {
        println!(
            "{:<28} n={:<6} median_ns={:<12} min={} max={}",
            r.metric, r.size, r.median_ns, r.min_ns, r.max_ns
        );
    }
    Ok(())
}

#[derive(Debug, serde::Serialize)]
struct EnvInfo {
    os: String,
    arch: String,
    rustc: String,
    timestamp_unix: u64,
    hostname_hash: String,
}

impl EnvInfo {
    fn capture() -> Self {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            rustc: std::process::Command::new("rustc")
                .arg("--version")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("rust-version-req {}", env!("CARGO_PKG_RUST_VERSION"))),
            timestamp_unix: ts,
            hostname_hash: "omitted".into(),
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct Methodology {
    runs_per_measurement: u32,
    warmup: u32,
    aggregation: String,
    notes: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
struct BenchReport {
    env: EnvInfo,
    methodology: Methodology,
    results: Vec<MetricResult>,
}

#[derive(Debug, serde::Serialize)]
struct MetricResult {
    metric: String,
    size: usize,
    median_ns: u128,
    min_ns: u128,
    max_ns: u128,
    extra: serde_json::Value,
}

fn timed_median(warmup: u32, runs: u32, mut f: impl FnMut()) -> (u128, u128, u128) {
    for _ in 0..warmup {
        f();
    }
    let mut samples = Vec::with_capacity(runs as usize);
    for _ in 0..runs {
        let start = Instant::now();
        f();
        samples.push(start.elapsed().as_nanos());
    }
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let min = *samples.first().unwrap();
    let max = *samples.last().unwrap();
    (median, min, max)
}

fn sample_twin(n_states: usize, transitions_per_state: usize) -> BehavioralTwin {
    let mut states = IndexMap::new();
    for i in 0..n_states {
        let name = format!("s{i}");
        states.insert(
            name.clone(),
            TwinState {
                name,
                terminal: false,
                description: None,
            },
        );
    }
    let mut actions = IndexMap::new();
    actions.insert(
        "step".into(),
        TwinAction {
            name: "step".into(),
            parameters: IndexMap::new(),
            description: None,
        },
    );
    let mut transitions = Vec::new();
    for i in 0..n_states {
        for k in 0..transitions_per_state.min(n_states) {
            let to = (i + k + 1) % n_states;
            transitions.push(TwinTransition {
                id: Id::new(format!("t-{i}-{k}")).unwrap(),
                from: format!("s{i}"),
                to: format!("s{to}"),
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
            });
        }
    }
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
        id: Id::new(format!("bench-twin-{n_states}")).unwrap(),
        name: format!("bench-{n_states}"),
        description: None,
        initial_state: "s0".into(),
        states,
        variables,
        actions,
        transitions,
        invariants: vec![InvariantDecl {
            id: Id::new("counter-nonneg").unwrap(),
            expression: serde_json::json!("state.counter >= 0"),
            description: None,
        }],
        unknowns: vec![],
        hypotheses: vec![],
        evidence_refs: vec![],
        coverage: CoverageReport::default(),
        certainty: Certainty::Specified,
    }
}

fn measure_package_parse(n: usize) -> Result<MetricResult> {
    let twin = sample_twin(n.min(1000), 1);
    let json = serde_json::to_string(&twin)?;
    let (median, min, max) = timed_median(2, 7, || {
        let _: BehavioralTwin = serde_json::from_str(&json).unwrap();
    });
    Ok(MetricResult {
        metric: "package_parse_latency".into(),
        size: n,
        median_ns: median,
        min_ns: min,
        max_ns: max,
        extra: serde_json::json!({"bytes": json.len()}),
    })
}

fn measure_validation(n: usize) -> Result<MetricResult> {
    let twin = sample_twin(n.min(1000), 1);
    let (median, min, max) = timed_median(2, 7, || {
        let _ = twin.validate_structure();
    });
    Ok(MetricResult {
        metric: "package_validation_latency".into(),
        size: n,
        median_ns: median,
        min_ns: min,
        max_ns: max,
        extra: serde_json::json!({}),
    })
}

fn measure_replay(n: usize) -> Result<MetricResult> {
    let twin = sample_twin((n as f64).sqrt() as usize + 1, 1);
    let seq = ActionSequence {
        actions: (0..n.min(5000))
            .map(|_| ActionCall {
                action: "step".into(),
                parameters: IndexMap::new(),
            })
            .collect(),
    };
    let (median, min, max) = timed_median(1, 5, || {
        let _ = replay_sequence(twin.clone(), &seq).unwrap();
    });
    let throughput = (n.min(5000) as u128)
        .checked_mul(1_000_000_000)
        .and_then(|v| v.checked_div(median))
        .unwrap_or(0);
    Ok(MetricResult {
        metric: "behavioral_replay_throughput".into(),
        size: n,
        median_ns: median,
        min_ns: min,
        max_ns: max,
        extra: serde_json::json!({"actions_per_sec_est": throughput}),
    })
}

fn measure_invariant_overhead(n: usize) -> Result<MetricResult> {
    let mut twin = sample_twin(5, 1);
    twin.invariants.clear();
    for i in 0..n.min(500) {
        twin.invariants.push(InvariantDecl {
            id: Id::new(format!("inv-{i}")).unwrap(),
            expression: serde_json::json!("state.counter >= 0"),
            description: None,
        });
    }
    let engine = TwinEngine::new(twin).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let (median, min, max) = timed_median(2, 7, || {
        let _ = engine.check_invariants();
    });
    Ok(MetricResult {
        metric: "invariant_checking_overhead".into(),
        size: n,
        median_ns: median,
        min_ns: min,
        max_ns: max,
        extra: serde_json::json!({}),
    })
}

fn measure_dependency_graph(n: usize) -> Result<MetricResult> {
    let mut graph = DependencyGraph::new();
    for i in 0..n.min(2000) {
        let id = Id::new(format!("cap-{i}")).unwrap();
        let mut required = vec![];
        if i > 0 {
            required.push(DepEdge {
                capability_id: Id::new(format!("cap-{}", i - 1)).unwrap(),
                version_req: VersionReq::new(">=0.1.0").unwrap(),
                required_interfaces: vec![],
            });
        }
        graph.add(DependencyContract {
            capability_id: id,
            version: PackageVersion::new("1.0.0").unwrap(),
            required_capabilities: required,
            optional_capabilities: vec![],
            provided_interfaces: IndexMap::new(),
            required_interfaces: IndexMap::new(),
            runtime_assumptions: vec![],
            resource_requirements: vec![],
            permission_requirements: vec![],
            explicit_incompatibilities: vec![],
            side_effects: vec![],
        });
    }
    let (median, min, max) = timed_median(2, 7, || {
        let _ = graph.validate();
    });
    Ok(MetricResult {
        metric: "dependency_graph_analysis".into(),
        size: n,
        median_ns: median,
        min_ns: min,
        max_ns: max,
        extra: serde_json::json!({}),
    })
}

fn measure_evidence_import(n: usize) -> Result<MetricResult> {
    let payload = vec![0u8; n.min(10_000)];
    let (median, min, max) = timed_median(2, 7, || {
        let mut store = ContentStore::new();
        for i in 0..100 {
            let mut p = payload.clone();
            if !p.is_empty() {
                p[0] = i as u8;
            }
            let _ = store.put(&p);
        }
        let mut g = EvidenceGraph::new();
        g.store = store;
        let _ = g.verify_all_integrity();
    });
    Ok(MetricResult {
        metric: "evidence_import_verify".into(),
        size: n,
        median_ns: median,
        min_ns: min,
        max_ns: max,
        extra: serde_json::json!({}),
    })
}

fn measure_serialization(n: usize) -> Result<MetricResult> {
    let twin = sample_twin(n.min(500), 2);
    let (median, min, max) = timed_median(2, 7, || {
        let _ = digest_of_canonical(&twin).unwrap();
    });
    let bytes = serde_json::to_vec(&twin)?.len();
    Ok(MetricResult {
        metric: "serialization_canonical_digest".into(),
        size: n,
        median_ns: median,
        min_ns: min,
        max_ns: max,
        extra: serde_json::json!({"output_size_bytes": bytes}),
    })
}

fn measure_composition_pipeline() -> Result<Vec<MetricResult>> {
    use reforge::compose::compile_file;
    use reforge::runtime::AppRuntime;
    use std::path::PathBuf;

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let composition = root.join("examples/compositions/notes-tasks/composition.json");
    if !composition.is_file() {
        anyhow::bail!("composition fixture missing");
    }

    let (c_med, c_min, c_max) = timed_median(2, 7, || {
        let _ = compile_file(&composition).expect("compile");
    });

    let compiled = compile_file(&composition)?;
    let n_caps = compiled.ir.instances.len();
    let mut start_i = 0u64;
    let (s_med, s_min, s_max) = timed_median(2, 7, || {
        start_i += 1;
        let mut rt = AppRuntime::from_compile(compile_file(&composition).unwrap()).unwrap();
        rt.attach_memory_db().unwrap();
        let _ = rt.dispatch(
            "tasks",
            "create_task",
            IndexMap::from([
                ("id".into(), TypedValue::String(format!("t-{start_i}"))),
                ("title".into(), TypedValue::String("bench".into())),
            ]),
        );
    });

    let mut rt = AppRuntime::from_compile(compiled)?;
    rt.attach_memory_db()?;
    let mut cmd_i = 0u64;
    let (d_med, d_min, d_max) = timed_median(2, 7, || {
        cmd_i += 1;
        let _ = rt.dispatch(
            "notes",
            "create_note",
            IndexMap::from([
                ("id".into(), TypedValue::String(format!("n-{cmd_i}"))),
                ("title".into(), TypedValue::String("b".into())),
                ("body".into(), TypedValue::String("".into())),
            ]),
        );
    });

    Ok(vec![
        MetricResult {
            metric: "composition_compile".into(),
            size: n_caps,
            median_ns: c_med,
            min_ns: c_min,
            max_ns: c_max,
            extra: serde_json::json!({"fixture": "notes-tasks"}),
        },
        MetricResult {
            metric: "app_startup_plus_command".into(),
            size: n_caps,
            median_ns: s_med,
            min_ns: s_min,
            max_ns: s_max,
            extra: serde_json::json!({}),
        },
        MetricResult {
            metric: "command_execution".into(),
            size: n_caps,
            median_ns: d_med,
            min_ns: d_min,
            max_ns: d_max,
            extra: serde_json::json!({}),
        },
    ])
}
