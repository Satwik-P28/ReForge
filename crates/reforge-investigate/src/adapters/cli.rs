//! Authorised CLI investigation adapter.
//!
//! A "CLI target" is a program that keeps its state in a working directory and
//! emits one JSON object per invocation. The adapter:
//!
//! * runs the program in an isolated working directory with a scrubbed
//!   environment,
//! * fingerprints state from disk (never from memory),
//! * captures stdout/stderr/exit code and filesystem side effects,
//! * discovers actions from `--help` and from `available_actions` in responses,
//! * can restore a pristine fixture directory between experiments.
//!
//! It assumes nothing about determinism: nondeterministic output shows up as
//! differing fingerprints and is preserved.

use indexmap::IndexMap;
use reforge_canon::digest_of_raw;
use reforge_observe::InvestigationError;
use reforge_observe::*;
use reforge_types::{Digest, Id, TypeDesc, TypedValue};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Declarative CLI target specification (the `target.json` format).
///
/// Loading and path resolution live here, not in the CLI binary, so benchmarks
/// and tests investigate the same targets through the same code path.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct CliTargetSpec {
    pub target_id: String,
    pub target_name: String,
    #[serde(default)]
    pub authorization: Option<Authorization>,
    pub program: String,
    #[serde(default)]
    pub program_args: Vec<String>,
    #[serde(default)]
    pub pristine_dir: Option<PathBuf>,
    #[serde(default)]
    pub workdir: Option<PathBuf>,
    #[serde(default)]
    pub state_file: Option<String>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    pub actions: Vec<CliActionSpecFile>,
    #[serde(skip)]
    pub spec_dir: PathBuf,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct CliActionSpecFile {
    pub action: String,
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub params: IndexMap<String, ParamSource>,
    #[serde(default)]
    pub parameters: IndexMap<String, TypeDesc>,
    #[serde(default)]
    pub description: Option<String>,
}

fn default_timeout_ms() -> u64 {
    10_000
}

impl CliTargetSpec {
    pub fn load(path: &Path) -> Result<Self, InvestigationError> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            InvestigationError::Target(format!("read target spec {}: {e}", path.display()))
        })?;
        let mut spec: Self = serde_json::from_str(&text).map_err(|e| {
            InvestigationError::Target(format!("parse target spec {}: {e}", path.display()))
        })?;
        let base = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        spec.spec_dir = std::fs::canonicalize(&base).unwrap_or(base);
        Ok(spec)
    }

    pub fn build(&self, workdir_override: Option<&Path>) -> Result<CliTarget, InvestigationError> {
        let base = &self.spec_dir;
        let base_text = base.to_string_lossy().to_string();
        let expand = |path: &Path| -> PathBuf {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                base.join(path)
            }
        };
        let program = PathBuf::from(self.program.replace("{spec_dir}", &base_text));
        let program_args = self
            .program_args
            .iter()
            .map(|arg| arg.replace("{spec_dir}", &base_text))
            .collect();
        let pristine_dir = expand(
            self.pristine_dir
                .as_deref()
                .unwrap_or_else(|| Path::new(".")),
        );
        let workdir = match workdir_override {
            Some(path) => path.to_path_buf(),
            None => match self.workdir.as_deref() {
                Some(path) => expand(path),
                None => std::env::temp_dir().join(format!("reforge-work-{}", self.target_id)),
            },
        };
        let actions = self
            .actions
            .iter()
            .map(|spec| CliActionSpec {
                action: spec.action.clone(),
                argv: spec.argv.clone(),
                params: spec.params.clone(),
                parameters: spec.parameters.clone(),
                description: spec.description.clone(),
            })
            .collect();
        CliTarget::new(CliTargetConfig {
            target_id: self.target_id.clone(),
            target_name: self.target_name.clone(),
            authorization: self.authorization.unwrap_or(Authorization::Unverified),
            program,
            program_args,
            pristine_dir,
            workdir,
            env: IndexMap::new(),
            timeout_ms: self.timeout_ms,
            actions,
            reset: CliResetStrategy::CopyPristine,
            state_file: self
                .state_file
                .clone()
                .unwrap_or_else(|| "state.json".into()),
        })
    }
}

