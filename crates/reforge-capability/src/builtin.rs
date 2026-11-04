//! Host-builtin reference implementations registered by module name.
//!
//! Package manifests may declare `HostBuiltin` implementations. The host
//! maps module names to deterministic handlers. Package code is never loaded
//! or executed during inspect/validate.

use indexmap::IndexMap;
use reforge_types::TypedValue;

pub type BuiltinHandler = fn(
    op: &str,
    state: &mut IndexMap<String, TypedValue>,
    input: &IndexMap<String, TypedValue>,
) -> Result<IndexMap<String, TypedValue>, BuiltinError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltinError {
    UnknownModule(String),
    UnknownOp(String),
    Invalid(String),
}

#[derive(Default)]
pub struct BuiltinRegistry {
    modules: IndexMap<String, BuiltinHandler>,
}

impl BuiltinRegistry {
    pub fn new() -> Self {
        let mut reg = Self::default();
        reg.register("notes_ref", notes_handler);
        reg.register("tasks_ref", tasks_handler);
        reg
    }

    pub fn register(&mut self, name: impl Into<String>, handler: BuiltinHandler) {
        self.modules.insert(name.into(), handler);
    }

    pub fn call(
        &self,
        module: &str,
        op: &str,
        state: &mut IndexMap<String, TypedValue>,
        input: &IndexMap<String, TypedValue>,
    ) -> Result<IndexMap<String, TypedValue>, BuiltinError> {
        let handler = self
            .modules
            .get(module)
            .ok_or_else(|| BuiltinError::UnknownModule(module.to_string()))?;
        handler(op, state, input)
    }
}

fn require_str(input: &IndexMap<String, TypedValue>, key: &str) -> Result<String, BuiltinError> {
    input
        .get(key)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .ok_or_else(|| BuiltinError::Invalid(format!("missing string input '{key}'")))
}

fn notes_map(
    state: &mut IndexMap<String, TypedValue>,
) -> Result<&mut IndexMap<String, TypedValue>, BuiltinError> {
    if !state.contains_key("notes") {
        state.insert("notes".into(), TypedValue::Map(IndexMap::new()));
    }
    match state.get_mut("notes") {
        Some(TypedValue::Map(m)) => Ok(m),
        _ => Err(BuiltinError::Invalid("state.notes must be a map".into())),
    }
}

fn notes_handler(
    op: &str,
    state: &mut IndexMap<String, TypedValue>,
    input: &IndexMap<String, TypedValue>,
) -> Result<IndexMap<String, TypedValue>, BuiltinError> {
    let mut out = IndexMap::new();
    match op {
        "create_note" => {
            let id = require_str(input, "id")?;
            let title = require_str(input, "title")?;
            let body = input
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let notes = notes_map(state)?;
            if notes.contains_key(&id) {
                return Err(BuiltinError::Invalid(format!("note '{id}' already exists")));
            }
            let mut note = IndexMap::new();
            note.insert("id".into(), TypedValue::String(id.clone()));
            note.insert("title".into(), TypedValue::String(title));
            note.insert("body".into(), TypedValue::String(body));
            note.insert("tags".into(), TypedValue::List(vec![]));
            notes.insert(id.clone(), TypedValue::Map(note));
            out.insert("id".into(), TypedValue::String(id));
        }
        "read_note" => {
            let id = require_str(input, "id")?;
            let notes = notes_map(state)?;
            let note = notes
                .get(&id)
                .cloned()
                .ok_or_else(|| BuiltinError::Invalid(format!("note '{id}' not found")))?;
            out.insert("note".into(), note);
        }
        "update_note" => {
            let id = require_str(input, "id")?;
            let notes = notes_map(state)?;
            let note = notes
                .get_mut(&id)
                .ok_or_else(|| BuiltinError::Invalid(format!("note '{id}' not found")))?;
            let TypedValue::Map(map) = note else {
                return Err(BuiltinError::Invalid("corrupt note".into()));
            };
            if let Some(title) = input.get("title").and_then(|v| v.as_str()) {
                map.insert("title".into(), TypedValue::String(title.to_string()));
            }
            if let Some(body) = input.get("body").and_then(|v| v.as_str()) {
                map.insert("body".into(), TypedValue::String(body.to_string()));
            }
            out.insert("id".into(), TypedValue::String(id));
        }
        "delete_note" => {
            let id = require_str(input, "id")?;
            let notes = notes_map(state)?;
            if notes.shift_remove(&id).is_none() {
                return Err(BuiltinError::Invalid(format!("note '{id}' not found")));
            }
            out.insert("deleted".into(), TypedValue::String(id));
        }
        "list_notes" => {
            let notes = notes_map(state)?;
            let ids: Vec<_> = notes
                .keys()
                .map(|k| TypedValue::String(k.clone()))
                .collect();
            out.insert("ids".into(), TypedValue::List(ids));
        }
        "search_notes" => {
            let query = require_str(input, "query")?.to_lowercase();
            let notes = notes_map(state)?;
            let mut ids = Vec::new();
            for (id, note) in notes.iter() {
                if let TypedValue::Map(m) = note {
                    let title = m.get("title").and_then(|v| v.as_str()).unwrap_or("");
                    let body = m.get("body").and_then(|v| v.as_str()).unwrap_or("");
                    if title.to_lowercase().contains(&query) || body.to_lowercase().contains(&query)
                    {
                        ids.push(TypedValue::String(id.clone()));
                    }
                }
            }
            out.insert("ids".into(), TypedValue::List(ids));
        }
        "tag_note" => {
            let id = require_str(input, "id")?;
            let tag = require_str(input, "tag")?;
            let notes = notes_map(state)?;
            let note = notes
                .get_mut(&id)
                .ok_or_else(|| BuiltinError::Invalid(format!("note '{id}' not found")))?;
            let TypedValue::Map(map) = note else {
                return Err(BuiltinError::Invalid("corrupt note".into()));
            };
            let tags = map
                .entry("tags".into())
                .or_insert_with(|| TypedValue::List(vec![]));
            if let TypedValue::List(list) = tags {
                if !list.iter().any(|t| t.as_str() == Some(tag.as_str())) {
                    list.push(TypedValue::String(tag));
                }
            }
            out.insert("id".into(), TypedValue::String(id));
        }
        other => return Err(BuiltinError::UnknownOp(other.to_string())),
    }
    Ok(out)
}

