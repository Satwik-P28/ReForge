# RFM-001 Requirement Compliance Matrix

**Specification:** `REFORGE_MASTER_SPEC.md`, Document ID RFM-001, version 1.0.0  
**Authoritative commit:** `3c349c11855b5d46a340251b233a40610b8d6021`  
**Audited tree:** `19d81165f987823138f0d9d85f575a3976df7e02` (spec cherry-picked onto `724f629`)  
**Branch:** `develop/neural-symbolic`  
**Audit date:** 2026-11-12

This is a baseline compliance audit, not a claim that the matrix itself makes a feature complete. No implementation feature status was changed by adding the specification and this matrix. Future phases must name the requirement IDs they advance and must identify the acceptance test that changed the status.

Status vocabulary follows RFM-001 §0.2: `not-started`, `research`, `prototype`, `implemented`, `verified-under-test`, `production-ready`, `blocked`. `verified-under-test` means the cited end-to-end test or frozen evaluation was run; it does not imply universal correctness.

## Change compliance table

| Requirement ID | Status before | What changed | Verification evidence | Status after | Remaining gap |
| --- | --- | --- | --- | --- | --- |
| OPS-001 | Not enforced by a repository-wide matrix | Imported RFM-001 and added this ID-indexed matrix | Spec commit `3c349c1`; this file | implemented | Every future phase must update affected rows and name acceptance tests |
| OPS-002 | Local history preserved | Cherry-picked spec onto existing Phase 9 commit without reset or rebase | `git log`: `19d8116` on `724f629` on `8a40181` | verified-under-test | None for this operation |
| OPS-003 | Local HEAD checked | Verified local HEAD, parent and tree before cherry-pick | HEAD `19d8116`, parent `724f629`, tree `3aff6b3` | verified-under-test | Remote backup of the full development branch remains unpushed by authorization |
| OPS-007 | Quality gates passed before this task | Re-ran focused end-to-end and extraction tests after the spec import | `e2e_composition`, `e2e_export`, `portability_audit`, `adversarial`, `extraction` all passed | verified-under-test | Full workspace gate is recorded in the Phase 9 report; no code changed here |
| OPS-012 | Prior Phase 9 commit included an unrequested co-author trailer | Did not rewrite published/development history; disclosed the deviation here | `git log 724f629` | deviation recorded | Future commits must omit co-author trailers and attribution unless requested |

## Requirement matrix

### User journeys (USR)

| ID | Status | Evidence | Remaining acceptance gap |
| --- | --- | --- | --- |
| USR-001 | prototype | CLI investigate/reconstruct/differential pipeline; Phase 8/9 frozen evaluations | No visual fidelity; no arbitrary target coverage; no complete unfamiliar workflow |
| USR-002 | prototype | Capability/UI decoupling and composition IR; export runner | No redesign UI or independent visual binding; no behavior-preserving redesign test |
| USR-003 | prototype | Notes+Tasks, Inventory+Tasks, Notes+Labels composition e2e tests | No extraction of one capability from A and another from B into one verified app |
| USR-004 | prototype | `reforge init`; composition compiler; example projects | No request-to-application assembly or template/capability selection |
| USR-005 | verified-under-test | `e2e_export` builds and runs in a clean temporary directory | Only `local-web`/Rust export is tested; no editable source workspace UI |
| USR-006 | prototype | Deterministic replay, differential verification and regression suites | No impact analysis or stale-test workflow after a capability/source edit |
| USR-007 | prototype | Evidence graph, package inspect, CLI evidence explain, Studio package inspect | No complete evidence inspector for raw evidence, contradictions and provider coverage |

### Architectural invariants (INV)

