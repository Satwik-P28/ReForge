//! Observation Intermediate Representation (version `2.0.0`).
//!
//! The IR exists to keep four things separate that are routinely conflated:
//!
//! 1. **Evidence** — what an adapter actually captured (`Observation`).
//! 2. **Interpretation** — abstracted states and hypotheses (`reforge-investigate`).
//! 3. **Specification** — portable capability contracts (`reforge-extract`).
//! 4. **Implementation** — an executable twin or reconstructed app.
//!
//! Every observation records *how* it was obtained, whether the environment was
//! reset, how complete it is, and what remains unknown. Inference never
//! overwrites observation: inferred hidden state lives in
//! [`Observation::inferred_hidden_state`] and carries its own basis.

use indexmap::IndexMap;
use reforge_canon::digest_of_raw;
use reforge_types::{Digest, Id, TypeDesc, TypedValue};
use serde::{Deserialize, Serialize};

/// IR version for this module. Phase 1 artifacts used `SchemaVersion` (major 1);
/// the observation IR is versioned independently so it can evolve without
/// silently reinterpreting existing capability packages.
pub const OBSERVATION_IR_VERSION: &str = "2.0.0";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IrVersion(pub String);

impl IrVersion {
    pub fn current() -> Self {
        Self(OBSERVATION_IR_VERSION.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_supported(&self) -> bool {
        // Accept 2.x; reject anything else rather than guess semantics.
        self.0.split('.').next() == Some("2")
    }
}

impl Default for IrVersion {
    fn default() -> Self {
        Self::current()
    }
}

/// Platform family an adapter speaks to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformKind {
    Web,
    Cli,
    Source,
    Electron,
    Native,
    Synthetic,
}

/// Who authorised investigation of this target.
///
/// ReForge never infers permission. An unverified authorization is recorded
/// honestly and blocks network-capable adapters at the scheduling layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authorization {
    /// Operator owns the target (their own app or a local fixture).
    OwnerOwned,
    /// Repository-provided test fixture shipped with ReForge.
    LocalFixture,
    /// Explicitly authorised third-party test target.
    AuthorizedTestTarget,
    /// No authorization recorded; read-only local investigation only.
    Unverified,
}

impl Authorization {
    pub fn allows_execution(self) -> bool {
        !matches!(self, Authorization::Unverified)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetIdentity {
    pub id: Id,
    pub name: String,
    pub platform: PlatformKind,
    /// Adapter-visible version string (e.g. `git:abc123`, `sha256:...`).
    pub version: String,
    pub authorization: Authorization,
}

/// Hard limits applied to one investigation session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub max_actions: u32,
    pub max_resets: u32,
    pub action_timeout_ms: u64,
    pub session_timeout_ms: u64,
    pub max_captured_bytes: u64,
    pub max_navigations: u32,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_actions: 200,
            max_resets: 100,
            action_timeout_ms: 2_000,
            session_timeout_ms: 120_000,
            max_captured_bytes: 8 * 1024 * 1024,
            max_navigations: 50,
        }
    }
}

impl ResourceLimits {
    /// Conservative defaults for a bounded research run.
    pub fn research_default() -> Self {
        Self {
            max_actions: 120,
            ..Self::default()
        }
    }

    pub fn with_max_actions(mut self, n: u32) -> Self {
        self.max_actions = n;
        self
    }
}

/// How the adapter locates an action target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetOrigin {
    /// Accessible role + name (portable across re-renders).
    Accessibility,
    /// Stable DOM identifier provided by the application.
    DomId,
    /// Command-line argument / subcommand.
    Argv,
    /// Route or URL path.
    Route,
    /// Opaque selector. Never carried into extracted capabilities.
    OpaqueSelector,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticTarget {
    pub role: String,
    pub name: String,
    pub origin: TargetOrigin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Click,
    Type,
    Navigate,
    Submit,
    KeyPress,
    Invoke,
    Query,
    Observe,
}

/// One element of the investigation alphabet: an action *schema*.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlphabetEntry {
    /// Stable action label used everywhere downstream (e.g. `delete_task`).
    pub action: String,
    pub kind: ActionKind,
    #[serde(default)]
    pub parameters: IndexMap<String, TypeDesc>,
    #[serde(default)]
    pub params: IndexMap<String, ParamSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<SemanticTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Where a concrete parameter value comes from when the engine instantiates an action.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParamSource {
    Literal {
        value: TypedValue,
    },
    /// A fresh identifier with the given prefix (deterministic per seed).
    FreshId {
        prefix: String,
    },
    /// The identifier of an entity most recently created in the session.
    LastEntity {
        pool: String,
    },
}

