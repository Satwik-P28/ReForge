use crate::links::LinkStore;
use indexmap::IndexMap;
use reforge_types::TypedValue;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PersistError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serde: {0}")]
    Serde(String),
    #[error("io: {0}")]
    Io(String),
}

/// Loaded instance twin snapshot: `(current_state, variables)`.
pub type InstanceSnapshot = (String, IndexMap<String, TypedValue>);

/// SQLite-backed snapshot store for instance twin state + link tables.
pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, PersistError> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent).map_err(|e| PersistError::Io(e.to_string()))?;
        }
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_memory() -> Result<Self, PersistError> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), PersistError> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS meta (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS instance_state (
              instance TEXT PRIMARY KEY,
              current_state TEXT NOT NULL,
              variables_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS links (
              relationship_id TEXT PRIMARY KEY,
              edges_json TEXT NOT NULL
            );
            INSERT OR IGNORE INTO meta(key, value) VALUES ('schema_version', '1');
            "#,
        )?;
        Ok(())
    }

    pub fn save_instance(
        &self,
        instance: &str,
        current_state: &str,
        variables: &IndexMap<String, TypedValue>,
    ) -> Result<(), PersistError> {
        let json =
            serde_json::to_string(variables).map_err(|e| PersistError::Serde(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO instance_state(instance, current_state, variables_json)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(instance) DO UPDATE SET
               current_state=excluded.current_state,
               variables_json=excluded.variables_json",
            params![instance, current_state, json],
        )?;
        Ok(())
    }

    pub fn load_instance(
        &self,
        instance: &str,
    ) -> Result<Option<InstanceSnapshot>, PersistError> {
        let mut stmt = self.conn.prepare(
            "SELECT current_state, variables_json FROM instance_state WHERE instance = ?1",
        )?;
        let mut rows = stmt.query(params![instance])?;
        if let Some(row) = rows.next()? {
            let state: String = row.get(0)?;
            let json: String = row.get(1)?;
            let vars: IndexMap<String, TypedValue> =
                serde_json::from_str(&json).map_err(|e| PersistError::Serde(e.to_string()))?;
            Ok(Some((state, vars)))
        } else {
            Ok(None)
        }
    }

    pub fn save_links(&self, links: &LinkStore) -> Result<(), PersistError> {
        for (rel, edges) in &links.tables {
            let json =
                serde_json::to_string(edges).map_err(|e| PersistError::Serde(e.to_string()))?;
            self.conn.execute(
                "INSERT INTO links(relationship_id, edges_json) VALUES (?1, ?2)
                 ON CONFLICT(relationship_id) DO UPDATE SET edges_json=excluded.edges_json",
                params![rel, json],
            )?;
        }
        Ok(())
    }

    pub fn load_links(&self) -> Result<LinkStore, PersistError> {
        let mut stmt = self
            .conn
            .prepare("SELECT relationship_id, edges_json FROM links")?;
        let mut rows = stmt.query([])?;
        let mut store = LinkStore::default();
        while let Some(row) = rows.next()? {
            let rel: String = row.get(0)?;
            let json: String = row.get(1)?;
            let edges: Vec<crate::links::LinkEdge> =
                serde_json::from_str(&json).map_err(|e| PersistError::Serde(e.to_string()))?;
            store.tables.insert(rel, edges);
        }
        Ok(store)
    }

    /// Run a closure inside a SQLite transaction; rollback on error.
    pub fn transaction<T, E, F>(&mut self, f: F) -> Result<T, E>
    where
        E: From<PersistError>,
        F: FnOnce(&Store) -> Result<T, E>,
    {
        self.conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(PersistError::from)
            .map_err(E::from)?;
        match f(self) {
            Ok(v) => {
                self.conn
                    .execute_batch("COMMIT")
                    .map_err(PersistError::from)
                    .map_err(E::from)?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    pub fn meta_get(&self, key: &str) -> Result<Option<String>, PersistError> {
        let mut stmt = self.conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn meta_set(&self, key: &str, value: &str) -> Result<(), PersistError> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn export_json_snapshot(&self) -> Result<Value, PersistError> {
        let mut instances = serde_json::Map::new();
        let mut stmt = self
            .conn
            .prepare("SELECT instance, current_state, variables_json FROM instance_state")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let inst: String = row.get(0)?;
            let state: String = row.get(1)?;
            let vars: Value = serde_json::from_str(&row.get::<_, String>(2)?)
                .map_err(|e| PersistError::Serde(e.to_string()))?;
            instances.insert(
                inst,
                serde_json::json!({"current_state": state, "variables": vars}),
            );
        }
        let links = self.load_links()?;
        Ok(serde_json::json!({
            "schema_version": self.meta_get("schema_version")?.unwrap_or_else(|| "1".into()),
            "instances": instances,
            "links": links,
        }))
    }
}