| ID | Status | Evidence | Remaining acceptance gap |
| --- | --- | --- | --- |
| INV-001 | verified-under-test | No-LLM workspace tests, deterministic benchmark runs, fixed seeds | No proof for every target/provider; optional browser and AI paths remain variable |
| INV-002 | implemented | Evidence kinds, unknowns and contradictions are retained in typed models | Not every product surface exposes unknown/contradicted evidence |
| INV-003 | verified-under-test | Exported app builds without workspace; extraction test runs without twin/reference | Only supported capability subset; no broad portability proof |
| INV-004 | verified-under-test | Data-aware comparison covers state, status, error, outputs, events, persistence | Visual/interaction and cross-capability channels are separate and incomplete |
| INV-005 | verified-under-test | Extraction test deletes twin and runs held-out input; provenance retained | Generated implementation is an observed abstract machine, not domain CRUD |
| INV-006 | implemented | Capability package and runtime are independent of any UI | No implemented visual designer or round-trip UI binding |
| INV-007 | verified-under-test | Typed event bindings, relationships and cross-capability e2e tests | Only authored fixtures; no general semantic composition of arbitrary capabilities |
| INV-008 | prototype | Authorization enum/gate, resource limits, timeouts, unsafe-path checks, loopback-only Studio | No OS-level sandbox for untrusted executables/analyzers |
| INV-009 | implemented | Development, frozen holdout and independent external results are separately labeled | Reporting discipline must be maintained for every future phase |
| INV-010 | implemented | RFM-001 matrix covers all families, including Studio and composition | Repository still lacks several planned product surfaces |
| INV-011 | verified-under-test | Existing crates are used by composition, verification and export tests | Further work must avoid duplicate engines/providers |
| INV-012 | verified-under-test | `e2e_export` clean build/run; export report lists dependencies | Export permissions/licenses are not yet surfaced in a UI |

### Investigation and REA (INVEST)

| ID | Status | Evidence | Remaining acceptance gap |
| --- | --- | --- | --- |
| INVEST-001 | verified-under-test | Pinned REA 6.7.0 run in Phase 7; raw evidence and version recorded | Only tested JavaScript/Electron analysis; broad target/API audit incomplete |
| INVEST-002 | prototype | `ReaConfig`, provider detection, version/timeout/size/errors | No general provider trait across CLI/web/platform providers |
| INVEST-003 | verified-under-test | Phase 7 REA analysis imported into evidence graph and package provenance | No demonstrated final reconstruction benefit from the imported facts |
| INVEST-004 | verified-under-test | Raw evidence bytes/digest retained separately from compact projection | Projection schema is narrow; large-app incremental views absent |
| INVEST-005 | verified-under-test | Benchmarks run without REA; REA detection errors are recorded/ignored | No complete matrix of malformed/timeout/crash degradation |
| INVEST-006 | verified-under-test | Phase 7 matched arms; REA guidance did not certify behavior, and result was negative | Guidance influence remains not measurably beneficial |
| INVEST-007 | prototype | `max_evidence_bytes`, bounded projections | No incremental analysis or bounded multi-provider evidence store |
| INVEST-008 | not-started | No ERA or ReversePort integration | Audit and integration not begun |
| INVEST-009 | prototype | REA version pin and benchmark target hash locks | No repository-wide provider responsibility matrix/license lockfile |
| INVEST-010 | verified-under-test | Source-assisted REA arm and black-box arm are labeled separately | Labels must be carried into future reports |
| INVEST-011 | not-started | Target schemas are manually specified | No content/manifest-based target router |
| INVEST-012 | prototype | Explicit target specs; REA optional/unavailable fallback | No automated compatible-provider selection |
| INVEST-013 | prototype | Evidence graph can hold multiple observations | No tested cross-provider contradiction merge |
| INVEST-014 | prototype | `Authorization` gate blocks unverified execution | Permissions are not capability-scoped or consistently enforced across every provider |
| INVEST-015 | prototype | Target schemas and Phase reports identify manual action schemas | No structured field/record on every result for schema origin |

### Evidence graph (EVD)