#[derive(Clone, Debug)]
pub struct CliActionSpec {
    pub action: String,
    /// argv fragment; `$name` placeholders are substituted from params.
    pub argv: Vec<String>,
    pub params: IndexMap<String, ParamSource>,
    pub parameters: IndexMap<String, TypeDesc>,
    pub description: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliResetStrategy {
    /// Wipe the working directory and copy the pristine fixture back.
    CopyPristine,
}

#[derive(Clone, Debug)]
pub struct CliTargetConfig {
    pub target_id: String,
    pub target_name: String,
    pub authorization: Authorization,
    pub program: PathBuf,
    pub program_args: Vec<String>,
    pub pristine_dir: PathBuf,
    pub workdir: PathBuf,
    pub env: IndexMap<String, String>,
    pub timeout_ms: u64,
    pub actions: Vec<CliActionSpec>,
    pub reset: CliResetStrategy,
    /// File whose content defines observable state (``state.json``).
    pub state_file: String,
}

/// A filesystem snapshot used for fingerprints and side effects.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileTree {
    entries: BTreeMap<String, String>,
}

impl FileTree {
    pub fn snapshot(root: &Path) -> Result<Self, InvestigationError> {
        let mut entries = BTreeMap::new();
        collect(root, root, &mut entries, 0)?;
        Ok(Self { entries })
    }

