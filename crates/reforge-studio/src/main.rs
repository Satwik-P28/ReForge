//! ReForge Studio Lite — loopback-only local UI backed by real engine APIs.

use anyhow::{Context, Result};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::Parser;
use indexmap::IndexMap;
use reforge::compose::compile_file;
use reforge::runtime::AppRuntime;
use reforge::types::TypedValue;
use reforge::verify::{verify_expectation, ComposeExpectation};
use reforge::{export_local_web, load_package};
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Parser, Debug)]
#[command(
    name = "reforge-studio",
    about = "ReForge Studio Lite (127.0.0.1 only)"
)]
struct Args {
    /// Bind address (loopback only enforced).
    #[arg(long, default_value = "127.0.0.1:7420")]
    bind: String,
    /// Optional composition to preload.
    #[arg(long)]
    composition: Option<PathBuf>,
    /// Workspace root for export vendoring.
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

struct AppState {
    workspace: PathBuf,
    runtime: Mutex<Option<AppRuntime>>,
    composition_path: Mutex<Option<PathBuf>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let addr: SocketAddr = args.bind.parse().context("bind address")?;
    if !addr.ip().is_loopback() {
        anyhow::bail!(
            "Studio must bind to loopback (127.0.0.1/::1); refused {}",
            addr
        );
    }

    let state = Arc::new(AppState {
        workspace: args.workspace.canonicalize().unwrap_or(args.workspace),
        runtime: Mutex::new(None),
        composition_path: Mutex::new(args.composition.clone()),
    });

    if let Some(path) = &args.composition {
        let compiled = compile_file(path).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let mut rt =
            AppRuntime::from_compile(compiled).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let db = state.workspace.join(".reforge/studio-data.sqlite");
        rt.attach_sqlite(&db)?;
        *state.runtime.lock().await = Some(rt);
    }

    let app = Router::new()
        .route("/", get(index))
        .route(
            "/api/health",
            get(|| async { Json(serde_json::json!({"ok": true})) }),
        )
        .route("/api/compose/load", post(load_composition))
        .route("/api/snapshot", get(snapshot))
        .route("/api/dispatch", post(dispatch))
        .route("/api/package/inspect", get(inspect_package_api))
        .route("/api/verify", post(verify_api))
        .route("/api/export", post(export_api))
        .with_state(state);

    println!("ReForge Studio Lite on http://{addr} (loopback only)");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

#[derive(Deserialize)]
struct LoadBody {
    path: String,
}

async fn load_composition(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoadBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let path = PathBuf::from(&body.path);
    let compiled = compile_file(&path).map_err(map_err)?;
    let mut rt = AppRuntime::from_compile(compiled).map_err(map_err)?;
    let db = state.workspace.join(".reforge/studio-data.sqlite");
    rt.attach_sqlite(&db).map_err(map_err)?;
    let snap = rt.snapshot();
    *state.runtime.lock().await = Some(rt);
    *state.composition_path.lock().await = Some(path);
    Ok(Json(snap))
}

async fn snapshot(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let guard = state.runtime.lock().await;
    let rt = guard
        .as_ref()
        .ok_or((StatusCode::BAD_REQUEST, "no composition loaded".into()))?;
    Ok(Json(rt.snapshot()))
}

#[derive(Deserialize)]
struct DispatchBody {
    instance: String,
    action: String,
    #[serde(default)]
    parameters: IndexMap<String, TypedValue>,
}

async fn dispatch(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DispatchBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut guard = state.runtime.lock().await;
    let rt = guard
        .as_mut()
        .ok_or((StatusCode::BAD_REQUEST, "no composition loaded".into()))?;
    let result = rt
        .dispatch(&body.instance, &body.action, body.parameters)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(serde_json::to_value(result).unwrap()))
}

#[derive(Deserialize)]
struct InspectQuery {
    path: String,
}

async fn inspect_package_api(
    Query(q): Query<InspectQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pkg = load_package(&q.path).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let info = reforge::inspect_package(&pkg);
    Ok(Json(serde_json::to_value(info).unwrap()))
}

async fn verify_api(
    State(state): State<Arc<AppState>>,
    Json(expectation): Json<ComposeExpectation>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut guard = state.runtime.lock().await;
    let rt = guard
        .as_mut()
        .ok_or((StatusCode::BAD_REQUEST, "no composition loaded".into()))?;
    let report = verify_expectation(rt, &expectation).map_err(map_err)?;
    Ok(Json(serde_json::to_value(report).unwrap()))
}

#[derive(Deserialize)]
struct ExportBody {
    out: String,
}

async fn export_api(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ExportBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let path = state
        .composition_path
        .lock()
        .await
        .clone()
        .ok_or((StatusCode::BAD_REQUEST, "no composition loaded".into()))?;
    let report = export_local_web(&path, &body.out, &state.workspace).map_err(map_err)?;
    Ok(Json(serde_json::to_value(report).unwrap()))
}

fn map_err(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, e.to_string())
}