/// The finite action alphabet presented to the learner.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct ActionAlphabet {
    pub entries: Vec<AlphabetEntry>,
    /// Human-readable description of how the alphabet was obtained.
    pub discovered_by: String,
}

impl ActionAlphabet {
    pub fn labels(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.action.clone()).collect()
    }

    pub fn get(&self, action: &str) -> Option<&AlphabetEntry> {
        self.entries.iter().find(|e| e.action == action)
    }
}

/// A concrete action with resolved parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionInstance {
    pub action: String,
    pub params: IndexMap<String, TypedValue>,
}

impl ActionInstance {
    pub fn new(action: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            params: IndexMap::new(),
        }
    }

    pub fn with(mut self, key: impl Into<String>, value: TypedValue) -> Self {
        self.params.insert(key.into(), value);
        self
    }

    /// Stable label including parameters, used for traces and counterexamples.
    pub fn label(&self) -> String {
        if self.params.is_empty() {
            return self.action.clone();
        }
        let parts: Vec<String> = self
            .params
            .iter()
            .map(|(k, v)| format!("{k}={}", compact(v)))
            .collect();
        format!("{}({})", self.action, parts.join(","))
    }
}

fn compact(v: &TypedValue) -> String {
    match v {
        TypedValue::Null => "null".into(),
        TypedValue::Bool(b) => b.to_string(),
        TypedValue::Int(i) => i.to_string(),
        TypedValue::Float(f) => f.to_string(),
        TypedValue::String(s) => {
            if s.chars().count() > 24 {
                format!("{}…", s.chars().take(24).collect::<String>())
            } else {
                s.clone()
            }
        }
        TypedValue::BytesHex(b) => format!("bytes:{b}"),
        TypedValue::List(l) => format!("[{} items]", l.len()),
        TypedValue::Map(m) => format!("{{{} keys}}", m.len()),
    }
}

/// Characterization of observable state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FingerprintKind {
    /// Structured observation (semantic / accessibility / process output).
    Structured,
    /// Only pixels were available; abstraction is weaker and flagged.
    VisualOnly,
    /// Process exit code + output streams.
    Process,
}

/// A state fingerprint is a digest plus the *features* it was computed from.
///
/// Keeping features separately (rather than only a hash) is what allows state
/// abstraction to refine an over-coarse class instead of silently collapsing
/// distinct behaviors.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateFingerprint {
    pub kind: FingerprintKind,
    pub digest: Digest,
    pub features: std::collections::BTreeMap<String, String>,
}

impl StateFingerprint {
    pub fn from_features(
        kind: FingerprintKind,
        features: std::collections::BTreeMap<String, String>,
    ) -> Self {
        let mut buf = String::new();
        for (k, v) in &features {
            buf.push_str(k);
            buf.push('\u{1}');
            buf.push_str(v);
            buf.push('\u{2}');
        }
        let digest = digest_of_raw(buf.as_bytes());
        Self {
            kind,
            digest,
            features,
        }
    }

    pub fn short(&self) -> String {
        self.digest.hex.chars().take(12).collect()
    }

    pub fn feature(&self, key: &str) -> Option<&str> {
        self.features.get(key).map(|s| s.as_str())
    }
}

/// Normalized visible text: digest + bounded excerpt, never the full capture inline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedText {
    pub digest: Digest,
    pub line_count: u32,
    pub char_count: u32,
    pub excerpt: String,
}

impl NormalizedText {
    pub const EXCERPT_LIMIT: usize = 400;

    pub fn capture(text: &str) -> Self {
        let excerpt: String = text
            .chars()
            .take(Self::EXCERPT_LIMIT)
            .collect::<String>()
            .replace('\n', " ⏎ ");
        Self {
            digest: digest_of_raw(text.as_bytes()),
            line_count: text.lines().count() as u32,
            char_count: text.chars().count() as u32,
            excerpt,
        }
    }
}

/// Content-addressed artifact produced during an observation (screenshot, raw DOM, ...).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayloadRef {
    pub kind: String,
    pub digest: Digest,
    pub bytes: u64,
    pub media_type: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SideEffectObservation {
    Filesystem {
        root: String,
        added: Vec<String>,
        removed: Vec<String>,
        modified: Vec<String>,
    },
    Storage {
        area: String,
        changed_keys: Vec<String>,
    },
    Network {
        method: String,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<u16>,
    },
    Process {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
    },
    Log {
        level: String,
        text: String,
    },
    None,
}