    pub fn digest(&self) -> Digest {
        let mut buf = String::new();
        for (path, digest) in &self.entries {
            buf.push_str(path);
            buf.push('\u{1}');
            buf.push_str(digest);
            buf.push('\u{3}');
        }
        digest_of_raw(buf.as_bytes())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn diff(&self, other: &FileTree) -> (Vec<String>, Vec<String>, Vec<String>) {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut modified = Vec::new();
        for (path, digest) in &other.entries {
            match self.entries.get(path) {
                None => added.push(path.clone()),
                Some(prev) if prev != digest => modified.push(path.clone()),
                _ => {}
            }
        }
        for path in self.entries.keys() {
            if !other.entries.contains_key(path) {
                removed.push(path.clone());
            }
        }
        (added, removed, modified)
    }

    pub fn digest_of(&self, rel: &str) -> Option<&str> {
        self.entries.get(rel).map(|s| s.as_str())
    }
}

fn collect(
    root: &Path,
    dir: &Path,
    out: &mut BTreeMap<String, String>,
    depth: usize,
) -> Result<(), InvestigationError> {
    if depth > 8 {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .map_err(|e| InvestigationError::Io(e.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        if rel.contains("__pycache__") {
            continue;
        }
        let meta = entry.metadata()?;
        if meta.is_dir() {
            collect(root, &path, out, depth + 1)?;
        } else if meta.is_file() {
            let bytes = std::fs::read(&path)?;
            out.insert(rel, digest_of_raw(&bytes).hex);
        }
    }
    Ok(())
}

struct ProcessOutput {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
    duration_ms: u64,
}

pub struct CliTarget {
    config: CliTargetConfig,
    identity: TargetIdentity,
    alphabet: ActionAlphabet,
    environment: EnvironmentInfo,
    env_digest: Option<Digest>,
    prepared: bool,
}

impl CliTarget {
    pub fn new(config: CliTargetConfig) -> Result<Self, InvestigationError> {
        if !config.pristine_dir.is_dir() {
            return Err(InvestigationError::Target(format!(
                "pristine directory '{}' does not exist",
                config.pristine_dir.display()
            )));
        }
        let mut entries = Vec::new();
        for spec in &config.actions {
            entries.push(AlphabetEntry {
                action: spec.action.clone(),
                kind: ActionKind::Invoke,
                parameters: spec.parameters.clone(),
                params: spec.params.clone(),
                target: Some(SemanticTarget {
                    role: "command".into(),
                    name: spec.action.clone(),
                    origin: TargetOrigin::Argv,
                }),
                description: spec.description.clone(),
            });
        }
        let alphabet = ActionAlphabet {
            entries,
            discovered_by: format!(
                "declared spec ({} actions) + runtime discovery",
                config.actions.len()
            ),
        };
        let identity = TargetIdentity {
            id: Id::new(config.target_id.clone())
                .map_err(|e| InvestigationError::Target(format!("invalid target id: {e}")))?,
            name: config.target_name.clone(),
            platform: PlatformKind::Cli,
            version: format!("fixture:{}", config.target_name),
            authorization: config.authorization,
        };
        let environment = EnvironmentInfo {
            platform: PlatformKind::Cli,
            os: std::env::consts::OS.to_string(),
            tool: "reforge-cli-adapter".into(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            viewport: None,
            fixture_digest: None,
            clock: "deterministic (no wall clock injected)".into(),
        };
        Ok(Self {
            config,
            identity,
            alphabet,
            environment,
            env_digest: None,
            prepared: false,
        })
    }

    fn workdir(&self) -> &Path {
        &self.config.workdir
    }
    /// Derive observable features from the persisted state *without* assuming
    /// any application schema. Earlier versions read `tasks`/`status`/`notes`
    /// by name, which silently limited the adapter to one fixture; the
    /// projection below only relies on generic JSON structure.
    fn fingerprint(&self) -> Result<StateFingerprint, InvestigationError> {
        let tree = FileTree::snapshot(self.workdir())?;
        let state_path = self.workdir().join(&self.config.state_file);
        let state = std::fs::read_to_string(&state_path).ok();
        let parsed: Option<serde_json::Value> =
            state.as_deref().and_then(|s| serde_json::from_str(s).ok());

        let mut features = BTreeMap::new();
        let view = parsed
            .as_ref()
            .and_then(|value| {
                ["view", "step", "mode", "phase", "stage"]
                    .iter()
                    .find_map(|key| value.get(*key))
            })
            .and_then(|value| value.as_str())
            .unwrap_or("absent")
            .to_string();
        features.insert("view".into(), view);

        let mut flags: Vec<String> = Vec::new();
        let mut membership: Vec<String> = Vec::new();
        let mut counts: Vec<String> = Vec::new();
        if let Some(serde_json::Value::Object(map)) = &parsed {
            for (key, value) in map {
                match value {
                    serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
                        flags.push(format!("{key}={value}"));
                    }
                    serde_json::Value::String(text) if text.chars().count() <= 24 => {
                        flags.push(format!("{key}={text}"));
                    }
                    serde_json::Value::Array(items) => {
                        counts.push(format!("{key}:{}", items.len()));
                        let mut entries: Vec<String> = Vec::new();
                        for (index, item) in items.iter().enumerate() {
                            match item.as_object() {
                                Some(object) => {
                                    let id = ["id", "sku", "name", "key"]
                                        .iter()
                                        .find_map(|k| object.get(*k))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    let status = ["status", "state", "phase", "step", "stage"]
                                        .iter()
                                        .find_map(|k| object.get(*k))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    entries.push(format!("{index}:{id}:{status}"));
                                }
                                None => entries.push(format!("{index}:{}", compact_json(item))),
                            }
                        }
                        entries.sort();
                        membership.push(format!("{key}=[{}]", entries.join(",")));
                    }
                    serde_json::Value::Object(object) => {
                        counts.push(format!("{key}:{}", object.len()));
                        let mut keys: Vec<&String> = object.keys().collect();
                        keys.sort();
                        membership.push(format!(
                            "{key}={{{}}}",
                            keys.iter()
                                .map(|k| k.as_str())
                                .collect::<Vec<_>>()
                                .join(",")
                        ));
                    }
                    _ => {}
                }
            }
        }
        flags.sort();
        membership.sort();
        counts.sort();
        features.insert("flags".into(), flags.join(" "));
        features.insert(
            "membership".into(),
            digest_of_raw(membership.join("|").as_bytes()).hex,
        );
        features.insert("counts".into(), counts.join(" "));
        // Behavioral projection. The raw flags/membership features above are
        // provenance, not state identity: they include fresh IDs, derived
        // totals and monotonic rejection counters, which made the same
        // behavioral state split whenever an input string or error count
        // changed. This projection keeps observable status/quantity/count
        // distinctions and drops identity and debug bookkeeping.
        features.insert("behavior".into(), behavioral_shape(parsed.as_ref()));
        // Raw tree digest keeps payload-level changes visible without being
        // used by the default abstraction.
        features.insert("tree".into(), tree.digest().hex.clone());
        Ok(StateFingerprint::from_features(
            FingerprintKind::Process,
            features,
        ))
    }

    fn copy_dir(src: &Path, dst: &Path) -> Result<(), InvestigationError> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let path = entry.path();
            let target = dst.join(entry.file_name());
            if path.is_dir() {
                if entry.file_name().to_string_lossy().contains("__pycache__") {
                    continue;
                }
                Self::copy_dir(&path, &target)?;
            } else {
                std::fs::copy(&path, &target)?;
            }
        }
        Ok(())
    }

    fn run_process(&self, argv: &[String]) -> Result<ProcessOutput, InvestigationError> {
        let path_env = std::env::var("PATH").unwrap_or_default();
        let mut cmd = Command::new(&self.config.program);
        cmd.args(&self.config.program_args)
            .args(argv)
            .current_dir(self.workdir())
            .env_clear()
            .env("PATH", path_env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in &self.config.env {
            cmd.env(k, v);
        }
        let started = Instant::now();
        let mut child = cmd
            .spawn()
            .map_err(|e| InvestigationError::Target(format!("failed to spawn target: {e}")))?;
        let timeout = Duration::from_millis(self.config.timeout_ms);
        let mut timed_out = false;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if started.elapsed() >= timeout {
                        let _ = child.kill();
                        timed_out = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(e) => {
                    return Err(InvestigationError::Target(format!(
                        "waiting for target failed: {e}"
                    )))
                }
            }
        }
        let duration_ms = started.elapsed().as_millis() as u64;
        let mut stdout = String::new();
        let mut stderr = String::new();
        if let Some(mut out) = child.stdout.take() {
            let _ = out.read_to_string(&mut stdout);
        }
        if let Some(mut err) = child.stderr.take() {
            let _ = err.read_to_string(&mut stderr);
        }
        let exit_code = child.wait().ok().and_then(|s| s.code());
        Ok(ProcessOutput {
            exit_code,
            stdout,
            stderr,
            timed_out,
            duration_ms,
        })
    }

    fn argv_for(&self, action: &ActionInstance) -> Result<Vec<String>, InvestigationError> {
        let spec = self
            .config
            .actions
            .iter()
            .find(|s| s.action == action.action);
        let Some(spec) = spec else {
            // Discovered-at-runtime action with no declared argv mapping:
            // invoke it bare, which is honest even if the target rejects it.
            return Ok(vec![action.action.clone()]);
        };
        let mut argv = Vec::new();
        for token in &spec.argv {
            if let Some(name) = token.strip_prefix('$') {
                let value = action.params.get(name).ok_or_else(|| {
                    InvestigationError::Protocol(format!(
                        "action '{}' missing parameter '{name}'",
                        action.action
                    ))
                })?;
                argv.push(typed_to_arg(value));
            } else {
                argv.push(token.clone());
            }
        }
        Ok(argv)
    }

    fn observation_from(
        &self,
        action: Option<ActionInstance>,
        before: StateFingerprint,
        out: &ProcessOutput,
        tree_before: &FileTree,
        passive: bool,
    ) -> Observation {
        let envelope: Option<serde_json::Value> = out
            .stdout
            .lines()
            .rev()
            .find_map(|line| serde_json::from_str(line).ok());

        let mut outputs = IndexMap::new();
        let mut errors = Vec::new();
        let mut completeness = Completeness::Complete;
        let mut uncertainty = Vec::new();
        let state_document = std::fs::read_to_string(self.workdir().join(&self.config.state_file))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .filter(|value| {
                serde_json::to_vec(value)
                    .map(|bytes| bytes.len())
                    .unwrap_or(0)
                    <= 512 * 1024
            });

        if passive {
            // A baseline capture reads the target's complete persisted state;
            // no process ran, so the absence of a response envelope is expected
            // and must not be reported as missing evidence.
            outputs.insert("outcome".into(), TypedValue::String("observed".into()));
            let state_path = self.workdir().join(&self.config.state_file);
            if let Ok(text) = std::fs::read_to_string(&state_path) {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(map) = value.as_object() {
                        for (k, v) in map {
                            outputs.insert(format!("state.{k}"), json_to_typed(v));
                        }
                    }
                }
            }
        } else {
            match &envelope {
                Some(value) if value.is_object() => {
                    if let Some(map) = value.as_object() {
                        for (k, v) in map {
                            outputs.insert(k.clone(), json_to_typed(v));
                        }
                    }
                }
                _ => {
                    completeness = Completeness::Partial;
                    uncertainty.push(UncertaintyNote {
                        subject: "outputs".into(),
                        kind: UncertaintyKind::Unobserved,
                        statement: "target produced no machine-readable JSON envelope".into(),
                    });
                }
            }
        }
        if out.timed_out {
            errors.push(format!("timed out after {} ms", self.config.timeout_ms));
            completeness = Completeness::Partial;
            uncertainty.push(UncertaintyNote {
                subject: "execution".into(),
                kind: UncertaintyKind::EnvironmentDependent,
                statement: "process was killed at the adapter timeout".into(),
            });
        }
        for line in out.stderr.lines().filter(|l| !l.trim().is_empty()).take(8) {
            errors.push(line.to_string());
        }

        let tree_after = FileTree::snapshot(self.workdir()).unwrap_or_default();
        let (added, removed, modified) = tree_before.diff(&tree_after);
        let mut side_effects = Vec::new();
        if !(added.is_empty() && removed.is_empty() && modified.is_empty()) {
            side_effects.push(SideEffectObservation::Filesystem {
                root: "workdir".into(),
                added,
                removed,
                modified,
            });
        }
        side_effects.push(SideEffectObservation::Process {
            exit_code: out.exit_code,
        });

        let after = self.fingerprint().unwrap_or_else(|_| before.clone());

        Observation {
            schema_version: IrVersion::current(),
            id: Id::new("pending").expect("static id"),
            session_id: Id::new("pending").expect("static id"),
            step_index: 0,
            target: self.identity.id.clone(),
            baseline: passive,
            action,
            before,
            after: Some(after),
            outputs,
            state_document,
            visible_text: Some(NormalizedText::capture(&out.stdout)),
            side_effects,
            errors,
            timing: TimingInfo {
                action_ms: out.duration_ms,
                observe_ms: 0,
            },
            environment: self.environment.clone(),
            payloads: Vec::new(),
            evidence_refs: Vec::new(),
            completeness,
            uncertainty,
            reproducibility: Reproducibility::Unknown,
            inferred_hidden_state: Vec::new(),
            reset: ResetProvenance::assumed(ResetPolicy::FreshEnvironment),
        }
    }
}

