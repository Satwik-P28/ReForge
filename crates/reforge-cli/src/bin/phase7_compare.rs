//! Matched Phase 7 comparison: black-box-only vs REA-guided investigation.
//!
//! Both arms use the same target version, initial state, action schema, seed,
//! policy, and behavioral interaction budget. The only difference is whether
//! imported REA evidence is supplied to EvidenceGuidance.
use reforge_investigate::{
    investigate, synthesize_data_program, CliTargetSpec, DataProgram, DataStatus,
    InvestigationConfig, InvestigationRecord, PolicyKind,
};
use reforge_rea::{ReaConfig, ReaProvider};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 6 {
        anyhow::bail!("usage: phase7_compare <rea-bin> <source> <target.json> <out-dir> <budget>");
    }
    let rea_bin = PathBuf::from(&args[1]);
    let source = PathBuf::from(&args[2]);
    let spec_path = PathBuf::from(&args[3]);
    let out = PathBuf::from(&args[4]);
    let budget: u32 = args[5].parse()?;
    std::fs::create_dir_all(&out)?;

    let config = ReaConfig {
        command: Some(rea_bin),
        ..ReaConfig::default()
    };
    let rea_started = Instant::now();
    let provider = ReaProvider::detect(&config)?;
    let analysis = provider.analyze_javascript(&source, &out.join("rea"), &config)?;
    let rea_analysis_ms = rea_started.elapsed().as_millis();

    let spec = CliTargetSpec::load(&spec_path)?;
    let mut arms = Vec::new();
    for (name, guided) in [("blackbox", false), ("rea_guided", true)] {
        let arm_dir = out.join(name);
        std::fs::create_dir_all(&arm_dir)?;
        let mut target = spec.build(Some(&arm_dir.join("work")))?;
        let started = Instant::now();
        let mut investigation = InvestigationConfig::default()
            .with_policy(PolicyKind::Hypothesis)
            .with_budget(budget)
            .with_seed(1);
        if guided {
            investigation.evidence_guidance = Some(analysis.projection.guidance());
        }
        let record = investigate(&mut target, &investigation)?;
        let elapsed_ms = started.elapsed().as_millis();
        std::fs::write(
            arm_dir.join("investigation.json"),
            serde_json::to_vec_pretty(&record)?,
        )?;
        let data = synthesize_data_program(&record);
        std::fs::write(
            arm_dir.join("data-program.json"),
            serde_json::to_vec_pretty(&data)?,
        )?;
        let workflow = run_workflow(&data);
        arms.push(json!({
            "arm": name,
            "rea_guidance": guided,
            "elapsed_ms": elapsed_ms,
            "actions": record.coverage.actions_used,
            "transitions": record.coverage.transitions,
            "classes": record.coverage.classes,
            "guidance_evidence": record.evidence_guidance.as_ref().map(|g| g.evidence_ids.clone()),
            "rules": data.rules.len(),
            "exact_rules": data.rules.iter().filter(|rule| rule.exact()).count(),
            "support": data.rules.iter().map(|rule| rule.support).sum::<u32>(),
            "reproduced": data.rules.iter().map(|rule| rule.reproduced).sum::<u32>(),
            "unsupported": data.unsupported,
            "workflow": workflow,
        }));
        write_record(&arm_dir, &record)?;
    }

    let report = json!({
        "target": spec_path,
        "budget": budget,
        "seed": 1,
        "policy": "hypothesis",
        "rea_version": provider.version().version,
        "rea_evidence_id": analysis.evidence_id,
        "rea_analysis_ms": rea_analysis_ms,
        "arms": arms,
    });
    std::fs::write(
        out.join("comparison.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn write_record(dir: &Path, record: &InvestigationRecord) -> anyhow::Result<()> {
    std::fs::write(dir.join("investigation-summary.json"), record.to_json()?)?;
    Ok(())
}

/// A reference-free workflow over synthesized state. The checks express the
/// observable Taskbook subset, not Taskbook implementation details.
fn run_workflow(program: &DataProgram) -> Value {
    let mut state = program.initial_state.clone();
    let mut checks = Vec::new();
    let mut check = |name: &str, ok: bool, detail: Value| {
        checks.push(json!({"check": name, "passed": ok, "detail": detail}));
    };

    let (next, out) = execute(program, &state, "create_task", &[("title", "cmp alpha 71")]);
    state = next;
    check(
        "create_first_id",
        id_value(&out.outputs, "created_id") == Some(1),
        json!(out.outputs),
    );

    let (next, out) = execute(program, &state, "create_task", &[("title", "cmp beta 72")]);
    state = next;
    check(
        "create_second_id",
        id_value(&out.outputs, "created_id") == Some(2),
        json!(out.outputs),
    );

    let (next, out) = execute(program, &state, "read_task", &[("id", "1")]);
    state = next;
    check(
        "read_first",
        out.status == DataStatus::Success
            && id_value(&out.outputs, "id") == Some(1)
            && out.outputs.get("description").and_then(Value::as_str) == Some("cmp alpha 71")
            && out.outputs.get("isComplete").and_then(Value::as_bool) == Some(false),
        json!(out.outputs),
    );

    let (next, out) = execute(program, &state, "check_task", &[("id", "1")]);
    state = next;
    check(
        "check_first",
        out.status == DataStatus::Success
            && id_value(&out.outputs, "done") == Some(1)
            && id_value(&out.outputs, "pending") == Some(1),
        json!(out.outputs),
    );

    let (next, out) = execute(program, &state, "read_task", &[("id", "1")]);
    state = next;
    check(
        "read_after_check",
        out.status == DataStatus::Success
            && out.outputs.get("isComplete").and_then(Value::as_bool) == Some(true),
        json!(out.outputs),
    );

    let (next, out) = execute(program, &state, "stats", &[]);
    state = next;
    check(
        "stats_two",
        out.status == DataStatus::Success
            && id_value(&out.outputs, "total") == Some(2)
            && id_value(&out.outputs, "pending") == Some(1)
            && id_value(&out.outputs, "done") == Some(1),
        json!(out.outputs),
    );

    let (next, out) = execute(program, &state, "read_task", &[("id", "999")]);
    state = next;
    check(
        "missing_not_success",
        out.status != DataStatus::Success,
        json!({"status": out.status, "detail": out.detail}),
    );

    let (next, out) = execute(program, &state, "delete_task", &[("id", "2")]);
    state = next;
    check(
        "delete_second",
        out.status == DataStatus::Success
            && out.outputs.get("deleted").and_then(Value::as_bool) == Some(true),
        json!(out.outputs),
    );

    let (next, out) = execute(program, &state, "stats", &[]);
    state = next;
    check(
        "stats_after_delete",
        out.status == DataStatus::Success
            && id_value(&out.outputs, "total") == Some(1)
            && id_value(&out.outputs, "pending") == Some(0),
        json!(out.outputs),
    );

    // Restoration is a serialization boundary, matching a process restart.
    let restored: Value = serde_json::from_str(&state.to_string()).unwrap_or(Value::Null);
    let (_next, out) = execute(program, &restored, "read_task", &[("id", "1")]);
    check(
        "restart_read",
        out.status == DataStatus::Success && id_value(&out.outputs, "id") == Some(1),
        json!(out.outputs),
    );

    let passed = checks
        .iter()
        .filter(|check| check.get("passed").and_then(Value::as_bool) == Some(true))
        .count();
    json!({"passed": passed, "total": checks.len(), "checks": checks})
}

fn execute(
    program: &DataProgram,
    state: &Value,
    action: &str,
    params: &[(&str, &str)],
) -> (Value, reforge_investigate::DataOutcome) {
    let params: BTreeMap<String, Value> = params
        .iter()
        .map(|(key, value)| ((*key).to_string(), json!(value)))
        .collect();
    program.execute(state, action, &params)
}

fn id_value(outputs: &BTreeMap<String, Value>, key: &str) -> Option<i64> {
    outputs.get(key).and_then(Value::as_i64)
}