fn tasks_map(
    state: &mut IndexMap<String, TypedValue>,
) -> Result<&mut IndexMap<String, TypedValue>, BuiltinError> {
    if !state.contains_key("tasks") {
        state.insert("tasks".into(), TypedValue::Map(IndexMap::new()));
    }
    match state.get_mut("tasks") {
        Some(TypedValue::Map(m)) => Ok(m),
        _ => Err(BuiltinError::Invalid("state.tasks must be a map".into())),
    }
}

fn tasks_handler(
    op: &str,
    state: &mut IndexMap<String, TypedValue>,
    input: &IndexMap<String, TypedValue>,
) -> Result<IndexMap<String, TypedValue>, BuiltinError> {
    let mut out = IndexMap::new();
    match op {
        "create_task" => {
            let id = require_str(input, "id")?;
            let title = require_str(input, "title")?;
            let tasks = tasks_map(state)?;
            if tasks.contains_key(&id) {
                return Err(BuiltinError::Invalid(format!("task '{id}' already exists")));
            }
            let mut task = IndexMap::new();
            task.insert("id".into(), TypedValue::String(id.clone()));
            task.insert("title".into(), TypedValue::String(title));
            task.insert("status".into(), TypedValue::String("open".into()));
            task.insert("priority".into(), TypedValue::Int(0));
            tasks.insert(id.clone(), TypedValue::Map(task));
            out.insert("id".into(), TypedValue::String(id));
        }
        "update_task" => {
            let id = require_str(input, "id")?;
            let tasks = tasks_map(state)?;
            let task = tasks
                .get_mut(&id)
                .ok_or_else(|| BuiltinError::Invalid(format!("task '{id}' not found")))?;
            let TypedValue::Map(map) = task else {
                return Err(BuiltinError::Invalid("corrupt task".into()));
            };
            if map.get("status").and_then(|v| v.as_str()) == Some("deleted") {
                return Err(BuiltinError::Invalid("cannot update a deleted task".into()));
            }
            if let Some(title) = input.get("title").and_then(|v| v.as_str()) {
                map.insert("title".into(), TypedValue::String(title.to_string()));
            }
            out.insert("id".into(), TypedValue::String(id));
        }
        "set_priority" => {
            let id = require_str(input, "id")?;
            let priority = input
                .get("priority")
                .and_then(|v| v.as_int())
                .ok_or_else(|| BuiltinError::Invalid("missing int priority".into()))?;
            let tasks = tasks_map(state)?;
            let task = tasks
                .get_mut(&id)
                .ok_or_else(|| BuiltinError::Invalid(format!("task '{id}' not found")))?;
            let TypedValue::Map(map) = task else {
                return Err(BuiltinError::Invalid("corrupt task".into()));
            };
            map.insert("priority".into(), TypedValue::Int(priority));
            out.insert("id".into(), TypedValue::String(id));
        }
        "set_status" | "complete_task" | "reopen_task" => {
            let id = require_str(input, "id")?;
            let status = match op {
                "complete_task" => "completed".to_string(),
                "reopen_task" => "open".to_string(),
                _ => require_str(input, "status")?,
            };
            let tasks = tasks_map(state)?;
            if !tasks.contains_key(&id) {
                return Err(BuiltinError::Invalid(format!("task '{id}' not found")));
            }
            let task = tasks.get_mut(&id).unwrap();
            let TypedValue::Map(map) = task else {
                return Err(BuiltinError::Invalid("corrupt task".into()));
            };
            if map.get("status").and_then(|v| v.as_str()) == Some("deleted") {
                return Err(BuiltinError::Invalid(
                    "cannot change status of deleted task".into(),
                ));
            }
            map.insert("status".into(), TypedValue::String(status));
            out.insert("id".into(), TypedValue::String(id));
        }
        "delete_task" => {
            let id = require_str(input, "id")?;
            let tasks = tasks_map(state)?;
            let task = tasks
                .get_mut(&id)
                .ok_or_else(|| BuiltinError::Invalid(format!("task '{id}' not found")))?;
            let TypedValue::Map(map) = task else {
                return Err(BuiltinError::Invalid("corrupt task".into()));
            };
            map.insert("status".into(), TypedValue::String("deleted".into()));
            out.insert("id".into(), TypedValue::String(id));
        }
        "query_tasks" => {
            let status_filter = input.get("status").and_then(|v| v.as_str());
            let tasks = tasks_map(state)?;
            let mut ids = Vec::new();
            for (id, task) in tasks.iter() {
                if let TypedValue::Map(m) = task {
                    let st = m.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    if status_filter.map(|f| f == st).unwrap_or(true) {
                        ids.push(TypedValue::String(id.clone()));
                    }
                }
            }
            out.insert("ids".into(), TypedValue::List(ids));
        }
        other => return Err(BuiltinError::UnknownOp(other.to_string())),
    }
    Ok(out)
}
