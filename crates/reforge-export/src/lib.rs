//! Export a compiled application as a standalone local-web project.

#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use reforge_compose::{compile_file, ApplicationIr, CompileResult};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Serialize)]
pub struct ExportReport {
    pub out_dir: String,
    pub ir_id: String,
    pub packages_copied: Vec<String>,
    pub runner: String,
    pub instructions: Vec<String>,
}

/// Export a composition into `out_dir` as an independently buildable project.
///
/// The exported tree vendors the ReForge runtime crates required to run the
/// application, copies capability packages, and writes a small runner binary.
pub fn export_local_web(
    composition_path: impl AsRef<Path>,
    out_dir: impl AsRef<Path>,
    workspace_root: impl AsRef<Path>,
) -> Result<ExportReport> {
    let composition_path = composition_path.as_ref();
    let out_dir = out_dir.as_ref();
    let workspace_root = workspace_root.as_ref();

    if out_dir.exists() {
        fs::remove_dir_all(out_dir).context("clear out_dir")?;
    }
    fs::create_dir_all(out_dir)?;

    let compiled = compile_file(composition_path).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    write_json(out_dir.join("application.ir.json"), &compiled.ir)?;

    let packages_dir = out_dir.join("packages");
    fs::create_dir_all(&packages_dir)?;
    let mut packages_copied = Vec::new();
    for (instance, pkg) in &compiled.packages {
        let dest = packages_dir.join(instance);
        copy_dir(&pkg.root, &dest)?;
        packages_copied.push(instance.clone());
    }

    // Vendor runtime crates for offline standalone build.
    let vendor = out_dir.join("vendor");
    fs::create_dir_all(&vendor)?;
    for crate_name in [
        "reforge-types",
        "reforge-canon",
        "reforge-expr",
        "reforge-behavior",
        "reforge-capability",
        "reforge-evidence",
        "reforge-dependency",
        "reforge-schema",
        "reforge-compose",
        "reforge-observe",
        "reforge-investigate",
        "reforge-runtime",
    ] {
        let src = workspace_root.join("crates").join(crate_name);
        let dest = vendor.join(crate_name);
        copy_dir(&src, &dest)?;
        // Patch path deps in vendored Cargo.toml to relative vendor paths.
        patch_vendored_cargo_toml(&dest.join("Cargo.toml"), crate_name)?;
    }

    write_exported_workspace(out_dir, &compiled)?;
    write_reexport_composition(out_dir, &compiled.ir)?;
    write_runner(out_dir, &compiled.ir)?;

    let report = ExportReport {
        out_dir: out_dir.display().to_string(),
        ir_id: compiled.ir.id.to_string(),
        packages_copied,
        runner: "reforge-exported-app".into(),
        instructions: vec![
            format!("cd {}", out_dir.display()),
            "cargo run --release".into(),
            "The runner starts a loopback HTTP server and opens no cloud services.".into(),
            "Data persists under ./data/app.sqlite relative to the export directory.".into(),
        ],
    };
    write_json(out_dir.join("export-report.json"), &report)?;
    Ok(report)
}

fn write_json<T: Serialize>(path: PathBuf, value: &T) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

fn copy_dir(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in WalkDir::new(src) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(src)?;
        if rel
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            anyhow::bail!("unsafe path in copy");
        }
        let target = dest.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            // Skip target/ build artifacts if present.
            if rel.components().any(|c| c.as_os_str() == "target") {
                continue;
            }
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn patch_vendored_cargo_toml(path: &Path, _name: &str) -> Result<()> {
    let text = fs::read_to_string(path)?;
    // workspace = true fields break outside workspace — rewrite minimally.
    let mut out = String::new();
    let mut in_pkg = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[package]") {
            in_pkg = true;
        } else if trimmed.starts_with('[') {
            in_pkg = false;
        }
        if in_pkg
            && (trimmed.starts_with("version.workspace")
                || trimmed.starts_with("edition.workspace")
                || trimmed.starts_with("license.workspace")
                || trimmed.starts_with("authors.workspace")
                || trimmed.starts_with("rust-version.workspace")
                || trimmed.starts_with("description.workspace"))
        {
            continue;
        }
        if trimmed.contains("{ workspace = true }") && trimmed.contains("reforge-") {
            // Replace with path dependency to sibling vendor crate.
            if let Some((key, _)) = trimmed.split_once('=') {
                let key = key.trim();
                let crate_name = key;
                out.push_str(&format!(
                    "{crate_name} = {{ path = \"../{crate_name}\" }}\n"
                ));
                continue;
            }
        }
        if trimmed.contains("workspace = true") && !trimmed.starts_with("reforge-") {
            // External crates: drop .workspace and use plain version from known set — keep as-is by expanding common ones.
            let replaced = trimmed
                .replace("{ workspace = true }", "\"1\"")
                .replace(".workspace = true", " = \"1\"");
            // Too lossy — instead skip and inject a fixed deps block later.
            let _ = replaced;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    // Simpler strategy: rewrite Cargo.toml entirely for vendored crates with fixed versions.
    let _ = out;
    let name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    let toml = vendored_crate_toml(name);
    fs::write(path, toml)?;
    Ok(())
}

