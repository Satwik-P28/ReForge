use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use indexmap::IndexMap;
use reforge::behavior::{replay_sequence, verify_replay, ActionSequence};
use reforge::capability::{export_package, inspect_package, load_package};
use reforge::compose::compile_file;
use reforge::dependency::DependencyGraph;
use reforge::export::export_local_web;
use reforge::runtime::AppRuntime;
use reforge::schema::migrate_json_str;
use reforge::types::*;
use reforge::verify::{verify_expectation, ComposeExpectation};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

#[derive(Parser, Debug)]
#[command(
    name = "reforge",
    version,
    about = "ReForge — deterministic capability composition"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Init {
        name: String,
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    Package {
        #[command(subcommand)]
        cmd: PackageCmd,
    },
    Twin {
        #[command(subcommand)]
        cmd: TwinCmd,
    },
    Evidence {
        #[command(subcommand)]
        cmd: EvidenceCmd,
    },
    Deps {
        #[command(subcommand)]
        cmd: DepsCmd,
    },
    Schema {
        #[command(subcommand)]
        cmd: SchemaCmd,
    },
    Compose {
        #[arg(long)]
        config: PathBuf,
    },
    Verify {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        expectation: PathBuf,
    },
    Export {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "local-web")]
        target: String,
        #[arg(long, default_value = ".")]
        workspace: PathBuf,
    },
    Studio {
        #[arg(long)]
        composition: Option<PathBuf>,
        #[arg(long, default_value = "127.0.0.1:7420")]
        bind: String,
    },
    Run {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        instance: String,
        #[arg(long)]
        action: String,
        #[arg(long, default_value = "{}")]
        params: String,
        #[arg(long)]
        db: Option<PathBuf>,
    },
    Bench,
}

