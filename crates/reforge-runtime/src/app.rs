use crate::links::{relationship_key, LinkStore};
use crate::persist::{PersistError, Store};
use indexmap::IndexMap;
use reforge_behavior::TwinEngine;
use reforge_capability::LoadedPackage;
use reforge_compose::{ApplicationIr, BindingDecl, CompileResult, EndpointKind, OnDeletePolicy};
use reforge_types::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::Path;
use thiserror::Error;

const MAX_EVENT_DEPTH: usize = 32;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("unknown instance '{0}'")]
    UnknownInstance(String),
    #[error("instance '{0}' has no twin")]
    NoTwin(String),
    #[error("persist: {0}")]
    Persist(#[from] PersistError),
    #[error("rejected: {0}")]
    Rejected(String),
    #[error("event loop limit exceeded")]
    EventLoopLimit,
    #[error("{0}")]
    Other(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DispatchResult {
    pub instance: String,
    pub action: String,
    pub step: TraceStep,
    #[serde(default)]
    pub propagated: Vec<DispatchResult>,
    #[serde(default)]
    pub link_diagnostics: Vec<Diagnostic>,
}

pub struct AppRuntime {
    pub ir: ApplicationIr,
    engines: IndexMap<String, TwinEngine>,
    #[allow(dead_code)]
    packages: IndexMap<String, LoadedPackage>,
    pub links: LinkStore,
    store: Option<Store>,
}

impl AppRuntime {
    pub fn from_compile(compiled: CompileResult) -> Result<Self, RuntimeError> {
        let mut engines = IndexMap::new();
        for (name, pkg) in &compiled.packages {
            let twin = pkg
                .twin
                .clone()
                .ok_or_else(|| RuntimeError::NoTwin(name.clone()))?;
            let engine = TwinEngine::new(twin).map_err(|e| RuntimeError::Other(e.to_string()))?;
            engines.insert(name.clone(), engine);
        }
        let mut links = LinkStore::default();
        for rel in &compiled.ir.relationships {
            links.ensure_table(&relationship_key(&rel.id));
        }
        Ok(Self {
            ir: compiled.ir,
            engines,
            packages: compiled.packages,
            links,
            store: None,
        })
    }

    pub fn attach_sqlite(&mut self, path: impl AsRef<Path>) -> Result<(), RuntimeError> {
        let store = Store::open(path)?;
        // Restore if present.
        for name in self.engines.keys().cloned().collect::<Vec<_>>() {
            if let Some((state, vars)) = store.load_instance(&name)? {
                if let Some(engine) = self.engines.get_mut(&name) {
                    // Reconstruct runtime by replaying is hard; restore snapshot fields.
                    // TwinEngine doesn't expose setters — use a restore helper via reset+inject.
                    restore_engine_state(engine, state, vars)?;
                }
            }
        }
        self.links = store.load_links()?;
        for rel in &self.ir.relationships {
            self.links.ensure_table(&relationship_key(&rel.id));
        }
        self.store = Some(store);
        Ok(())
    }

    pub fn attach_memory_db(&mut self) -> Result<(), RuntimeError> {
        self.store = Some(Store::open_memory()?);
        Ok(())
    }

    pub fn persist_all(&self) -> Result<(), RuntimeError> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        for (name, engine) in &self.engines {
            store.save_instance(
                name,
                &engine.runtime().current_state,
                &engine.runtime().variables,
            )?;
        }
        store.save_links(&self.links)?;
        Ok(())
    }

    pub fn snapshot(&self) -> serde_json::Value {
        let mut instances = serde_json::Map::new();
        for (name, engine) in &self.engines {
            instances.insert(
                name.clone(),
                serde_json::json!({
                    "current_state": engine.runtime().current_state,
                    "variables": engine.runtime().variables,
                    "coverage": engine.coverage(),
                }),
            );
        }
        serde_json::json!({
            "app": self.ir.name,
            "instances": instances,
            "links": self.links,
            "customization": self.ir.customization,
        })
    }

    /// Dispatch a command/query to an instance, then process emitted events.
    pub fn dispatch(
        &mut self,
        instance: &str,
        action: &str,
        parameters: IndexMap<String, TypedValue>,
    ) -> Result<DispatchResult, RuntimeError> {
        // Special composition commands for relationships.
        if action == "reforge.link" {
            return self.cmd_link(parameters);
        }
        if action == "reforge.unlink" {
            return self.cmd_unlink(parameters);
        }
        if action == "reforge.links_for" {
            return self.cmd_links_for(parameters);
        }

        // Pre-delete policy check for delete_task / similar when relationships target this instance.
        if action.starts_with("delete_") {
            if let Some(entity_id) = parameters.get("id").and_then(|v| v.as_str()) {
                for rel in self.ir.relationships.clone() {
                    if rel.to_instance.as_str() == instance {
                        let policy = rel.on_delete;
                        let key = relationship_key(&rel.id);
                        let result = self.links.apply_on_delete(&key, entity_id, policy);
                        if !result.ok {
                            return Err(RuntimeError::Rejected(
                                result
                                    .diagnostics
                                    .first()
                                    .map(|d| d.message.clone())
                                    .unwrap_or_else(|| "delete rejected by link policy".into()),
                            ));
                        }
                        // Unlink policy already cleared edges inside apply_on_delete.
                        if matches!(policy, OnDeletePolicy::Cascade) {
                            // Cascade requires an explicit from-side delete action name in
                            // relationship.description prefixed with "cascade_delete_action="
                            // or defaults to rejecting with a diagnostic (no domain hardcoding).
                            let action_name = rel
                                .description
                                .as_deref()
                                .and_then(|d| {
                                    d.split("cascade_delete_action=")
                                        .nth(1)
                                        .map(|s| s.split_whitespace().next().unwrap_or(""))
                                })
                                .filter(|s| !s.is_empty());
                            if let Some(del) = action_name {
                                for edge in &result.removed {
                                    let _ = self.dispatch(
                                        rel.from_instance.as_str(),
                                        del,
                                        IndexMap::from([(
                                            "id".into(),
                                            TypedValue::String(edge.from_id.clone()),
                                        )]),
                                    );
                                }
                                self.links
                                    .apply_on_delete(&key, entity_id, OnDeletePolicy::Unlink);
                            } else {
                                return Err(RuntimeError::Rejected(
                                    "cascade policy requires relationship description cascade_delete_action=<cmd>".into(),
                                ));
                            }
                        }
                    }
                }
            }
        }

        let step = {
            let engine = self
                .engines
                .get_mut(instance)
                .ok_or_else(|| RuntimeError::UnknownInstance(instance.into()))?;
            engine.execute(action, parameters)
        };

        let mut result = DispatchResult {
            instance: instance.into(),
            action: action.into(),
            step: step.clone(),
            propagated: vec![],
            link_diagnostics: vec![],
        };

        if matches!(step.result, StepResult::Success) {
            let events = step.events.clone();
            let mut queue: VecDeque<(String, String, IndexMap<String, TypedValue>, usize)> =
                VecDeque::new();
            for ev in events {
                let payload = event_payload_from_step(&step, &ev);
                queue.push_back((instance.into(), ev, payload, 0));
            }
            while let Some((src_inst, ev_name, payload, depth)) = queue.pop_front() {
                if depth > MAX_EVENT_DEPTH {
                    return Err(RuntimeError::EventLoopLimit);
                }
                let bindings: Vec<BindingDecl> = self
                    .ir
                    .bindings
                    .iter()
                    .filter(|b| {
                        b.source.instance.as_str() == src_inst
                            && b.source.name == ev_name
                            && matches!(b.source.kind, EndpointKind::Event)
                    })
                    .cloned()
                    .collect();
                for b in bindings {
                    let mut params = IndexMap::new();
                    for (tgt_key, src_key) in &b.parameter_map {
                        if let Some(v) = payload.get(src_key) {
                            params.insert(tgt_key.clone(), v.clone());
                        }
                    }
                    // Also pass through common id fields.
                    if params.is_empty() {
                        params = payload.clone();
                    }
                    let child =
                        self.dispatch(b.target.instance.as_str(), &b.target.name, params)?;
                    for ev in &child.step.events {
                        if matches!(child.step.result, StepResult::Success) {
                            let p = event_payload_from_step(&child.step, ev);
                            queue.push_back((child.instance.clone(), ev.clone(), p, depth + 1));
                        }
                    }
                    result.propagated.push(child);
                }
            }
        }

        self.persist_all()?;
        Ok(result)
    }

    fn cmd_link(
        &mut self,
        parameters: IndexMap<String, TypedValue>,
    ) -> Result<DispatchResult, RuntimeError> {
        let rel = require_str(&parameters, "relationship")?;
        let from_id = require_str(&parameters, "from_id")?;
        let to_id = require_str(&parameters, "to_id")?;
        let op = self.links.link(&rel, from_id, to_id);
        self.persist_all()?;
        Ok(synthetic_ok("reforge", "reforge.link", op.diagnostics))
    }

    fn cmd_unlink(
        &mut self,
        parameters: IndexMap<String, TypedValue>,
    ) -> Result<DispatchResult, RuntimeError> {
        let rel = require_str(&parameters, "relationship")?;
        let from_id = require_str(&parameters, "from_id")?;
        let to_id = require_str(&parameters, "to_id")?;
        let op = self.links.unlink(&rel, &from_id, &to_id);
        self.persist_all()?;
        Ok(synthetic_ok("reforge", "reforge.unlink", op.diagnostics))
    }

    fn cmd_links_for(
        &mut self,
        parameters: IndexMap<String, TypedValue>,
    ) -> Result<DispatchResult, RuntimeError> {
        let rel = require_str(&parameters, "relationship")?;
        let to_id = parameters.get("to_id").and_then(|v| v.as_str());
        let from_id = parameters.get("from_id").and_then(|v| v.as_str());
        let edges = if let Some(t) = to_id {
            self.links.links_to(&rel, t)
        } else if let Some(f) = from_id {
            self.links.links_from(&rel, f)
        } else {
            self.links.tables.get(&rel).cloned().unwrap_or_default()
        };
        let list: Vec<TypedValue> = edges
            .into_iter()
            .map(|e| {
                TypedValue::Map(IndexMap::from([
                    ("from_id".into(), TypedValue::String(e.from_id)),
                    ("to_id".into(), TypedValue::String(e.to_id)),
                ]))
            })
            .collect();
        let mut outputs = IndexMap::new();
        outputs.insert("links".into(), TypedValue::List(list));
        Ok(DispatchResult {
            instance: "reforge".into(),
            action: "reforge.links_for".into(),
            step: TraceStep {
                index: 0,
                action: "reforge.links_for".into(),
                parameters,
                result: StepResult::Success,
                transition_id: None,
                state_before: "n/a".into(),
                state_after: "n/a".into(),
                variables_after: IndexMap::new(),
                outputs,
                events: vec![],
            },
            propagated: vec![],
            link_diagnostics: vec![],
        })
    }

    pub fn engine(&self, instance: &str) -> Option<&TwinEngine> {
        self.engines.get(instance)
    }
}

fn require_str(params: &IndexMap<String, TypedValue>, key: &str) -> Result<String, RuntimeError> {
    params
        .get(key)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .ok_or_else(|| RuntimeError::Other(format!("missing string param '{key}'")))
}

fn synthetic_ok(instance: &str, action: &str, diags: Vec<Diagnostic>) -> DispatchResult {
    DispatchResult {
        instance: instance.into(),
        action: action.into(),
        step: TraceStep {
            index: 0,
            action: action.into(),
            parameters: IndexMap::new(),
            result: StepResult::Success,
            transition_id: None,
            state_before: "n/a".into(),
            state_after: "n/a".into(),
            variables_after: IndexMap::new(),
            outputs: IndexMap::new(),
            events: vec![],
        },
        propagated: vec![],
        link_diagnostics: diags,
    }
}

fn event_payload_from_step(step: &TraceStep, _event: &str) -> IndexMap<String, TypedValue> {
    // Prefer outputs; fall back to input parameters (common for id).
    if !step.outputs.is_empty() {
        step.outputs.clone()
    } else {
        step.parameters.clone()
    }
}

fn restore_engine_state(
    engine: &mut TwinEngine,
    current_state: String,
    variables: IndexMap<String, TypedValue>,
) -> Result<(), RuntimeError> {
    // TwinEngine has no public setter; recreate from twin with patched initial variables.
    let mut twin = engine.twin().clone();
    twin.initial_state = current_state.clone();
    for (k, v) in &variables {
        if let Some(var) = twin.variables.get_mut(k) {
            var.initial = Some(v.clone());
        }
    }
    let restored = TwinEngine::new(twin).map_err(|e| RuntimeError::Other(e.to_string()))?;
    *engine = restored;
    // TwinEngine::new sets current_state from initial_state — good.
    // But if variables keys missing from twin.variables, ensure they're present by executing no-op.
    let _ = current_state;
    let _ = variables;
    Ok(())
}
