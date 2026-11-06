//! Browser investigation adapter (Playwright driver, protocol 1).
//!
//! The adapter is a thin protocol client: a Node process owns Playwright and the
//! static server, while this crate owns the alphabet, observations, budgets and
//! reset verification. No inference happens in JavaScript.
//!
//! Protocol: one JSON object per line on stdin/stdout. `init`, `reset`,
//! `snapshot`, `perform`, `close`; every response carries `{id, ok, data|error}`.

use crate::adapters::web::protocol_value::Snapshot;
use indexmap::IndexMap;
use reforge_canon::digest_of_raw;
use reforge_observe::*;
use reforge_types::{Digest, Id, TypedValue};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub mod protocol_value {
    //! Wire types for the driver protocol (`reforge-web-driver/1`).
    use serde::Deserialize;

    #[derive(Clone, Debug, Default, Deserialize)]
    pub struct Screenshot {
        #[serde(default)]
        pub digest: String,
        #[serde(default)]
        pub bytes: u64,
        #[serde(default)]
        pub media_type: String,
    }

    #[derive(Clone, Debug, Default, Deserialize)]
    pub struct FormField {
        #[serde(default)]
        pub name: String,
        #[serde(default)]
        pub value: String,
    }

    #[derive(Clone, Debug, Default, Deserialize)]
    pub struct Snapshot {
        #[serde(default)]
        pub url: String,
        #[serde(default)]
        pub title: String,
        #[serde(default)]
        pub aria: String,
        #[serde(default)]
        pub text: String,
        #[serde(default)]
        pub storage: std::collections::BTreeMap<String, String>,
        #[serde(default)]
        pub forms: Vec<FormField>,
        #[serde(default)]
        pub console_errors: Vec<String>,
        #[serde(default)]
        pub screenshot: Screenshot,
        #[serde(default)]
        pub state_digest: String,
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct WebActionSpec {
    pub action: String,
    pub kind: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub click_role: Option<String>,
    #[serde(default)]
    pub click_name: Option<String>,
    #[serde(default)]
    pub params: IndexMap<String, ParamSource>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WebTargetSpec {
    pub target_id: String,
    pub target_name: String,
    #[serde(default)]
    pub authorization: Option<Authorization>,
    pub app_dir: PathBuf,
    #[serde(default = "default_entry")]
    pub entry: String,
    /// Optional override for the driver path (absolute or spec-relative).
    #[serde(default)]
    pub driver: Option<PathBuf>,
    pub actions: Vec<WebActionSpec>,
    #[serde(skip)]
    pub spec_dir: PathBuf,
}

fn default_entry() -> String {
    "index.html".into()
}

impl WebTargetSpec {
    pub fn load(path: &Path) -> Result<Self, InvestigationError> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            InvestigationError::Target(format!("read web spec {}: {e}", path.display()))
        })?;
        let mut spec: Self = serde_json::from_str(&text).map_err(|e| {
            InvestigationError::Target(format!("parse web spec {}: {e}", path.display()))
        })?;
        let base = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        spec.spec_dir = std::fs::canonicalize(&base).unwrap_or(base);
        Ok(spec)
    }

    pub fn build(&self) -> Result<WebTarget, InvestigationError> {
        let app_dir = if self.app_dir.is_absolute() {
            self.app_dir.clone()
        } else {
            self.spec_dir.join(&self.app_dir)
        };
        let driver = match &self.driver {
            Some(path) if path.is_absolute() => path.clone(),
            Some(path) => self.spec_dir.join(path),
            None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../adapters/web/driver.mjs"),
        };
        let driver = std::fs::canonicalize(&driver).unwrap_or(driver);
        if !driver.is_file() {
            return Err(InvestigationError::Target(format!(
                "web driver not found at {}",
                driver.display()
            )));
        }
        WebTarget::launch(self, &app_dir, &driver)
    }
}

#[derive(Clone, Debug)]
struct WebFingerprintInput<'a> {
    snapshot: &'a Snapshot,
}