| ID | Status | Evidence | Remaining acceptance gap |
| --- | --- | --- | --- |
| EVD-001 | verified-under-test | Evidence graph links claims to provenance; notes evidence test | Not every derived rule carries target revision/environment in one uniform record |
| EVD-002 | implemented | Typed evidence kinds distinguish observation, specification, inference, hypothesis and test | Product UI does not expose all distinctions |
| EVD-003 | implemented | Supporting and contradicting evidence fields; no averaged confidence | Contradictions are not surfaced consistently across all reports |
| EVD-004 | verified-under-test | Phase 7 raw evidence digest, raw path and projection retained | Transformation lineage for every projected field is not exhaustive |
| EVD-005 | verified-under-test | Content-addressed payload store; tamper test passes | Missing/corrupt status presentation is not complete |
| EVD-006 | prototype | Evidence links claims to packages/rules/tests in some pipelines | No complete UI action → code → state → DB → capability graph |
| EVD-007 | implemented | Rules require observations, not names; evidence policy documented | No automated detector for name-only claims |
| EVD-008 | prototype | Target revisions/environment appear in benchmark/evidence metadata | Reuse compatibility is not enforced by the evidence store |
| EVD-009 | prototype | Provenance retained through extraction and package export | No complete UI-edit/composition round-trip provenance |
| EVD-010 | verified-under-test | Evidence inspection works without AI; no AI required in tests | Evidence inspector UI is not implemented |

### Active investigation (ACT)

| ID | Status | Evidence | Remaining acceptance gap |
| --- | --- | --- | --- |
| ACT-001 | implemented | Versioned hypotheses, support/contradiction evidence, experiments | No universal hypothesis lifecycle across all providers |
| ACT-002 | verified-under-test | Reset-backed CLI investigation tests; typed params and guards | Browser form acceptance remains unverified |
| ACT-003 | implemented | Baseline/pre/post/reset epochs and absent-persistence handling | Lazy initialization is only partially modeled |
| ACT-004 | implemented | Causal deltas and competing effects (preserve/assign/increment/create/delete/conditional) | Some nested/conditional cases remain ambiguous or unsupported |
| ACT-005 | implemented | Typed parameter identity; integer/string/null/generated distinctions | No exhaustive parameter-class coverage |
| ACT-006 | implemented | Hybrid experiment scheduler with competing predictions | No proof of optimal distinguishing experiments |
| ACT-007 | verified-under-test | Repeated proposals counted as zero in benchmark accounting | Deduplication is heuristic, not formally complete |
| ACT-008 | verified-under-test | Benchmark counts resets, setup, actions, observations, verification | Analysis-provider cost is recorded but not uniformly in every suite |
| ACT-009 | verified-under-test | Global budget exhaustion yields partial results with accounting | No repair strategy after exhaustion |
| ACT-010 | verified-under-test | Random, coverage, hypothesis, prerequisite policies compared | No AALpy/established automata baseline |
| ACT-011 | verified-under-test | REA-guided arm tested under matched conditions | Measured result was neutral/negative |
| ACT-012 | prototype | No random/time/external-service normalization policy is fully specified | Unstable outputs/environments remain target-specific gaps |
| ACT-013 | implemented | Hidden workflows are outside investigation input in benchmark harness | Must be audited for every new suite |
| ACT-014 | prototype | Benchmark reports action/state/rule/guard/error coverage | No complete coverage view by output field/provider/persistence class |

### Semantic core (SEM)

