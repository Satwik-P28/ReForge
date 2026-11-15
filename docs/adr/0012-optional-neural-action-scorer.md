# ADR 0012 — The neural component only ranks actions

**Status:** Accepted (v0.2.0)
**Date:** 2026-11-04

## Context

The milestone calls for a neural model, but the project's core promise is that a
prediction must never silently become a verified fact. A model that writes
transitions, states, or verification results would break that promise.

## Decision

- **Scope:** the model scores `(state summary, action)` pairs. It only orders the
  candidate list handed to the policy. Everything recorded still comes from an
  executed action and its observation.
- **Feature layout is fixed in Rust** (`feature.rs`, 26 dimensions) and exported
  pre-encoded in the dataset, so training and serving cannot drift.
- **Artifact format** `reforge-explorernet/1`: a 26→16→1 MLP (ReLU hidden,
  sigmoid readout), weights as JSON, plus metrics, dataset digest, and trainer
  provenance. `ActionScorer::load` rejects a mismatched feature dimension or
  non-finite weights instead of guessing.
- **Fallback is explicit.** Requesting `--policy neural` without a usable model
  falls back to coverage and records that in `policy_used` and `notes`.
- **Training data is produced by the deterministic policies only.** Neural
  rollouts are measured but never trained on.

## Evidence

Dataset at the original 2026-11-04 run: 4050 rows from five apps × two budgets ×
three seeds × three non-neural policies. The 2026-11-05 retrain used the current
3600-row budget-60 dataset and is recorded in the audit. Split by
**application**, not by run or row: forms, inventory and taskboard train;
contacts and events are held out. The target is `label_value_5` (a new behavior
or state within five actions), computed post-hoc within an episode.

| Metric | Value |
| --- | --- |
| val loss | 0.3237 |
| val AUC | 0.8882 |
| baseline AUC (predict mean) | 0.5000 |
| train AUC | 0.8404 |
| train / validation rows | 2430 / 1620 |
| holdout positive rate | 0.940 contacts, 0.784 events |

The earlier 0.9844 result used a label that effectively duplicated an input
feature and a run-level split. It was leakage, not evidence of generalization.
The fresh retrain reached validation AUC 0.8776 and mixed exploration results
(more transitions than coverage, fewer classes); it is not a demonstrated
advantage.

**Even after the fix, neural guidance does not improve exploration.** Paired
runs, 15 per arm, budgets 30/60:

| Policy | Mean transitions | Mean classes |
| --- | --- | --- |
| coverage | 55.5 | 27.7 |
| neural | 47.0 | 20.1 |

Neural wins 4/15 paired runs on transitions and 3/15 on classes. On `cli-events`
it stalls at 10 steps / 2 classes in every seed.

## Consequences

- The ML path is real, reproducible, and optional; it is not a capability claim.
- The classifier's AUC is inflated by class imbalance and does not translate into
  action-selection value. The primary metric is incremental discovery, not AUC.
- Coverage remains the default. Neural is retained only as an experimental
  backend; any future claim needs paired budgets and an application holdout.