impl InvestigationTarget for CliTarget {
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
        if self.workdir().exists() {
            std::fs::remove_dir_all(self.workdir())?;
        }
        Self::copy_dir(&self.config.pristine_dir, self.workdir())?;
        let tree = FileTree::snapshot(self.workdir())?;
        let pristine = FileTree::snapshot(&self.config.pristine_dir)?;
        let succeeded = tree == pristine;
        self.env_digest = Some(pristine.digest());
        self.environment.fixture_digest = Some(pristine.digest());
        self.prepared = true;
        Ok(ResetOutcome {
            policy: ResetPolicy::FreshEnvironment,
            succeeded,
            attempts: 1,
            state_digest: Some(tree.digest()),
            detail: Some(if succeeded {
                "workdir restored from pristine fixture".into()
            } else {
                "workdir differs from pristine fixture after copy".into()
            }),
        })
    }

    fn observe(&mut self) -> Result<Observation, InvestigationError> {
        if !self.prepared {
            return Err(InvestigationError::Protocol(
                "observe called before reset".into(),
            ));
        }
        let fp = self.fingerprint()?;
        let out = ProcessOutput {
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
            duration_ms: 0,
        };
        let tree = FileTree::snapshot(self.workdir())?;
        let mut observation = self.observation_from(None, fp, &out, &tree, true);
        observation.reset = ResetProvenance {
            policy: ResetPolicy::FreshEnvironment,
            succeeded: true,
            attempts: 1,
            state_after_reset: self.env_digest.clone(),
            detail: Some("baseline captured after a verified reset".into()),
        };
        Ok(observation)
    }

    fn perform(&mut self, action: &ActionInstance) -> Result<Observation, InvestigationError> {
        if !self.prepared {
            return Err(InvestigationError::Protocol(
                "perform called before reset".into(),
            ));
        }
        let before = self.fingerprint()?;
        let tree_before = FileTree::snapshot(self.workdir())?;
        let argv = self.argv_for(action)?;
        let out = self.run_process(&argv)?;
        let mut observation =
            self.observation_from(Some(action.clone()), before, &out, &tree_before, false);
        observation.reset = ResetProvenance {
            policy: ResetPolicy::FreshEnvironment,
            succeeded: true,
            attempts: 1,
            state_after_reset: self.env_digest.clone(),
            detail: Some("workdir restored before this observation's episode".into()),
        };
        Ok(observation)
    }
}