| ID | Status | Evidence | Remaining acceptance gap |
| --- | --- | --- | --- |
| SEM-001 | verified-under-test | Behavioral Twin schema and engine tests | Existing Twin is not a complete state/output model by itself |
| SEM-002 | verified-under-test | Expression engine depth-limited; adversarial deep-nesting test | No host evaluation or hidden shell callbacks; must remain so |
| SEM-003 | implemented | Observed/specified/inferred/hypothetical/unknown/contradictory labels in types | Not all labels are exposed in every execution/report path |
| SEM-004 | verified-under-test | Data-aware contract compares state/outcome/error/output/event/persistence | Visual behavior is outside this contract |
| SEM-005 | implemented | Typed identity, create/delete, list/map/set, derived aggregates, multi-record creation | Broad nested/conditional transformations still evidence-limited |
| SEM-006 | implemented | Absence/null/zero/false/empty distinctions in typed values and contracts | No dedicated mutation test for every distinction |
| SEM-007 | implemented | Typed output expressions with execution-stage semantics | Some output projections remain unimplemented |
| SEM-008 | verified-under-test | Output-contract tests reject state-only exact matches | No universal output completeness proof |
| SEM-009 | implemented | Ambiguity retained when contrasts are insufficient | Guard/effect alternatives are not fully user-visible |
| SEM-010 | implemented | Effects are checked against every branch observation | Some candidates still fit incomplete evidence |
| SEM-011 | prototype | Package schema migrations exist; DataProgram schema is versioned | No DataProgram migration path beyond the current version |
| SEM-012 | implemented | REA projects into existing evidence/guidance types, not a second IR | Projection coverage remains narrow |
| SEM-013 | implemented | Rule provenance includes dependencies/parameters/state-update claims | Type assumptions are not exhaustive for every expression |
| SEM-014 | implemented | Unsupported/unknown actions remain unknown in data programs | Declared operation coverage is not universally machine-readable |

### Capability packages (CAP)

| ID | Status | Evidence | Remaining acceptance gap |
| --- | --- | --- | --- |
| CAP-001 | implemented | `CapabilityPackage` includes interfaces, models, contracts, permissions, runtime requirements, implementation status, evidence/tests | Some package fields are not exercised by every example |
| CAP-002 | verified-under-test | `implementation_status` validation and spec-only/partial fixtures | No universal verified executable status for arbitrary extraction |
| CAP-003 | verified-under-test | Package evidence links and digest checks | Not every package operation has a direct evidence/test edge |
| CAP-004 | verified-under-test | Extraction test produces loadable, executable package | Domain CRUD beyond observed abstract machine is absent |
| CAP-005 | verified-under-test | Packages execute through runtime independent of Studio UI | No alternate UI binding implementation |
| CAP-006 | verified-under-test | Hand-authored Notes/Tasks/Inventory/Labels packages load and run | Imported third-party packages are not broadly tested |
| CAP-007 | verified-under-test | Validation rejects unsafe paths, future schemas, bad deps, cyclic deps | Resource permission enforcement is incomplete |
| CAP-008 | prototype | Faulty twin is rejected by independent expectation | No general implementation-swap regression workflow |
| CAP-009 | verified-under-test | Phase 7 extracts Twin/DataProgram into executable package operations | Narrow partial reconstruction; no full schema/invariant synthesis |
| CAP-010 | verified-under-test | Portability audit rejects builtin fallback/copied-reference execution | Must be extended to every export target |
| CAP-011 | verified-under-test | Clean import/export, digest, round-trip tamper tests | Reproducible build is local, not CI-enforced |
| CAP-012 | prototype | Global NOTICE/THIRD_PARTY_LICENSES and export docs | Per-package copied/generated distinction is not automatic |

### Composition (COMP)

| ID | Status | Evidence | Remaining acceptance gap |
| --- | --- | --- | --- |
| COMP-001 | verified-under-test | Manifest → Application IR e2e tests | Schema is v0.1-oriented |
| COMP-002 | verified-under-test | Dependency/version/contract resolution tests | Broad semver solver and optional-dependency policy remain limited |
| COMP-003 | verified-under-test | Instance namespaces and isolated runtime state in e2e | No proof for every package layout |
| COMP-004 | verified-under-test | Typed event→command/query binding tests | Only event-source bindings are supported in v0.1 |
| COMP-005 | verified-under-test | Relationship identity mapping and link policies | General identity reconciliation remains limited |
| COMP-006 | verified-under-test | Unlink/reject/cascade policy tests | Cascade depends on explicit relationship metadata |
| COMP-007 | prototype | Event queue depth limits, SQLite transactions, error propagation | No general compensation/transaction boundary across capabilities |
| COMP-008 | prototype | Structural/dependency diagnostics | No complete conflicting-invariant/permission analysis |
| COMP-009 | verified-under-test | Inventory/Tasks tests assert no Notes+Tasks hardcoding | Must keep generic compiler constraints |
| COMP-010 | verified-under-test | Notes+Tasks and Inventory+Tasks reproducible standalone compositions | Two authored capabilities only; no broad extracted composition |
| COMP-011 | prototype | IR/provenance records packages and bindings | No full user-requirement-to-test trace |
| COMP-012 | prototype | Regression tests can be rerun manually | No automatic stale/impact revalidation |