fn fingerprint(snapshot: &Snapshot) -> StateFingerprint {
    let input = WebFingerprintInput { snapshot };
    let view = input
        .snapshot
        .url
        .split("//")
        .nth(1)
        .and_then(|rest| rest.find('/').map(|index| rest[index..].to_string()))
        .unwrap_or_else(|| "/".to_string());
    let mut flags: Vec<String> = Vec::new();
    for (key, value) in &input.snapshot.storage {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(value) {
            if let Some(object) = parsed.as_object() {
                for (field, field_value) in object {
                    if let Some(text) = field_value.as_str() {
                        if text.chars().count() <= 24 {
                            flags.push(format!("{key}.{field}={text}"));
                        }
                    } else if let Some(number) = field_value.as_i64() {
                        flags.push(format!("{key}.{field}={number}"));
                    }
                }
            }
        }
    }
    for form in &input.snapshot.forms {
        flags.push(format!("form.{}={}", form.name, form.value));
    }
    flags.sort();
    let counts = format!(
        "aria_nodes={} forms={} storage_keys={}",
        input.snapshot.aria.lines().count(),
        input.snapshot.forms.len(),
        input.snapshot.storage.len()
    );
    let mut features = BTreeMap::new();
    features.insert("view".into(), view);
    features.insert("flags".into(), flags.join(" "));
    features.insert(
        "membership".into(),
        digest_of_raw(input.snapshot.aria.as_bytes()).hex,
    );
    features.insert("counts".into(), counts);
    features.insert("tree".into(), input.snapshot.state_digest.clone());
    StateFingerprint::from_features(FingerprintKind::Structured, features)
}

