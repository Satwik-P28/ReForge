//! Execute a serialized data-aware reconstruction without the reference app.
//!
//! This runner consumes only a `DataProgram` JSON file and its own persisted
//! state. It never starts, imports, or reads the original application.

use anyhow::{Context, Result};
use reforge_investigate::{DataProgram, ExecutionMemory};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn usage() -> &'static str {
    "usage: reforge_data_runtime --program <program.json> --state <state.json> [--inspect | --action <name> --params <json-object>]"
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut program_path: Option<PathBuf> = None;
    let mut state_path: Option<PathBuf> = None;
    let mut action: Option<String> = None;
    let mut params_text = "{}".to_string();
    let mut inspect = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--program" => program_path = Some(PathBuf::from(args.next().context(usage())?)),
            "--state" => state_path = Some(PathBuf::from(args.next().context(usage())?)),
            "--action" => action = Some(args.next().context(usage())?),
            "--params" => params_text = args.next().context(usage())?,
            "--inspect" => inspect = true,
            "--help" | "-h" => {
                println!("{}", usage());
                return Ok(());
            }
            other => anyhow::bail!("unknown argument {other:?}\n{}", usage()),
        }
    }
    let program_path = program_path.context(usage())?;
    let state_path = state_path.context(usage())?;
    let text = std::fs::read_to_string(&program_path)
        .with_context(|| format!("read program {}", program_path.display()))?;
    let program: DataProgram =
        serde_json::from_str(&text).context("parse serialized DataProgram")?;
    let memory_path = state_path.with_extension("memory.json");
    let mut memory: ExecutionMemory = if memory_path.exists() {
        serde_json::from_str(&std::fs::read_to_string(&memory_path)?)
            .with_context(|| format!("parse memory {}", memory_path.display()))?
    } else {
        ExecutionMemory::default()
    };
    let state: Value = if state_path.exists() {
        serde_json::from_str(&std::fs::read_to_string(&state_path)?)
            .with_context(|| format!("parse state {}", state_path.display()))?
    } else {
        program.initial_state.clone()
    };

    if inspect {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "target_id": program.target_id,
                "initial_state": program.initial_state,
                "current_state": state,
                "rules": program.rules.len(),
                "collections": program.collections,
                "memory": memory,
            }))?
        );
        return Ok(());
    }

    let action = action.context(usage())?;
    let raw_params: BTreeMap<String, Value> =
        serde_json::from_str(&params_text).context("parse --params as a JSON object")?;
    let result = program.execute_tracked(&state, &memory, &action, &raw_params);
    let next = result.state;
    let outcome = result.outcome;
    memory = result.memory;
    if let Some(parent) = state_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(&state_path, serde_json::to_string_pretty(&next)?)?;
    if memory.is_empty() {
        let _ = std::fs::remove_file(&memory_path);
    } else {
        std::fs::write(&memory_path, serde_json::to_string_pretty(&memory)?)?;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "action": action,
            "params": raw_params,
            "outcome": outcome,
            "state": next,
            "memory": memory,
        }))?
    );
    Ok(())
}