#[derive(Subcommand, Debug)]
enum PackageCmd {
    Validate {
        path: PathBuf,
    },
    Inspect {
        path: PathBuf,
    },
    Export {
        path: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum TwinCmd {
    Validate {
        path: PathBuf,
    },
    Replay {
        path: PathBuf,
        #[arg(long)]
        trace: PathBuf,
    },
    Execute {
        path: PathBuf,
        #[arg(long)]
        action: String,
        #[arg(long, default_value = "{}")]
        params: String,
    },
}

#[derive(Subcommand, Debug)]
enum EvidenceCmd {
    Verify {
        path: PathBuf,
    },
    Explain {
        path: PathBuf,
        #[arg(long)]
        claim: String,
    },
}

#[derive(Subcommand, Debug)]
enum DepsCmd {
    Check { paths: Vec<PathBuf> },
}

#[derive(Subcommand, Debug)]
enum SchemaCmd {
    Migrate { path: PathBuf },
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init { name, dir } => {
            let root = dir.join(&name);
            fs::create_dir_all(root.join("capabilities"))?;
            let composition = serde_json::json!({
                "schema_version": "1.0.0",
                "id": format!("app.{}", name.replace(' ', "-")),
                "name": name,
                "description": "Created by reforge init",
                "capabilities": [],
                "bindings": [],
                "relationships": [],
                "customization": {
                    "app_title": name,
                    "theme": "light",
                    "layout": "tabs",
                    "nav_order": [],
                    "labels": {},
                    "hidden_views": []
                },
                "runtime": { "persistence": "sqlite", "export_target": "local-web" },
                "verification": []
            });
            fs::write(
                root.join("composition.json"),
                serde_json::to_string_pretty(&composition)?,
            )?;
            fs::write(
                root.join("README.md"),
                format!("# {name}\n\nAdd capabilities and run `reforge compose --config composition.json`.\n"),
            )?;
            println!("created {}", root.display());
            Ok(ExitCode::SUCCESS)
        }
        Commands::Package { cmd } => match cmd {
            PackageCmd::Validate { path } => {
                let pkg = load_package(&path)?;
                let mut report = pkg.manifest.validate_structure();
                if let Some(twin) = &pkg.twin {
                    report.merge(twin.validate_structure());
                }
                print_json(&report)?;
                Ok(exit_ok(report.is_ok()))
            }
            PackageCmd::Inspect { path } => {
                let pkg = load_package(&path)?;
                print_json(&inspect_package(&pkg))?;
                Ok(ExitCode::SUCCESS)
            }
            PackageCmd::Export { path, out } => {
                let pkg = load_package(&path)?;
                let digest = export_package(&pkg, &out)?;
                print_json(&serde_json::json!({"out": out, "digest": digest.hex}))?;
                Ok(ExitCode::SUCCESS)
            }
        },
        Commands::Twin { cmd } => match cmd {
            TwinCmd::Validate { path } => {
                let pkg = load_package(&path)?;
                let twin = pkg.twin.as_ref().context("no twin.json")?;
                let report = twin.validate_structure();
                print_json(&report)?;
                Ok(exit_ok(report.is_ok()))
            }
            TwinCmd::Replay { path, trace } => {
                let pkg = load_package(&path)?;
                let twin = pkg.twin.context("no twin.json")?;
                let trace_text = fs::read_to_string(&trace)?;
                if let Ok(expected) = serde_json::from_str::<ExecutionTrace>(&trace_text) {
                    let result = verify_replay(twin, &expected).map_err(|e| anyhow::anyhow!(e))?;
                    print_json(&result)?;
                    Ok(exit_ok(result.matched))
                } else {
                    let seq: ActionSequence = serde_json::from_str(&trace_text)?;
                    let actual = replay_sequence(twin, &seq).map_err(|e| anyhow::anyhow!(e))?;
                    print_json(&actual)?;
                    Ok(ExitCode::SUCCESS)
                }
            }
            TwinCmd::Execute {
                path,
                action,
                params,
            } => {
                let pkg = load_package(&path)?;
                let mut engine = pkg.open_engine().map_err(|e| anyhow::anyhow!(e))?;
                let parameters = parse_param_map(&params)?;
                let step = engine.execute(&action, parameters);
                print_json(&step)?;
                Ok(exit_ok(matches!(step.result, StepResult::Success)))
            }
        },
        Commands::Evidence { cmd } => match cmd {
            EvidenceCmd::Verify { path } => {
                let mut pkg = load_package(&path)?;
                let results = pkg.evidence.verify_all_integrity();
                print_json(&results)?;
                let bad = results.iter().any(|(_, s)| {
                    matches!(s, IntegrityStatus::Corrupted | IntegrityStatus::Missing)
                });
                Ok(exit_ok(!bad))
            }
            EvidenceCmd::Explain { path, claim } => {
                let pkg = load_package(&path)?;
                let explanation = pkg
                    .evidence
                    .explain(&claim)
                    .map_err(|e| anyhow::anyhow!(e))?;
                print_json(&explanation)?;
                Ok(ExitCode::SUCCESS)
            }
        },
        Commands::Deps { cmd } => match cmd {
            DepsCmd::Check { paths } => {
                if paths.is_empty() {
                    bail!("provide at least one package path");
                }
                let mut manifests = Vec::new();
                for p in paths {
                    manifests.push(load_package(p)?.manifest);
                }
                let report = DependencyGraph::from_packages(&manifests).validate();
                print_json(&report)?;
                Ok(exit_ok(report.is_compatible()))
            }
        },
        Commands::Schema { cmd } => match cmd {
            SchemaCmd::Migrate { path } => {
                let text = fs::read_to_string(&path)?;
                let (doc, digest) = migrate_json_str(&text)?;
                print_json(&serde_json::json!({"digest": digest.hex, "document": doc}))?;
                Ok(ExitCode::SUCCESS)
            }
        },
        Commands::Compose { config } => {
            let compiled = compile_file(&config).map_err(|e| anyhow::anyhow!(e.to_string()))?;
            print_json(&compiled.ir)?;
            Ok(exit_ok(compiled.ir.validate_structure().is_ok()))
        }
        Commands::Verify {
            config,
            expectation,
        } => {
            let compiled = compile_file(&config).map_err(|e| anyhow::anyhow!(e.to_string()))?;
            let mut rt = AppRuntime::from_compile(compiled).map_err(|e| anyhow::anyhow!(e))?;
            rt.attach_memory_db()?;
            let exp = load_expectation(&expectation)?;
            let report = verify_expectation(&mut rt, &exp).map_err(|e| anyhow::anyhow!(e))?;
            print_json(&report)?;
            Ok(exit_ok(report.status == VerificationStatus::Passed))
        }
        Commands::Export {
            config,
            out,
            target,
            workspace,
        } => {
            if target != "local-web" {
                bail!("unsupported export target '{target}' (v0.1 supports local-web)");
            }
            let report = export_local_web(&config, &out, &workspace)?;
            print_json(&report)?;
            Ok(ExitCode::SUCCESS)
        }
        Commands::Studio { composition, bind } => {
            let mut cmd = Command::new("cargo");
            cmd.args(["run", "-p", "reforge-studio", "--", "--bind", &bind]);
            if let Some(c) = composition {
                cmd.arg("--composition").arg(c);
            }
            let status = cmd.status().context("launch studio")?;
            Ok(if status.success() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        Commands::Run {
            config,
            instance,
            action,
            params,
            db,
        } => {
            let compiled = compile_file(&config).map_err(|e| anyhow::anyhow!(e.to_string()))?;
            let mut rt = AppRuntime::from_compile(compiled).map_err(|e| anyhow::anyhow!(e))?;
            if let Some(db) = db {
                rt.attach_sqlite(db)?;
            } else {
                rt.attach_memory_db()?;
            }
            let parameters = parse_param_map(&params)?;
            let result = rt
                .dispatch(&instance, &action, parameters)
                .map_err(|e| anyhow::anyhow!(e))?;
            print_json(&result)?;
            Ok(exit_ok(matches!(result.step.result, StepResult::Success)))
        }
        Commands::Bench => {
            println!("Run: cargo run -p reforge-bench --release");
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn exit_ok(ok: bool) -> ExitCode {
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// Accept either tagged TypedValue JSON or plain JSON literals in parameter maps.
fn parse_param_map(text: &str) -> Result<IndexMap<String, TypedValue>> {
    let raw: IndexMap<String, serde_json::Value> = serde_json::from_str(text)?;
    let mut out = IndexMap::new();
    for (k, v) in raw {
        out.insert(k, json_to_typed(v)?);
    }
    Ok(out)
}

fn json_to_typed(v: serde_json::Value) -> Result<TypedValue> {
    if let Ok(tv) = serde_json::from_value::<TypedValue>(v.clone()) {
        return Ok(tv);
    }
    Ok(match v {
        serde_json::Value::Null => TypedValue::Null,
        serde_json::Value::Bool(b) => TypedValue::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                TypedValue::Int(i)
            } else if let Some(u) = n.as_u64() {
                TypedValue::Int(i64::try_from(u).unwrap_or(i64::MAX))
            } else {
                TypedValue::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => TypedValue::String(s),
        serde_json::Value::Array(items) => {
            let mut list = Vec::with_capacity(items.len());
            for item in items {
                list.push(json_to_typed(item)?);
            }
            TypedValue::List(list)
        }
        serde_json::Value::Object(map) => {
            let mut out = IndexMap::new();
            for (k, val) in map {
                out.insert(k, json_to_typed(val)?);
            }
            TypedValue::Map(out)
        }
    })
}

fn load_expectation(path: &PathBuf) -> Result<ComposeExpectation> {
    let text = fs::read_to_string(path)?;
    if let Ok(exp) = serde_json::from_str::<ComposeExpectation>(&text) {
        return Ok(exp);
    }
    // Allow plain JSON literals inside step parameters (README-friendly).
    let mut v: serde_json::Value = serde_json::from_str(&text)?;
    if let Some(steps) = v.get_mut("steps").and_then(|s| s.as_array_mut()) {
        for step in steps {
            if let Some(params) = step.get_mut("parameters").and_then(|p| p.as_object_mut()) {
                let keys: Vec<_> = params.keys().cloned().collect();
                for k in keys {
                    let raw = params.remove(&k).unwrap();
                    let tv = json_to_typed(raw)?;
                    params.insert(k, serde_json::to_value(tv)?);
                }
            }
        }
    }
    Ok(serde_json::from_value(v)?)
}