impl SideEffectObservation {
    pub fn is_none(&self) -> bool {
        matches!(self, SideEffectObservation::None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    #[default]
    Complete,
    /// Some part of the observable surface was not captured.
    Partial,
    /// A capture limit was hit (payload size, element count, ...).
    Truncated,
}

impl Completeness {
    pub fn is_complete(self) -> bool {
        matches!(self, Completeness::Complete)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UncertaintyKind {
    /// Behavior never exercised in this session.
    Unobserved,
    /// Exercised fewer times than the configured sample floor.
    LowSamples,
    /// Result varied across identical runs.
    EnvironmentDependent,
    /// Derived from inference, not observation.
    ModelInference,
    /// Reset could not be verified, so comparisons are conditional.
    ResetUnverified,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UncertaintyNote {
    pub subject: String,
    pub kind: UncertaintyKind,
    pub statement: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Reproducibility {
    Deterministic,
    Nondeterministic {
        variants: u8,
    },
    #[default]
    Unknown,
    /// Reproducible only relative to an assumed initial state.
    ConditionalOnReset,
}

impl Reproducibility {
    pub fn is_deterministic(self) -> bool {
        matches!(self, Reproducibility::Deterministic)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResetPolicy {
    /// Target is recreated in a pristine directory / process.
    FreshEnvironment,
    /// Target state restored from a snapshot the adapter took itself.
    SnapshotRestore,
    /// Page reload / navigation to entry route.
    Reload,
    /// Process restarted with identical arguments.
    ProcessRestart,
    /// No reset capability; comparisons are conditional.
    None,
}

impl ResetPolicy {
    pub fn is_reliable(self) -> bool {
        !matches!(self, ResetPolicy::None)
    }
}

/// Evidence that a reset actually happened, not just that it was attempted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResetProvenance {
    pub policy: ResetPolicy,
    pub succeeded: bool,
    pub attempts: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_after_reset: Option<Digest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ResetProvenance {
    pub fn assumed(policy: ResetPolicy) -> Self {
        Self {
            policy,
            succeeded: false,
            attempts: 0,
            state_after_reset: None,
            detail: Some("reset not performed before this observation".into()),
        }
    }

    pub fn verified(&self) -> bool {
        self.succeeded && self.policy.is_reliable()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentInfo {
    pub platform: PlatformKind,
    pub os: String,
    /// Tool that produced the observation (e.g. `playwright`, `direct-exec`).
    pub tool: String,
    pub tool_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixture_digest: Option<Digest>,
    /// Clock model — `deterministic` when the adapter pins time.
    pub clock: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TimingInfo {
    pub action_ms: u64,
    pub observe_ms: u64,
}

/// An inference about state that was never directly observed.
///
/// Separate from observation by construction; consumers must not treat these
/// as facts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HiddenStateInference {
    pub subject: String,
    pub hypothesis: String,
    pub basis: String,
}

/// One recorded observation: an action (or baseline capture) plus its effects.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub schema_version: IrVersion,
    pub id: Id,
    pub session_id: Id,
    pub step_index: u64,
    pub target: Id,
    /// `True` for a capture that applied no action: the session's initial state
    /// or a post-reset baseline. Baseline-ness is a property of the capture, not
    /// of its position, so a baseline may legitimately occur mid-session.
    #[serde(default)]
    pub baseline: bool,
    /// `None` for a baseline capture that applied no action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<ActionInstance>,
    pub before: StateFingerprint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<StateFingerprint>,
    /// Structured, adapter-normalized outputs (never free-form UI dumps).
    #[serde(default)]
    pub outputs: IndexMap<String, TypedValue>,
    /// Bounded adapter-visible state document captured after the action.
    /// This is raw evidence for data-aware inference (entity identity,
    /// relationships and numeric fields), not an abstraction. Adapters that
    /// cannot expose a structured document leave it absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_document: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_text: Option<NormalizedText>,
    #[serde(default)]
    pub side_effects: Vec<SideEffectObservation>,
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default)]
    pub timing: TimingInfo,
    pub environment: EnvironmentInfo,
    #[serde(default)]
    pub payloads: Vec<PayloadRef>,
    #[serde(default)]
    pub evidence_refs: Vec<Id>,
    #[serde(default)]
    pub completeness: Completeness,
    #[serde(default)]
    pub uncertainty: Vec<UncertaintyNote>,
    #[serde(default)]
    pub reproducibility: Reproducibility,
    #[serde(default)]
    pub inferred_hidden_state: Vec<HiddenStateInference>,
    pub reset: ResetProvenance,
}

impl Observation {
    pub fn rejected(&self) -> bool {
        self.outputs
            .get("outcome")
            .and_then(|v| v.as_str())
            .map(|s| s == "rejected" || s == "error")
            .unwrap_or(false)
    }

    pub fn outcome(&self) -> &str {
        self.outputs
            .get("outcome")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
    }

    /// Whether the observable state changed relative to the pre-action capture.
    pub fn state_changed(&self) -> bool {
        match &self.after {
            Some(after) => after.digest != self.before.digest,
            None => false,
        }
    }

    pub fn external_effect(&self) -> bool {
        self.side_effects.iter().any(|s| !s.is_none())
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.schema_version.is_supported() {
            return Err(format!(
                "unsupported observation IR version '{}'",
                self.schema_version.as_str()
            ));
        }
        if self.baseline && self.action.is_some() {
            return Err("a baseline capture must not record an action".into());
        }
        if !self.baseline && self.action.is_none() {
            return Err("a non-baseline observation must record an action".into());
        }
        if !self.completeness.is_complete() && self.uncertainty.is_empty() {
            return Err("incomplete observation must carry an uncertainty note".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> EnvironmentInfo {
        EnvironmentInfo {
            platform: PlatformKind::Cli,
            os: "linux".into(),
            tool: "direct-exec".into(),
            tool_version: "1".into(),
            viewport: None,
            fixture_digest: None,
            clock: "deterministic".into(),
        }
    }
    fn fp(state: &str) -> StateFingerprint {
        let mut m = std::collections::BTreeMap::new();
        m.insert("state".to_string(), state.to_string());
        StateFingerprint::from_features(FingerprintKind::Process, m)
    }

    fn obs(action: Option<ActionInstance>, before: &str, after: &str) -> Observation {
        Observation {
            schema_version: IrVersion::current(),
            id: Id::new("obs-1").unwrap(),
            session_id: Id::new("s-1").unwrap(),
            step_index: 0,
            target: Id::new("t-1").unwrap(),
            baseline: action.is_none(),
            action,
            before: fp(before),
            after: Some(fp(after)),
            outputs: IndexMap::new(),
            state_document: None,
            visible_text: None,
            side_effects: vec![],
            errors: vec![],
            timing: TimingInfo::default(),
            environment: env(),
            payloads: vec![],
            evidence_refs: vec![],
            completeness: Completeness::Complete,
            uncertainty: vec![],
            reproducibility: Reproducibility::Deterministic,
            inferred_hidden_state: vec![],
            reset: ResetProvenance::assumed(ResetPolicy::FreshEnvironment),
        }
    }

    #[test]
    fn fingerprint_is_order_independent() {
        let mut a = std::collections::BTreeMap::new();
        a.insert("x".to_string(), "1".to_string());
        a.insert("y".to_string(), "2".to_string());
        let mut b = std::collections::BTreeMap::new();
        b.insert("y".to_string(), "2".to_string());
        b.insert("x".to_string(), "1".to_string());
        assert_eq!(
            StateFingerprint::from_features(FingerprintKind::Process, a),
            StateFingerprint::from_features(FingerprintKind::Process, b)
        );
    }

    #[test]
    fn fingerprint_changes_with_content() {
        let mut a = std::collections::BTreeMap::new();
        a.insert("x".to_string(), "1".to_string());
        let mut b = a.clone();
        b.insert("x".to_string(), "2".to_string());
        assert_ne!(
            StateFingerprint::from_features(FingerprintKind::Process, a).digest,
            StateFingerprint::from_features(FingerprintKind::Process, b).digest
        );
    }

    #[test]
    fn incomplete_observation_requires_uncertainty() {
        let mut o = obs(None, "a", "a");
        o.completeness = Completeness::Partial;
        assert!(o.validate().is_err());
        o.uncertainty.push(UncertaintyNote {
            subject: "dom".into(),
            kind: UncertaintyKind::Unobserved,
            statement: "below-the-fold content not captured".into(),
        });
        assert!(o.validate().is_ok());
    }

    #[test]
    fn unsupported_ir_version_rejected() {
        let mut o = obs(None, "a", "a");
        o.schema_version = IrVersion("3.0.0".into());
        assert!(o.validate().is_err());
    }

    #[test]
    fn state_change_detection() {
        let unchanged = obs(Some(ActionInstance::new("list")), "a", "a");
        assert!(!unchanged.state_changed());
        let changed = obs(Some(ActionInstance::new("create")), "a", "b");
        assert!(changed.state_changed());
    }

    #[test]
    fn action_label_is_stable() {
        let a = ActionInstance::new("create_task").with("title", TypedValue::String("x".into()));
        assert_eq!(a.label(), "create_task(title=x)");
        assert_eq!(ActionInstance::new("list").label(), "list");
    }
}
