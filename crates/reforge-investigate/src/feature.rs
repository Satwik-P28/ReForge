//! Feature encoding for neural action ranking.
//!
//! The layout below is the **single source of truth**. Training data is
//! exported as JSONL rows containing the already-encoded vector, and the Rust
//! scorer consumes the same encoding at inference time, so training and
//! serving cannot drift.

use crate::policy::DecisionContext;

/// Number of features in [`encode`]. Bump with the layout.
pub const FEATURE_DIM: usize = 29;

/// Buckets used for the hashed action identity.
pub const ACTION_BUCKETS: usize = 16;

/// Human-readable description of each index, exported with datasets.
pub fn feature_layout() -> Vec<&'static str> {
    vec![
        "action_bucket_0",
        "action_bucket_1",
        "action_bucket_2",
        "action_bucket_3",
        "action_bucket_4",
        "action_bucket_5",
        "action_bucket_6",
        "action_bucket_7",
        "action_bucket_8",
        "action_bucket_9",
        "action_bucket_10",
        "action_bucket_11",
        "action_bucket_12",
        "action_bucket_13",
        "action_bucket_14",
        "action_bucket_15",
        "pair_samples_norm",
        "pair_outcome_entropy",
        "pair_novelty",
        "unvisited_successor_mass",
        "pair_successor_entropy",
        "class_visits_norm",
        "action_global_count_norm",
        "budget_fraction_remaining",
        "discovered_classes_norm",
        "consecutive_failures_norm",
        "pair_success_rate",
        "pair_error_diversity",
        "bias",
    ]
}

fn action_bucket(action: &str) -> usize {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in action.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    (hash % ACTION_BUCKETS as u64) as usize
}

/// Encode a candidate action in the current decision context.
pub fn encode(ctx: &DecisionContext, action: &str) -> Vec<f64> {
    let mut v = vec![0.0; FEATURE_DIM];
    let bucket = action_bucket(action);
    v[bucket] = 1.0;

    let samples = ctx.model.samples(ctx.class, action);
    v[16] = (samples as f64 / 5.0).min(1.0);
    v[17] = ctx.model.outcome_entropy(ctx.class, action);
    v[18] = if samples == 0 { 1.0 } else { 0.0 };
    v[19] = ctx
        .model
        .unvisited_successor_mass(ctx.class, action, ctx.abstraction);
    v[20] = ctx.model.successor_entropy(ctx.class, action);
    let class_visits: u32 = ctx
        .model
        .pairs
        .iter()
        .filter(|((c, _), _)| c == ctx.class)
        .map(|(_, stats)| stats.samples)
        .sum();
    v[21] = (class_visits as f64 / 10.0).min(1.0);
    v[22] = (ctx.model.action_counts.get(action).copied().unwrap_or(0) as f64 / 10.0).min(1.0);
    v[23] = if ctx.max_actions == 0 {
        0.0
    } else {
        1.0 - (ctx.step as f64 / ctx.max_actions as f64).min(1.0)
    };
    v[24] = (ctx.abstraction.classes.len() as f64 / 10.0).min(1.0);
    v[25] = (ctx.consecutive_failures as f64 / 3.0).min(1.0);
    v[26] = ctx
        .model
        .stats(ctx.class, action)
        .map(|stats| {
            stats
                .outcomes
                .get(&crate::model::OutcomeKind::Success)
                .copied()
                .unwrap_or(0) as f64
                / stats.samples.max(1) as f64
        })
        .unwrap_or(0.5);
    v[27] = ctx
        .model
        .stats(ctx.class, action)
        .map(|stats| (stats.error_codes.len() as f64 / 3.0).min(1.0))
        .unwrap_or(0.0);
    v[28] = 1.0;
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abstraction::AbstractionSummary;
    use crate::model::BehaviorModel;
    use reforge_observe::{ActionAlphabet, ActionInstance};

    #[test]
    fn layout_matches_dimension() {
        assert_eq!(feature_layout().len(), FEATURE_DIM);
    }

    #[test]
    fn encoding_shape_and_one_hot() {
        let alphabet = ActionAlphabet::default();
        let cands = vec![ActionInstance::new("x")];
        let model = BehaviorModel::new();
        let summary = AbstractionSummary::default();
        let evidence = std::collections::BTreeMap::new();
        let ctx = DecisionContext {
            class: "s0",
            initial_class: "s0",
            alphabet: &alphabet,
            candidates: &cands,
            model: &model,
            abstraction: &summary,
            step: 0,
            max_actions: 10,
            consecutive_failures: 0,
            evidence_bonus: &evidence,
        };
        let v = encode(&ctx, "x");
        assert_eq!(v.len(), FEATURE_DIM);
        let hot: f64 = v[..ACTION_BUCKETS].iter().sum();
        assert_eq!(hot, 1.0);
        assert_eq!(v[28], 1.0);
        assert_eq!(v[18], 1.0, "untried pair flagged as novel");
    }

    #[test]
    fn encoding_is_deterministic() {
        let alphabet = ActionAlphabet::default();
        let cands = vec![ActionInstance::new("y")];
        let model = BehaviorModel::new();
        let summary = AbstractionSummary::default();
        let evidence = std::collections::BTreeMap::new();
        let ctx = DecisionContext {
            class: "s1",
            initial_class: "s1",
            alphabet: &alphabet,
            candidates: &cands,
            model: &model,
            abstraction: &summary,
            step: 3,
            max_actions: 20,
            consecutive_failures: 0,
            evidence_bonus: &evidence,
        };
        assert_eq!(encode(&ctx, "y"), encode(&ctx, "y"));
    }
}