### Verification (VER)

| ID | Status | Evidence | Remaining acceptance gap |
| --- | --- | --- | --- |
| VER-001 | verified-under-test | Data-aware comparison covers declared channels | Visual/order/nondeterministic channels are incomplete |
| VER-002 | implemented | Training/development/frozen/independent reports are separate | Must be preserved for every new suite |
| VER-003 | verified-under-test | Attempted/executed/matching/diverged/unverified/excluded accounting | Some older reports use only matching/attempted |
| VER-004 | implemented | Baseline mismatch notes in data-aware replay | No universal baseline-mismatch channel |
| VER-005 | implemented | Structured divergence details and counterexample traces | Minimization is heuristic |
| VER-006 | prototype | Adversarial/faulty-twin/corruption tests exist | No exhaustive mutation suite for IDs, guards, events, persistence, composition |
| VER-007 | verified-under-test | Export and extraction tests run without reference; verifier may call reference separately | Only supported subset |
| VER-008 | prototype | Evidence payloads/content digests are immutable | Raw benchmark artifacts under `bench-results/` are gitignored and not universally content-addressed |
| VER-009 | verified-under-test | Phase 9 repairs reran historical suites and affected steps | Future repairs must follow the same discipline |
| VER-010 | verified-under-test | Hidden workflows are not mixed into investigation samples | Must audit every harness |
| VER-011 | implemented | Frozen suite hashes and versioning rules documented | No automated change detector |
| VER-012 | verified-under-test | Reports explicitly count unverified/excluded steps and reject “zero divergence = correct” | Must continue |
| VER-013 | implemented | Reports name target version, subset, environment and budget | Some claims still need explicit peak-memory/reference-cost detail |
| VER-014 | verified-under-test | Clean export build is separate from fidelity claims | Fidelity remains target-subset-specific |

### Studio (STU)

| ID | Status | Evidence | Remaining acceptance gap |
| --- | --- | --- | --- |
| STU-001 | prototype | Studio Lite loads composition, snapshots state, inspects package | No target selection/investigation/unknown browsing |
| STU-002 | not-started | No evidence inspector UI | Raw evidence/contradiction/provider coverage not exposed |
| STU-003 | prototype | Package inspect endpoint | No browse/import/edit/test library |
| STU-004 | not-started | No architecture graph UI | Dependencies/change impact not visualized |
| STU-005 | not-started | No visual designer | Faithful/custom UI design absent |
| STU-006 | not-started | No source workspace UI | Generated source/config editing absent |
| STU-007 | prototype | Verify endpoint exists in Studio | No center with mutations/counterexamples/last verified revision |
| STU-008 | not-started | No benchmark lab UI | Algorithm comparison not exposed |
| STU-009 | prototype | Export endpoint exists in Studio | No export center with permissions/licenses/reproduction details |
| STU-010 | not-started | No optional agent approval UI | No reviewable AI proposals |
| STU-011 | not-started | No visual layout editing | No behavior-preservation test |
| STU-012 | not-started | No source/package edit graph | No stale-test marking |
| STU-013 | prototype | Capabilities are UI-independent | No faithful/custom UI switch |
| STU-014 | not-started | No generated/user code ownership rules | No merge/overwrite policy |
| STU-015 | implemented | Documentation and release reports state preview is not export behavior | Must keep preview separate from verification |

### Platform and export (PLT)

