//! Reproducible Phase 7 pipeline: REA -> guided investigation -> extraction.
use reforge_extract::{extract_capability, ExtractionConfig};
use reforge_investigate::{
    investigate, CliTargetSpec, InvestigationConfig, PolicyKind,
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
        "package": report.out_dir,
        "package_id": report.package_id,
        "extracted_files": report.files,
        "partial_coverage": report.partial_coverage,
    });
    std::fs::write(out.join("summary.json"), serde_json::to_vec_pretty(&summary)?)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
