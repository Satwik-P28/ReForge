# Evidence model (Phase 1)

ReForge separates **claims about behavior** from **evidence records** and from **payload bytes**. The Phase 1 store is content-addressed by SHA-256. Hierarchy labels are explicit classifications — not fabricated reliability percentages.

Primary types live in `reforge-types` (`EvidenceRecord`, `BehavioralClaim`, `EvidenceRelation`, `ClaimExplanation`, `VerificationRecord`). Runtime graph + store: `reforge-evidence`.

## Claim types

### BehavioralClaim

| Field | Meaning |
| --- | --- |
| `id` | Claim identity |
| `subject` | What the claim is about (e.g. operation name) |
| `statement` | Human-readable assertion |
| `certainty` | `Certainty` enum |
| `supporting_evidence` | Evidence ids |
| `contradicting_evidence` | Evidence ids |
| `derived_from` | Evidence/claim provenance ids |
| `verified_by_tests` | Test ids |
| `notes` | Optional honesty |

Claims are not automatically true because they exist in a package. Certainty must be read with the linked evidence hierarchy and integrity status.

### VerificationRecord

Records that a specific test exercised a contract/requirement under a stated environment:

- Inputs / expected / actual as `TypedValue` maps
- `status`: `passed`, `failed`, `unsupported`, `inconclusive`, `not_executed`
- Optional `counterexample` and diagnostics

Verification under test ≠ universal proof. Prefer `Certainty::VerifiedUnderTest` when linking such records to claims.

## Evidence records

`EvidenceRecord` metadata (payload is **not** inlined as the primary storage form):

| Field | Meaning |
| --- | --- |
| `evidence_type` | `observation`, `specification`, `documentation`, `test_result`, `trace`, `manifest`, or `other` |
| `source` | `user` / `tool` / `artifact` / `package` / `unknown` |
| `acquisition_method` | `manual`, `import`, `generated_fixture`, `runtime_capture`, `static_extraction`, … |
| `collection_context` / `timestamp` / `environment` | Provenance context |
| `artifact_id` | Optional linked `SoftwareArtifact` |
| `content_digest` | `{ "algorithm": "sha256", "hex": "..." }` |
| `payload_ref` | Store key / relative ref (no `..`) |
| `related_claims` | Claim ids |
| `tool` / `tool_version` | Acquisition tooling |
| `integrity` | `unverified`, `verified`, `missing`, `corrupted` |
| `hierarchy` | See below |
| `metadata` | String map |

### Evidence hierarchy

`EvidenceHierarchy` (explicit, no percentages):

- `direct_observation`
- `user_requirement`
- `documentation_derived`
- `static_analysis_inference`
- `runtime_analysis_inference`
- `hypothesis`
- `test_result`
- `unspecified` (default)

Observations, specifications, and inferences remain **separate records**. The graph does not collapse them into a single score.

## Provenance relationships

`EvidenceRelation`:

| Kind | Meaning |
| --- | --- |
| `supports` | Evidence supports claim |
| `contradicts` | Evidence contradicts claim (sets claim certainty to `contradictory`) |
| `derived_from` | Claim derived from evidence |
| `test_verifies` | Test id verifies claim |
| `collected_from` | Evidence collected from artifact |
| `hypothesis_depends` | Hypothesis depends on another id |

`EvidenceGraph::link` updates claim fields when supporting/contradicting/derived/test links are added. Unknown evidence or claim ids fail the link.

`explain(claim_id)` returns `ClaimExplanation` with supporting/contradicting records, linked tests, `unresolved_contradiction` (both sides non-empty), and narrative points. Narrative text is explanatory, not a verdict engine.

## Canonicalization and hashing

- **JSON documents** (manifests, twin objects when digested via canon helpers): RFC 8785 JCS via `serde_json_canonicalizer`, then SHA-256 (`reforge-canon::digest_of_canonical`).
- **Raw payloads** in `ContentStore`: SHA-256 over raw bytes (`digest_of_raw`). File names under `evidence/payloads/` are expected to be the lowercase hex digest.
- Key order in JSON does not change canonical digests; content changes do.
- Package `content_digest` is over the **canonicalized manifest object**, not the entire directory tree.

Tamper detection: `ContentStore::verify` returns `verified` / `missing` / `corrupted`. `verify_all_integrity` refreshes record integrity fields. Test-only `inject_corruption_for_test` exists for demos — not a production mutator API.

## Contradictions

- Linking `contradicts` marks the claim `Certainty::Contradictory`.
- `unresolved_contradiction` is true when both supporting and contradicting evidence lists resolve to present records.
- Phase 1 does **not** auto-resolve contradictions, vote, or drop evidence.
- Twin-level hypothesis conflicts (`HypothesisConflict`) are behavioral-engine outcomes, distinct from evidence-graph contradictions, though both signal competing interpretations.

## Verification limitations

Be explicit when consuming this model:

| Guaranteed | Not guaranteed |
| --- | --- |
| Digest match of stored bytes | Authentic authorship / signatures |
| Structural link integrity (ids resolve when linking) | Completeness of collected evidence |
| Replay of **modeled** twin traces | Equivalence to external products |
| Test record status as stored | That unlisted behaviors are impossible |
| Hierarchy labels as authored | Bayesian confidence or “trust scores” |

Absence of contradicting evidence is not evidence of absence. `generated_fixture` acquisition is honest about synthetic origins — treat fixture evidence accordingly.

## Package integration

Typical package wiring:

1. Put payload bytes in `evidence/payloads/<sha256>`.
2. Author `EvidenceRecord` with matching `content_digest` / `payload_ref`.
3. Author `BehavioralClaim` and `supports` / other relations in `evidence/graph.json`.
4. Reference evidence ids from the manifest and twin.

CLI:

```bash
reforge evidence verify examples/notes
reforge evidence explain examples/notes --claim note-creation
```