| ID | Status | Evidence | Remaining acceptance gap |
| --- | --- | --- | --- |
| PLT-001 | implemented | Reconstruction target and output runtime are separate models | Not exercised across all target types |
| PLT-002 | verified-under-test | Local-web/Rust export clean build | No additional runtime target yet |
| PLT-003 | prototype | Local-web exporter exists | Electron/native desktop adapters absent |
| PLT-004 | prototype | CLI targets run on Linux; platform adapters are separate | Windows/macOS/Linux UI/tracing providers absent |
| PLT-005 | not-started | No mobile investigation/export | Later-stage work |
| PLT-006 | prototype | Persistence and behavior survive local runtime | Cross-runtime migrations unverified |
| PLT-007 | verified-under-test | Exported app builds/runs in a clean temporary directory without reference | Only supported local-web target |
| PLT-008 | implemented | Package runtime requirements/permissions are declared | OS/resource assumptions not uniformly enforced |
| PLT-009 | prototype | Verification separates functional and visual claims | No browser/visual fidelity test suite |

### Optional AI (AI)

| ID | Status | Evidence | Remaining acceptance gap |
| --- | --- | --- | --- |
| AI-001 | verified-under-test | All deterministic tests and benchmarks run without an LLM | Browser target remains environment-limited |
| AI-002 | not-started | No LLM proposal integration | Neural scorer exists only as optional ranking |
| AI-003 | implemented | Neural ranking is opt-in; verification remains authoritative | No LLM-generated code/contract validation path |
| AI-004 | implemented | Reports/docs treat target text as untrusted; no AI authority path | No LLM sandbox to test yet |
| AI-005 | verified-under-test | Benchmarks state no AI/LLM was used; neural is optional | No unified AI cost report because no LLM integration |
| AI-006 | verified-under-test | Deterministic policies preferred; neural not claimed superior | Must keep this default |
| AI-007 | verified-under-test | Neural vs random/coverage/hypothesis matched comparison; mixed result | No AALpy or established automata baseline |
| AI-008 | verified-under-test | No simulated reference outputs in benchmark data program path | Must maintain |

### ReForgeBench (BENCH)

| ID | Status | Evidence | Remaining acceptance gap |
| --- | --- | --- | --- |
| BENCH-01 | prototype | Coverage/policy transition metrics on frozen CLI suites | No hidden-feature recall under a fixed total interaction budget across domains |
| BENCH-02 | verified-under-test | Unseen hidden-workflow prediction benchmarks (v0.2/v0.3, Phase 8/9) | Generalization remains weak; development vs independent separated |
| BENCH-03 | verified-under-test | Exact workflow pass rates on frozen external suites | 0/8 on Phase 9 npm/buku; 12/12 on External v3 development |
| BENCH-04 | not-started | No visual fidelity benchmark | Viewports/screenshots absent |
| BENCH-05 | prototype | Composition e2e tests | No benchmark track for cross-feature correctness/invariants |
| BENCH-06 | not-started | No cross-runtime parity benchmark | Single local-web export target |
| BENCH-07 | not-started | No reference-free hidden-requirement suite | Extraction independence test is not a full track |
| BENCH-08 | prototype | Faulty twin, tamper and adversarial tests | No mutation-score/precision-recall benchmark |
| BENCH-101 | implemented | Suite configs include versions/hashes/seeds/budgets; Phase 6.6 includes peak RSS | Not every suite records OS/tool versions/peak memory |
| BENCH-102 | verified-under-test | Reference interaction accounting includes resets/setup/probes/verification | Analysis-provider cost is separate and not always included |
| BENCH-103 | verified-under-test | Black-box/source-assisted/REA arms separated; no equal-information implication | LLM arm absent |
| BENCH-104 | verified-under-test | Phase 7/8 matched budgets and arms | Some historical cells not fully executed |
| BENCH-105 | verified-under-test | Phase 8 preflight/freeze and hashes | Later suites must repeat |
| BENCH-106 | verified-under-test | Phase 8 heldout frozen before execution; Phase 9 reclassified as development | Must not tune frozen holdouts later |
| BENCH-107 | verified-under-test | Negative Phase 8/9 and REA results preserved | Must continue |
| BENCH-108 | prototype | Standalone exported programs and raw local artifacts exist | Raw artifacts are gitignored; licensing/reproducibility incomplete |
| BENCH-109 | verified-under-test | State/status/error/output/event/persistence/workflow metrics separated | Visual channels absent |
| BENCH-110 | verified-under-test | npm/buku and External v3 are third-party software | Coverage remains narrow |
| BENCH-111 | implemented | Development scores explicitly not generalization | Must keep labels |
| BENCH-112 | verified-under-test | Phase 7 REA source-assisted arm measured a neutral/negative benefit | Guidance remains unproven |