/// Fields that identify an entity or carry user-supplied content. They are
/// intentionally excluded from behavioral state: changing a title from `x1`
/// to `alpha` must not create a new abstract state when the application's
/// behavior is otherwise identical.
fn is_content_field(key: &str) -> bool {
    matches!(
        key,
        "name" | "title" | "email" | "text" | "payload" | "channel" | "selected" | "query" | "tag"
    )
}

/// Entity IDs are allocator artifacts, not abstract state. A contact created
/// as `c1` in one reset and `c2` in another is behaviorally the same state;
/// parameter-sensitive transitions are disambiguated by guards instead.
fn is_entity_id_field(key: &str) -> bool {
    matches!(key, "id" | "sku" | "key")
}

fn behavioral_scalar(key: &str, value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Bool(b) => format!("{key}={b}"),
        serde_json::Value::Number(n) => {
            // Stock/quantity fields are behaviorally significant and keep
            // their exact value. Derived totals, ID allocators and monotonic
            // bookkeeping are not state.
            if matches!(
                key,
                "on_hand"
                    | "reserved"
                    | "qty"
                    | "stock"
                    | "plan"
                    | "copies"
                    | "on_loan"
                    | "priority"
                    | "comments"
                    | "quantity"
                    | "capacity"
            ) {
                format!("{key}={n}")
            } else if n.as_f64().unwrap_or(0.0) > 0.0 {
                format!("{key}=positive")
            } else {
                format!("{key}=zero")
            }
        }
        serde_json::Value::String(s) => format!("{key}={s}"),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Array(items) => format!("{key}#{}[{}]", items.len(), {
            let mut shapes: Vec<String> = items
                .iter()
                .map(|item| match item {
                    serde_json::Value::Object(object) => object_behavior(object),
                    other => behavioral_scalar(key, other),
                })
                .collect();
            shapes.sort();
            shapes.join("|")
        }),
        serde_json::Value::Object(object) => object_behavior(object),
    }
}