fn vendored_crate_toml(name: &str) -> String {
    let deps = match name {
        "reforge-types" => r#"
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["preserve_order"] }
thiserror = "2"
semver = { version = "1", features = ["serde"] }
indexmap = { version = "2", features = ["serde"] }
chrono = { version = "0.4", default-features = false, features = ["clock", "serde", "std"] }
hex = "0.4"
"#
        .to_string(),
        "reforge-canon" => r#"
reforge-types = { path = "../reforge-types" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_json_canonicalizer = "0.3"
sha2 = "0.10"
hex = "0.4"
thiserror = "2"
"#
        .to_string(),
        "reforge-expr" => r#"
reforge-types = { path = "../reforge-types" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
indexmap = { version = "2", features = ["serde"] }
"#
        .to_string(),
        "reforge-behavior" => r#"
reforge-types = { path = "../reforge-types" }
reforge-expr = { path = "../reforge-expr" }
reforge-canon = { path = "../reforge-canon" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
indexmap = { version = "2", features = ["serde"] }
"#
        .to_string(),
        "reforge-evidence" => r#"
reforge-types = { path = "../reforge-types" }
reforge-canon = { path = "../reforge-canon" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
indexmap = { version = "2", features = ["serde"] }
hex = "0.4"
"#
        .to_string(),
        "reforge-dependency" => r#"
reforge-types = { path = "../reforge-types" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
indexmap = { version = "2", features = ["serde"] }
"#
        .to_string(),
        "reforge-schema" => r#"
reforge-types = { path = "../reforge-types" }
reforge-canon = { path = "../reforge-canon" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
indexmap = { version = "2", features = ["serde"] }
semver = { version = "1", features = ["serde"] }
"#
        .to_string(),
        "reforge-observe" => r#"
reforge-types = { path = "../reforge-types" }
reforge-canon = { path = "../reforge-canon" }
reforge-evidence = { path = "../reforge-evidence" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
indexmap = { version = "2", features = ["serde"] }
sha2 = "0.10"
hex = "0.4"
"#
        .to_string(),
        "reforge-investigate" => r#"
reforge-types = { path = "../reforge-types" }
reforge-canon = { path = "../reforge-canon" }
reforge-observe = { path = "../reforge-observe" }
reforge-behavior = { path = "../reforge-behavior" }
reforge-evidence = { path = "../reforge-evidence" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
indexmap = { version = "2", features = ["serde"] }
sha2 = "0.10"
hex = "0.4"
"#
        .to_string(),
        "reforge-capability" => r#"
reforge-types = { path = "../reforge-types" }
reforge-canon = { path = "../reforge-canon" }
reforge-behavior = { path = "../reforge-behavior" }
reforge-evidence = { path = "../reforge-evidence" }
reforge-investigate = { path = "../reforge-investigate" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
indexmap = { version = "2", features = ["serde"] }
walkdir = "2"
semver = { version = "1", features = ["serde"] }
"#
        .to_string(),
        "reforge-compose" => r#"
reforge-types = { path = "../reforge-types" }
reforge-canon = { path = "../reforge-canon" }
reforge-capability = { path = "../reforge-capability" }
reforge-dependency = { path = "../reforge-dependency" }
reforge-behavior = { path = "../reforge-behavior" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
indexmap = { version = "2", features = ["serde"] }
anyhow = "1"
"#
        .to_string(),
        "reforge-runtime" => r#"
reforge-types = { path = "../reforge-types" }
reforge-behavior = { path = "../reforge-behavior" }
reforge-capability = { path = "../reforge-capability" }
reforge-compose = { path = "../reforge-compose" }
reforge-canon = { path = "../reforge-canon" }
reforge-investigate = { path = "../reforge-investigate" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
indexmap = { version = "2", features = ["serde"] }
anyhow = "1"
rusqlite = { version = "0.32", features = ["bundled"] }
"#
        .to_string(),
        _ => String::new(),
    };
    format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"
description = "Vendored ReForge crate for standalone export"

[dependencies]
{deps}
"#
    )
}

fn write_exported_workspace(out_dir: &Path, compiled: &CompileResult) -> Result<()> {
    let members = compiled
        .packages
        .keys()
        .map(|k| format!("# package instance {k}"))
        .collect::<Vec<_>>()
        .join("\n");
    let cargo = format!(
        r#"[workspace]
resolver = "2"
members = ["app"]
# Vendored runtime crates are path-deps of app, not workspace members.
# {members}

[workspace.package]
version = "0.1.0"
edition = "2021"
"#
    );
    fs::write(out_dir.join("Cargo.toml"), cargo)?;
    fs::write(
        out_dir.join("README.md"),
        r#"# Exported ReForge Application

This directory was generated by `reforge export`.

## Run

```bash
cargo run --release
```

The runner is an interactive local CLI. Persistent state is stored in `./data/app.sqlite`.

## Contents

- `application.ir.json` — compiled Application IR
- `packages/` — capability packages
- `vendor/` — ReForge runtime crates required to execute
- `app/` — thin command runner
"#,
    )?;
    Ok(())
}

fn write_runner(out_dir: &Path, ir: &ApplicationIr) -> Result<()> {
    let app_dir = out_dir.join("app");
    fs::create_dir_all(app_dir.join("src"))?;
    fs::write(
        app_dir.join("Cargo.toml"),
        r#"[package]
name = "reforge-exported-app"
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"

[[bin]]
name = "reforge-exported-app"
path = "src/main.rs"

[dependencies]
reforge-types = { path = "../vendor/reforge-types" }
reforge-compose = { path = "../vendor/reforge-compose" }
reforge-runtime = { path = "../vendor/reforge-runtime" }
reforge-capability = { path = "../vendor/reforge-capability" }
serde_json = "1"
anyhow = "1"
indexmap = { version = "2", features = ["serde"] }
"#,
    )?;
    let _ = ir;
    fs::write(
        app_dir.join("src/main.rs"),
        r#"//! Standalone runner for an exported ReForge application.
use anyhow::Result;
use indexmap::IndexMap;
use reforge_compose::compile_file;
use reforge_runtime::AppRuntime;
use reforge_types::TypedValue;
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;

fn json_to_typed(value: serde_json::Value) -> TypedValue {
    match value {
        serde_json::Value::Null => TypedValue::Null,
        serde_json::Value::Bool(value) => TypedValue::Bool(value),
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                TypedValue::Int(value)
            } else {
                TypedValue::Float(number.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(value) => TypedValue::String(value),
        serde_json::Value::Array(items) => {
            TypedValue::List(items.into_iter().map(json_to_typed).collect())
        }
        serde_json::Value::Object(map) => TypedValue::Map(
            map.into_iter()
                .map(|(key, value)| (key, json_to_typed(value)))
                .collect(),
        ),
    }
}

fn main() -> Result<()> {
    let mut root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    if root.ends_with("app") {
        root = root.parent().unwrap().to_path_buf();
    }
    let composition = root.join("composition.reexport.json");
    let compiled = compile_file(&composition).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let app_name = compiled.ir.name.clone();
    let mut runtime = AppRuntime::from_compile(compiled)?;
    let db = root.join("data/app.sqlite");
    if let Some(parent) = db.parent() {
        std::fs::create_dir_all(parent)?;
    }
    runtime.attach_sqlite(&db)?;
    println!("ReForge exported app: {app_name}");
    println!("DB: {}", db.display());
    println!("Commands: <instance> <action> [json-params] | snapshot | quit");
    let stdin = io::stdin();
    loop {
        print!("> ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        if stdin.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "quit" || line == "exit" {
            break;
        }
        if line == "snapshot" {
            println!("{}", serde_json::to_string_pretty(&runtime.snapshot())?);
            continue;
        }
        let parts: Vec<_> = line.splitn(3, ' ').collect();
        if parts.len() < 2 {
            eprintln!("usage: <instance> <action> [json]");
            continue;
        }
        let params: IndexMap<String, TypedValue> = if parts.len() == 3 {
            let raw: IndexMap<String, serde_json::Value> = serde_json::from_str(parts[2])?;
            raw.into_iter()
                .map(|(key, value)| (key, json_to_typed(value)))
                .collect()
        } else {
            IndexMap::new()
        };
        match runtime.dispatch(parts[0], parts[1], params) {
            Ok(r) => println!("{}", serde_json::to_string_pretty(&r)?),
            Err(e) => eprintln!("error: {e}"),
        }
    }
    Ok(())
}
"#,
    )?;

    // Also need reforge-types in runner - it's transitive. Add composition.reexport.json writer in export_local_web.
    Ok(())
}

/// Write a re-export composition that points package_path at ./packages/<instance>.
pub fn write_reexport_composition(out_dir: &Path, ir: &ApplicationIr) -> Result<()> {
    use reforge_compose::{CapabilityInstanceDecl, CompositionManifest};
    use reforge_types::{Id, SchemaVersion};
    let mut capabilities = Vec::new();
    for (instance, inst) in &ir.instances {
        capabilities.push(CapabilityInstanceDecl {
            instance: Id::new(instance.clone()).map_err(|e| anyhow::anyhow!(e.to_string()))?,
            package_path: format!("packages/{instance}"),
            alias: Some(inst.capability_id.to_string()),
        });
    }
    let manifest = CompositionManifest {
        schema_version: SchemaVersion::current(),
        id: ir.id.clone(),
        name: ir.name.clone(),
        description: ir.description.clone(),
        capabilities,
        bindings: ir.bindings.clone(),
        relationships: ir.relationships.clone(),
        customization: ir.customization.clone(),
        runtime: ir.runtime.clone(),
        verification: ir.verification.clone(),
    };
    write_json(out_dir.join("composition.reexport.json"), &manifest)?;
    Ok(())
}