### Security and rights (SEC)

| ID | Status | Evidence | Remaining acceptance gap |
| --- | --- | --- | --- |
| SEC-001 | prototype | Authorization enum/gate; target specs record authorization | Not a complete authorization manifest or capability-scoped policy |
| SEC-002 | prototype | Timeouts, resource limits, path checks, loopback Studio | No OS sandbox for untrusted executables/analyzers |
| SEC-003 | implemented | Untrusted target text is treated as data in agent/report policy | No automated prompt-injection detector |
| SEC-004 | prototype | No secrets intentionally stored in evidence/exports | No secret-scanning or redaction pipeline |
| SEC-005 | implemented | LICENSE/NOTICE/THIRD_PARTY_LICENSES and pinned tool hashes | Per-export transitive notice generation incomplete |
| SEC-006 | implemented | Generated vs copied provenance is represented | Not automatically distinguished in every package |
| SEC-007 | implemented | Policy prohibits access-control bypass; no such feature exists | Must remain enforced |
| SEC-008 | not-started | No pre-execution destructive-effect UI | Permissions/effects not shown before actions |
| SEC-009 | verified-under-test | Unsafe path, depth/size limits, subprocess timeout tests | Allowlisting is partial |
| SEC-010 | verified-under-test | Evidence hashes and tamper detection | Export artifact provenance is not universally signed/digested |
| SEC-011 | implemented | Verification uses fixtures/isolated copies | No production-data safeguard beyond policy |
| SEC-012 | implemented | License uncertainty reported in docs | Export UI does not expose per-package uncertainty |

### Delivery and operations (OPS)

| ID | Status | Evidence | Remaining acceptance gap |
| --- | --- | --- | --- |
| OPS-001 | implemented | This matrix names IDs; Phase 9 report names failure classes | Future phases must update statuses and acceptance tests |
| OPS-002 | verified-under-test | Local history preserved; spec cherry-picked | Full branch remains unpushed |
| OPS-003 | verified-under-test | HEAD/parent/tree verified before operation | Must repeat before future branch changes |
| OPS-004 | verified-under-test | Used `develop/neural-symbolic`; no force/rewrite | Remote backup remains a separate authorized action |
| OPS-005 | implemented | Reused existing crates; no duplicate engine introduced | Ongoing review |
| OPS-006 | verified-under-test | New IR variants have unit, serialization, execution and regression tests | Some variants still need negative tests |
| OPS-007 | verified-under-test | fmt, clippy, workspace tests and diff check passed | No CI enforcement |
| OPS-008 | verified-under-test | REA/buku/npm/external tool versions and hashes pinned | New providers need the same |
| OPS-009 | verified-under-test | Structured benchmark artifacts with seeds/budgets/hashes | Some raw artifacts are ignored/local-only |
| OPS-010 | prototype | `data.rs` is a large monolithic module | Refactor only with measured maintainability benefit |
| OPS-011 | verified-under-test | No secrets/caches committed; `.phase8-taskbook` remains untracked | Must keep |
| OPS-012 | blocked | Prior Phase 9 commit has an unrequested co-author trailer | Cannot remove without rewriting history; future commits omit it |
| OPS-013 | verified-under-test | No force-push, main merge or release performed | Push not authorized |
| OPS-014 | implemented | Negative Phase 8/9 results reported, not hidden | Must continue |

