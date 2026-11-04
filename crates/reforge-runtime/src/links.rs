use indexmap::IndexMap;
use reforge_compose::OnDeletePolicy;
use reforge_types::{Diagnostic, Id};
use serde::{Deserialize, Serialize};

/// Composition-owned directed links between entity ids across instances.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LinkStore {
    /// relationship_id -> list of (from_id, to_id)
    pub tables: IndexMap<String, Vec<LinkEdge>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkEdge {
    pub from_id: String,
    pub to_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinkOpResult {
    pub ok: bool,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub removed: Vec<LinkEdge>,
}

impl LinkStore {
    pub fn ensure_table(&mut self, relationship_id: &str) {
        self.tables.entry(relationship_id.to_string()).or_default();
    }

    pub fn link(
        &mut self,
        relationship_id: &str,
        from_id: impl Into<String>,
        to_id: impl Into<String>,
    ) -> LinkOpResult {
        self.ensure_table(relationship_id);
        let edge = LinkEdge {
            from_id: from_id.into(),
            to_id: to_id.into(),
        };
        let table = self.tables.get_mut(relationship_id).unwrap();
        if table.iter().any(|e| e == &edge) {
            return LinkOpResult {
                ok: true,
                diagnostics: vec![Diagnostic::warning("link.exists", "link already present")],
                removed: vec![],
            };
        }
        table.push(edge);
        LinkOpResult {
            ok: true,
            diagnostics: vec![],
            removed: vec![],
        }
    }

    pub fn unlink(&mut self, relationship_id: &str, from_id: &str, to_id: &str) -> LinkOpResult {
        let Some(table) = self.tables.get_mut(relationship_id) else {
            return LinkOpResult {
                ok: true,
                diagnostics: vec![],
                removed: vec![],
            };
        };
        let before = table.len();
        table.retain(|e| !(e.from_id == from_id && e.to_id == to_id));
        LinkOpResult {
            ok: true,
            diagnostics: vec![],
            removed: if before != table.len() {
                vec![LinkEdge {
                    from_id: from_id.into(),
                    to_id: to_id.into(),
                }]
            } else {
                vec![]
            },
        }
    }

    pub fn links_to(&self, relationship_id: &str, to_id: &str) -> Vec<LinkEdge> {
        self.tables
            .get(relationship_id)
            .map(|t| t.iter().filter(|e| e.to_id == to_id).cloned().collect())
            .unwrap_or_default()
    }

    pub fn links_from(&self, relationship_id: &str, from_id: &str) -> Vec<LinkEdge> {
        self.tables
            .get(relationship_id)
            .map(|t| t.iter().filter(|e| e.from_id == from_id).cloned().collect())
            .unwrap_or_default()
    }

    /// Apply on-delete policy for a deleted `to_id`. Returns whether deletion is allowed.
    pub fn apply_on_delete(
        &mut self,
        relationship_id: &str,
        to_id: &str,
        policy: OnDeletePolicy,
    ) -> LinkOpResult {
        let linked = self.links_to(relationship_id, to_id);
        match policy {
            OnDeletePolicy::Unlink => {
                if let Some(table) = self.tables.get_mut(relationship_id) {
                    table.retain(|e| e.to_id != to_id);
                }
                LinkOpResult {
                    ok: true,
                    diagnostics: vec![],
                    removed: linked,
                }
            }
            OnDeletePolicy::Reject => {
                if linked.is_empty() {
                    LinkOpResult {
                        ok: true,
                        diagnostics: vec![],
                        removed: vec![],
                    }
                } else {
                    LinkOpResult {
                        ok: false,
                        diagnostics: vec![Diagnostic::error(
                            "link.reject_delete",
                            format!(
                                "cannot delete '{}': {} link(s) remain under reject policy",
                                to_id,
                                linked.len()
                            ),
                        )],
                        removed: vec![],
                    }
                }
            }
            OnDeletePolicy::Cascade => {
                // Runtime caller must delete `from` entities; we only clear edges after.
                LinkOpResult {
                    ok: true,
                    diagnostics: vec![Diagnostic::warning(
                        "link.cascade",
                        "cascade requires runtime to delete linked from-entities",
                    )],
                    removed: linked,
                }
            }
        }
    }
}

/// Stable id helper for relationship tables.
pub fn relationship_key(id: &Id) -> String {
    id.to_string()
}