fn object_behavior(object: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut parts: Vec<String> = object
        .iter()
        .filter_map(|(key, value)| {
            if is_entity_id_field(key) {
                return None;
            }
            if is_content_field(key) {
                // Presence can matter (a draft with a name versus without);
                // the actual user text cannot be part of abstract identity.
                return Some(format!("{key}=present"));
            }
            let shape = behavioral_scalar(key, value);
            if shape.is_empty() {
                None
            } else {
                Some(shape)
            }
        })
        .collect();
    parts.sort();
    format!("{{{}}}", parts.join(","))
}

/// Canonical, behavior-oriented state projection used for class identity.
///
/// Deliberately ignores: allocator IDs (`next_id`, `next_event`), derived
/// totals and counts, monotonic rejection/submission counters, selected entity
/// IDs, and user-supplied strings. Keeps: view, statuses, stock quantities,
/// tag/note counts, draft field presence, and collection cardinality.
fn behavioral_shape(parsed: Option<&serde_json::Value>) -> String {
    let Some(serde_json::Value::Object(map)) = parsed else {
        return "absent".into();
    };
    let mut parts: Vec<String> = Vec::new();
    for (key, value) in map {
        if matches!(
            key.as_str(),
            "view"
                | "next_id"
                | "next_event"
                | "rejects"
                | "submissions"
                | "acked"
                | "active_count"
                | "archived_count"
                | "on_hand_total"
                | "reserved_total"
                | "sku_count"
                | "contact_count"
                | "queue_size"
                | "selected"
        ) {
            continue;
        }
        match value {
            serde_json::Value::Array(items) => parts.push(format!("{key}#{}[{}]", items.len(), {
                let mut shapes: Vec<String> = items
                    .iter()
                    .map(|item| match item {
                        serde_json::Value::Object(object) => object_behavior(object),
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .collect();
                shapes.sort();
                shapes.join("|")
            })),
            serde_json::Value::Object(object) => {
                // Map keys are allocator IDs; only the count and value shapes
                // are behavior. User text is excluded by object_behavior.
                let mut values: Vec<String> = object
                    .values()
                    .map(|v| match v {
                        serde_json::Value::Object(inner) => object_behavior(inner),
                        other => other.to_string(),
                    })
                    .collect();
                values.sort();
                parts.push(format!("{key}#{}[{}]", object.len(), values.join("|")));
            }
            serde_json::Value::Null => {}
            other => {
                let shape = behavioral_scalar(key, other);
                if !shape.is_empty() {
                    parts.push(shape);
                }
            }
        }
    }
    parts.sort();
    parts.join(";")
}

fn compact_json(value: &serde_json::Value) -> String {
    let text = value.to_string();
    if text.chars().count() > 24 {
        format!("{}…", text.chars().take(24).collect::<String>())
    } else {
        text
    }
}

fn typed_to_arg(value: &TypedValue) -> String {
    match value {
        TypedValue::Null => String::new(),
        TypedValue::Bool(b) => b.to_string(),
        TypedValue::Int(i) => i.to_string(),
        TypedValue::Float(f) => f.to_string(),
        TypedValue::String(s) => s.clone(),
        TypedValue::BytesHex(b) => b.clone(),
        TypedValue::List(_) | TypedValue::Map(_) => {
            serde_json::to_string(value).unwrap_or_default()
        }
    }
}

fn json_to_typed(value: &serde_json::Value) -> TypedValue {
    match value {
        serde_json::Value::Null => TypedValue::Null,
        serde_json::Value::Bool(b) => TypedValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                TypedValue::Int(i)
            } else {
                TypedValue::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => TypedValue::String(s.clone()),
        serde_json::Value::Array(items) => {
            TypedValue::List(items.iter().map(json_to_typed).collect())
        }
        serde_json::Value::Object(map) => {
            let mut out = IndexMap::new();
            for (k, v) in map {
                out.insert(k.clone(), json_to_typed(v));
            }
            TypedValue::Map(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_tree_diff_detects_all_change_kinds() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"one").unwrap();
        let before = FileTree::snapshot(dir.path()).unwrap();
        std::fs::write(dir.path().join("a.txt"), b"two").unwrap();
        std::fs::write(dir.path().join("b.txt"), b"new").unwrap();
        let after = FileTree::snapshot(dir.path()).unwrap();
        let (added, removed, modified) = before.diff(&after);
        assert_eq!(added, vec!["b.txt"]);
        assert!(removed.is_empty());
        assert_eq!(modified, vec!["a.txt"]);
        assert_ne!(before.digest(), after.digest());
    }

    #[test]
    fn typed_values_are_passed_as_arguments() {
        assert_eq!(typed_to_arg(&TypedValue::String("x".into())), "x");
        assert_eq!(typed_to_arg(&TypedValue::Int(3)), "3");
        assert_eq!(typed_to_arg(&TypedValue::Null), "");
    }
}
