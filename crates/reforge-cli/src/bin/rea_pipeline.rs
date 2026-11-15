//! Reproducible Phase 7 pipeline: REA -> guided investigation -> extraction.
use reforge_extract::{extract_capability, ExtractionConfig};
use reforge_investigate::{
    investigate, synthesize_data_program, CliTargetSpec, DataStatus, InvestigationConfig,
    PolicyKind,
};
use reforge_rea::{ReaConfig, ReaProvider};
use serde_json::json;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 6 {
        anyhow::bail!("usage: rea_pipeline <rea-bin> <source> <target.json> <out-dir> <budget>");
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
    let provider = ReaProvider::detect(&config)?;
    let analysis = provider.analyze_javascript(&source, &out.join("rea"), &config)?;

    let spec = CliTargetSpec::load(&spec_path)?;
    let work = out.join("work");
    let mut target = spec.build(Some(&work))?;
    let guidance = analysis.projection.guidance();
    let mut investigation = InvestigationConfig::default()
        .with_policy(PolicyKind::Hypothesis)
        .with_budget(budget)
        .with_seed(1);
    investigation.evidence_guidance = Some(guidance.clone());
    let record = investigate(&mut target, &investigation)?;
    std::fs::write(out.join("investigation.json"), record.to_json()?)?;

    let data = synthesize_data_program(&record);
    std::fs::write(
        out.join("data-program.json"),
        serde_json::to_vec_pretty(&data)?,
    )?;
    let mut replay_total = 0usize;
    let mut replay_matched = 0usize;
    for step in &record.steps {
        let (Some(before), Some(after)) = (&step.state_before, &step.state_after) else {
            continue;
        };
        let mut params = serde_json::Map::new();
        for (key, value) in step.params.split(',').filter_map(|s| s.split_once('=')) {
            params.insert(key.trim().to_string(), serde_json::json!(value));
        }
        let (next, outcome) = data.execute(
            before,
            &step.action,
            &params
                .into_iter()
                .collect::<std::collections::BTreeMap<_, _>>(),
        );
        replay_total += 1;
        let status_matches = matches!(
            (step.outcome, outcome.status),
            (
                reforge_investigate::OutcomeKind::Success,
                DataStatus::Success
            ) | (
                reforge_investigate::OutcomeKind::Rejected,
                DataStatus::Rejected
            )
        );
        if status_matches && &next == after {
            replay_matched += 1;
        }
    }
    let data_summary = json!({
        "collections": data.collections,
        "rules": data.rules.len(),
        "exact_rules": data.rules.iter().filter(|r| r.exact()).count(),
        "support": data.rules.iter().map(|r| r.support).sum::<u32>(),
        "reproduced": data.rules.iter().map(|r| r.reproduced).sum::<u32>(),
        "unsupported": data.unsupported,
        "observed_replay_matched": replay_matched,
        "observed_replay_total": replay_total,
        "initial_state": data.initial_state,
        "rule_summary": data.rules.iter().map(|r| json!({
            "id": r.id, "action": r.action, "branch": r.branch, "support": r.support,
            "reproduced": r.reproduced, "guards": r.guards, "effects": r.effects
        })).collect::<Vec<_>>(),
    });
    std::fs::write(
        out.join("data-summary.json"),
        serde_json::to_vec_pretty(&data_summary)?,
    )?;

    let mut extraction = ExtractionConfig::new(out.join("packages"));
    extraction.rea_projection = Some(analysis.projection.clone());
    extraction.rea_raw_path = Some(analysis.raw_path.clone());
    let report = extract_capability(&record, &extraction)?;
    let summary = json!({
        "rea_evidence_id": analysis.evidence_id,
        "rea_version": provider.version().version,
        "rea_elapsed_ms": analysis.elapsed_ms,
        "rea_raw_sha256": analysis.raw_sha256,
        "rea_raw_bytes": analysis.raw_bytes,
        "hint_counts": analysis.projection.hint_counts,
        "investigation": {
            "actions": record.coverage.actions_used,
            "transitions": record.coverage.transitions,
            "classes": record.coverage.classes,
            "guidance_evidence": record.evidence_guidance.as_ref().map(|g| g.evidence_ids.clone()),
        },
        "data_program": data_summary,
        "package": report.out_dir,
        "package_id": report.package_id,
        "extracted_files": report.files,
        "partial_coverage": report.partial_coverage,
    });
    std::fs::write(
        out.join("summary.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