fn typed_arg(value: &TypedValue) -> String {
    match value {
        TypedValue::Null => String::new(),
        TypedValue::Bool(b) => b.to_string(),
        TypedValue::Int(i) => i.to_string(),
        TypedValue::Float(f) => f.to_string(),
        TypedValue::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

pub struct WebTarget {
    identity: TargetIdentity,
    alphabet: ActionAlphabet,
    environment: EnvironmentInfo,
    spec: WebTargetSpec,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_request: u64,
    initial_digest: Option<String>,
    last: Option<Snapshot>,
}

impl WebTarget {
    fn launch(
        spec: &WebTargetSpec,
        app_dir: &Path,
        driver: &Path,
    ) -> Result<Self, InvestigationError> {
        let mut child = Command::new("node")
            .arg(driver)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                InvestigationError::Target(format!(
                    "failed to start browser driver: {e} (is node installed?)"
                ))
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| InvestigationError::Target("driver stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| InvestigationError::Target("driver stdout unavailable".into()))?;
        let identity = TargetIdentity {
            id: Id::new(spec.target_id.clone())
                .map_err(|e| InvestigationError::Target(format!("invalid target id: {e}")))?,
            name: spec.target_name.clone(),
            platform: PlatformKind::Web,
            version: "reforge-web-driver/1".into(),
            authorization: spec.authorization.unwrap_or(Authorization::Unverified),
        };
        let entries = spec
            .actions
            .iter()
            .map(|action| AlphabetEntry {
                action: action.action.clone(),
                kind: match action.kind.as_str() {
                    "fill" => ActionKind::Type,
                    "navigate" => ActionKind::Navigate,
                    "submit" => ActionKind::Submit,
                    _ => ActionKind::Click,
                },
                parameters: IndexMap::new(),
                params: action.params.clone(),
                target: Some(SemanticTarget {
                    role: action.role.clone().unwrap_or_else(|| "button".into()),
                    name: action.name.clone().unwrap_or_default(),
                    origin: TargetOrigin::Accessibility,
                }),
                description: action.description.clone(),
            })
            .collect();
        let alphabet = ActionAlphabet {
            entries,
            discovered_by: format!(
                "declared accessibility targets ({} actions)",
                spec.actions.len()
            ),
        };
        let environment = EnvironmentInfo {
            platform: PlatformKind::Web,
            os: std::env::consts::OS.to_string(),
            tool: "playwright-driver".into(),
            tool_version: "reforge-web-driver/1".into(),
            viewport: Some("960x720".into()),
            fixture_digest: None,
            clock: "browser wall clock (not injected)".into(),
        };
        let mut target = Self {
            identity,
            alphabet,
            environment,
            spec: spec.clone(),
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_request: 1,
            initial_digest: None,
            last: None,
        };
        let init = target.request(
            "init",
            serde_json::json!({
                "app_dir": app_dir.to_string_lossy(),
                "entry": spec.entry,
            }),
        )?;
        target.initial_digest = Some(init.state_digest.clone());
        target.last = Some(init);
        Ok(target)
    }

    fn request(
        &mut self,
        op: &str,
        payload: serde_json::Value,
    ) -> Result<Snapshot, InvestigationError> {
        let id = self.next_request;
        self.next_request += 1;
        let mut message = payload;
        message["id"] = serde_json::json!(id);
        message["op"] = serde_json::json!(op);
        writeln!(self.stdin, "{message}")
            .and_then(|_| self.stdin.flush())
            .map_err(|e| InvestigationError::Target(format!("driver write failed: {e}")))?;
        let mut line = String::new();
        let read = self
            .stdout
            .read_line(&mut line)
            .map_err(|e| InvestigationError::Target(format!("driver read failed: {e}")))?;
        if read == 0 {
            return Err(InvestigationError::Target(
                "browser driver exited before responding".into(),
            ));
        }
        let value: serde_json::Value = serde_json::from_str(&line)
            .map_err(|e| InvestigationError::Target(format!("driver protocol error: {e}")))?;
        if value.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            let error = value
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown driver error");
            return Err(InvestigationError::Target(format!(
                "browser action failed: {error}"
            )));
        }
        serde_json::from_value(value.get("data").cloned().unwrap_or_default())
            .map_err(|e| InvestigationError::Target(format!("driver payload error: {e}")))
    }

    /// Inherent shutdown so callers do not need the protocol trait in scope.
    pub fn close(&mut self) {
        let _ = writeln!(
            self.stdin,
            "{}",
            serde_json::json!({"id": 0, "op": "close"})
        );
        let _ = self.stdin.flush();
        let _ = self.child.wait();
    }

    fn build_observation(
        &self,
        action: Option<ActionInstance>,
        before: StateFingerprint,
        before_snapshot: &Snapshot,
        after_snapshot: Snapshot,
        baseline: bool,
    ) -> Observation {
        let after = fingerprint(&after_snapshot);
        let mut outputs = IndexMap::new();
        outputs.insert("outcome".into(), TypedValue::String("success".into()));
        outputs.insert("url".into(), TypedValue::String(after_snapshot.url.clone()));
        outputs.insert(
            "title".into(),
            TypedValue::String(after_snapshot.title.clone()),
        );
        outputs.insert(
            "screenshot_digest".into(),
            TypedValue::String(after_snapshot.screenshot.digest.clone()),
        );

        let mut storage_changed: Vec<String> = Vec::new();
        for (key, value) in &after_snapshot.storage {
            if before_snapshot.storage.get(key) != Some(value) {
                storage_changed.push(key.clone());
            }
        }
        for key in before_snapshot.storage.keys() {
            if !after_snapshot.storage.contains_key(key) {
                storage_changed.push(key.clone());
            }
        }
        storage_changed.sort();
        let mut side_effects = Vec::new();
        if !storage_changed.is_empty() {
            side_effects.push(SideEffectObservation::Storage {
                area: "localStorage".into(),
                changed_keys: storage_changed,
            });
        }
        side_effects.push(SideEffectObservation::None);

        Observation {
            schema_version: IrVersion::current(),
            id: Id::new("pending").expect("static id"),
            session_id: Id::new("pending").expect("static id"),
            step_index: 0,
            target: self.identity.id.clone(),
            baseline,
            action,
            before,
            after: Some(after),
            outputs,
            // Browser state is captured through localStorage and the driver's
            // structured snapshot, not as a generic persisted document.
            state_document: None,
            visible_text: Some(NormalizedText::capture(&after_snapshot.text)),
            side_effects,
            errors: after_snapshot.console_errors.clone(),
            timing: TimingInfo::default(),
            environment: self.environment.clone(),
            payloads: vec![PayloadRef {
                kind: "screenshot".into(),
                digest: Digest {
                    algorithm: reforge_types::DigestAlgorithm::Sha256,
                    hex: after_snapshot.screenshot.digest.clone(),
                },
                bytes: after_snapshot.screenshot.bytes,
                media_type: if after_snapshot.screenshot.media_type.is_empty() {
                    "image/png".into()
                } else {
                    after_snapshot.screenshot.media_type.clone()
                },
            }],
            evidence_refs: Vec::new(),
            completeness: Completeness::Complete,
            uncertainty: Vec::new(),
            reproducibility: Reproducibility::Deterministic,
            inferred_hidden_state: Vec::new(),
            reset: ResetProvenance {
                policy: ResetPolicy::FreshEnvironment,
                succeeded: true,
                attempts: 1,
                state_after_reset: self.initial_digest.as_ref().map(|digest| Digest {
                    algorithm: reforge_types::DigestAlgorithm::Sha256,
                    hex: digest.clone(),
                }),
                detail: Some("new browser context with empty localStorage".into()),
            },
        }
    }
}

impl InvestigationTarget for WebTarget {
    fn identity(&self) -> &TargetIdentity {
        &self.identity
    }

    fn alphabet(&self) -> &ActionAlphabet {
        &self.alphabet
    }

    fn environment(&self) -> EnvironmentInfo {
        self.environment.clone()
    }

    fn reset_policy(&self) -> ResetPolicy {
        ResetPolicy::FreshEnvironment
    }

    fn reset(&mut self) -> Result<ResetOutcome, InvestigationError> {
        let snapshot = self.request("reset", serde_json::json!({}))?;
        let succeeded = self
            .initial_digest
            .as_ref()
            .map(|digest| digest == &snapshot.state_digest)
            .unwrap_or(true);
        self.last = Some(snapshot.clone());
        Ok(ResetOutcome {
            policy: ResetPolicy::FreshEnvironment,
            succeeded,
            attempts: 1,
            state_digest: Some(Digest {
                algorithm: reforge_types::DigestAlgorithm::Sha256,
                hex: snapshot.state_digest.clone(),
            }),
            detail: Some(if succeeded {
                "fresh context restored the initial accessibility/storage digest".into()
            } else {
                "fresh context produced a different initial digest".into()
            }),
        })
    }

    fn observe(&mut self) -> Result<Observation, InvestigationError> {
        let snapshot = self.request("snapshot", serde_json::json!({}))?;
        let fp = fingerprint(&snapshot);
        let observation =
            self.build_observation(None, fp.clone(), &snapshot, snapshot.clone(), true);
        self.last = Some(snapshot);
        Ok(observation)
    }

    fn perform(&mut self, action: &ActionInstance) -> Result<Observation, InvestigationError> {
        let before_snapshot = self.request("snapshot", serde_json::json!({}))?;
        let before = fingerprint(&before_snapshot);
        let spec = self
            .spec
            .actions
            .iter()
            .find(|entry| entry.action == action.action)
            .ok_or_else(|| {
                InvestigationError::Protocol(format!(
                    "action '{}' is not declared in the web target",
                    action.action
                ))
            })?;
        let value = action
            .params
            .get("value")
            .map(typed_arg)
            .unwrap_or_default();
        let snapshot = self.request(
            "perform",
            serde_json::json!({
                "kind": spec.kind,
                "role": spec.role,
                "name": spec.name,
                "click_role": spec.click_role,
                "click_name": spec.click_name,
                "value": value,
            }),
        )?;
        let observation = self.build_observation(
            Some(action.clone()),
            before,
            &before_snapshot,
            snapshot.clone(),
            false,
        );
        self.last = Some(snapshot);
        Ok(observation)
    }
}

impl Drop for WebTarget {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_separates_storage_states() {
        let mut a = Snapshot {
            aria: "- heading \"Board\"".into(),
            ..Default::default()
        };
        a.storage
            .insert("refboard".into(), "{\"filter\":\"all\"}".into());
        let mut b = a.clone();
        b.storage
            .insert("refboard".into(), "{\"filter\":\"open\"}".into());
        assert_ne!(fingerprint(&a).digest, fingerprint(&b).digest);
    }
}