## Milestone and acceptance gates

| Gate | Status | Evidence | Remaining gap |
| --- | --- | --- | --- |
| M0 Preserve/consolidate | prototype | HEAD/tree verified; build/tests pass; Phase 8 freeze intact | No remote backup; clean-checkout reproduction not yet recorded |
| M1 REA-powered investigation | prototype | Real REA 6.7.0 analysis, raw evidence and matched arms | No measured reconstruction benefit |
| M2 Verified functional extraction | prototype | Phase 7 Taskbook capability; Phase 9 11/37 npm/buku | No fully matching unfamiliar workflow; 0/8 Phase 9 workflows |
| M3 Portable Capability Package | prototype | Extracted package loads/runs independently; import/export tests | Partial abstract machine; broad package acceptance not met |
| M4 First behavior-aware composition | verified-under-test | Notes+Tasks and Inventory+Tasks e2e, persistence/restart | Authored capabilities, not two unfamiliar extracted ones |
| M5 Studio/Evidence Inspector | not-started | Studio Lite has only composition/package/verify/export endpoints | No Explorer/Evidence Inspector |
| M6 Round-trip visual customization | not-started | No visual designer or UI binding workflow | Acceptance tests absent |
| M7 Additional platforms/exports | not-started | Only local-web/Rust export tested | No Electron/native/mobile targets |
| M8 Independent benchmark/release | prototype | Frozen external suites and negative results exist | No broad multi-app independent generalization |

## Master acceptance checklist (RFM-001 §19)

| Item | Status | Evidence | Gap |
| --- | --- | --- | --- |
| A. REA structured evidence enters graph | partial / verified-under-test for Taskbook | Phase 7 REA raw/projection/graph | No broad target coverage; no measurable benefit |
| B. Structural evidence informs hypothesis | partial | REA guidance recorded; matched arms neutral | No demonstrated causal influence on final rules |
| C. New-input stateful reconstruction | partial | Phase 7/8/9 independent execution tests | Phase 9 11/37, 0/8 workflows |
| D. Importable executable capability | partial | Extraction/package tests; Taskbook package | Partial abstract machine; broad package gate unmet |
| E. Two-capability composition | verified-under-test | Notes+Tasks and Inventory+Tasks e2e | Authored fixtures, not arbitrary extracted capabilities |
| F. Honest evidence/capability/verification UI | not-started | Studio Lite limited endpoints | No Evidence Inspector/Verification Center |
| G. Different UI binding | not-started | Capabilities are UI-independent in design | No UI switch or visual test |
| H. Clean supported exports | verified-under-test | `e2e_export` clean build/run | Only local-web target |
| I. Development/untouched/cost separation | verified-under-test | Phase 8/9 reports and summaries | Need consistent cost reporting in all phases |
| J. Threat model/auth/isolation | prototype | Security docs, authorization gate, limits | No OS sandbox or destructive-effect UI |
| K. No-LLM baseline/fair AI comparison | verified-under-test | Deterministic suite; optional neural comparison | No AALpy baseline; no LLM implementation |
| L. Reproducible state/tests/negative results | verified-under-test | Commit/tree, gates, frozen artifacts, Phase 9 report | No CI; some raw artifacts local-only |

## Current completion summary

- **Verified end-to-end in this audit:** composition/export/portability/adversarial tests and independent extraction tests.
- **Strongest verified product slice:** capability package composition, SQLite persistence, differential verification, and local-web/Rust export for authored and selected extracted capabilities.
- **Weakest areas:** Studio product surfaces, visual reconstruction, platform breadth, unrestricted REA integration, cross-capability compensation, exhaustive mutation testing, and unfamiliar-software generalization.
- **No requirement in this matrix is marked `production-ready`.**
- **No feature should be marked complete from this matrix alone.** The cited acceptance test must be rerun for the relevant commit and target.
