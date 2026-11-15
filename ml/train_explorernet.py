#!/usr/bin/env python3
"""Train the optional ReForge exploration scorer on application-level holdout.

Audit notes (v0.2, second iteration):

* The previous target was "did this action reveal a new behavior immediately".
  That is almost identical to the `pair_novelty` input feature, so the reported
  0.98 AUC was a proxy, not evidence of useful guidance.
* This version trains on `label_value_5`: whether the decision led to any new
  behavior or state within the next five actions of the same episode. It also
  splits by *application*, so rows from a held-out app are never seen in
  training.
* AUC is reported, but it is not the acceptance metric. Whether the policy
  actually discovers more behavior is measured by `reforgebench_v02`.

Usage:
    python3 ml/train_explorernet.py --dataset bench-results/v02-dataset.jsonl \
        --holdout-apps cli-contacts,cli-events --out ml/model.json
"""

import argparse
import hashlib
import json
import math
import os
import sys

import numpy as np

HIDDEN_DIM = 16
DEFAULT_HOLDOUT = "cli-contacts,cli-events"


def load_rows(path):
    rows = []
    with open(path, "r", encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if line:
                rows.append(json.loads(line))
    if not rows:
        raise SystemExit(f"no dataset rows in {path}")
    return rows


def label_of(row):
    if "label_value_5" in row:
        return float(row["label_value_5"])
    return float(row.get("label_new_behavior", 0))


def to_arrays(rows, feature_dim):
    x = np.zeros((len(rows), feature_dim), dtype=np.float64)
    y = np.zeros((len(rows),), dtype=np.float64)
    for i, row in enumerate(rows):
        features = row["features"]
        if len(features) != feature_dim:
            raise SystemExit(f"row {i} has {len(features)} features, expected {feature_dim}")
        x[i] = features
        y[i] = label_of(row)
    return x, y


def sigmoid(z):
    return 1.0 / (1.0 + np.exp(-np.clip(z, -60.0, 60.0)))


def auc(y_true, scores):
    pairs = sorted(zip(scores, y_true), key=lambda p: p[0])
    n_pos = sum(1 for _, t in pairs if t > 0.5)
    n_neg = len(pairs) - n_pos
    if n_pos == 0 or n_neg == 0:
        return float("nan")
    rank_sum, i = 0.0, 0
    while i < len(pairs):
        j = i
        while j + 1 < len(pairs) and pairs[j + 1][0] == pairs[i][0]:
            j += 1
        avg_rank = (i + j) / 2.0 + 1.0
        for k in range(i, j + 1):
            if pairs[k][1] > 0.5:
                rank_sum += avg_rank
        i = j + 1
    return (rank_sum - n_pos * (n_pos + 1) / 2.0) / (n_pos * n_neg)


def train_numpy(x, y, epochs, lr=0.05, seed=7):
    rng = np.random.default_rng(seed)
    n, d = x.shape
    w1 = rng.normal(0.0, 0.1, size=(d, HIDDEN_DIM))
    b1 = np.zeros(HIDDEN_DIM)
    w2 = rng.normal(0.0, 0.1, size=(HIDDEN_DIM,))
    b2 = 0.0
    for epoch in range(epochs):
        hidden = np.maximum(0.0, x @ w1 + b1)
        pred = sigmoid(hidden @ w2 + b2)
        delta = (pred - y) / n
        grad_w2 = hidden.T @ delta
        grad_b2 = delta.sum()
        grad_hidden = np.outer(delta, w2) * (hidden > 0)
        grad_w1 = x.T @ grad_hidden
        grad_b1 = grad_hidden.sum(axis=0)
        w1 -= lr * grad_w1
        b1 -= lr * grad_b1
        w2 -= lr * grad_w2
        b2 -= lr * grad_b2
        if (epoch + 1) % 100 == 0:
            loss = -np.mean(
                y * np.log(np.clip(pred, 1e-9, 1.0))
                + (1 - y) * np.log(np.clip(1 - pred, 1e-9, 1.0))
            )
            print(f"  epoch {epoch + 1:4d} loss={loss:.4f}", file=sys.stderr)
    return w1, b1, w2, float(b2)


def predict(x, w1, b1, w2, b2):
    hidden = np.maximum(0.0, x @ w1 + b1)
    return sigmoid(hidden @ w2 + b2)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dataset", default="bench-results/v02-dataset.jsonl")
    parser.add_argument("--out", default="ml/model.json")
    parser.add_argument("--holdout-apps", default=DEFAULT_HOLDOUT)
    parser.add_argument("--epochs", type=int, default=400)
    parser.add_argument("--seed", type=int, default=7)
    args = parser.parse_args()

    rows = load_rows(args.dataset)
    feature_dim = len(rows[0]["features"])
    holdout_apps = {name.strip() for name in args.holdout_apps.split(",") if name.strip()}
    seen_apps = sorted({row.get("app", "unknown") for row in rows})
    train_rows = [row for row in rows if row.get("app") not in holdout_apps]
    val_rows = [row for row in rows if row.get("app") in holdout_apps]
    if not train_rows or not val_rows:
        raise SystemExit(
            f"app split produced {len(train_rows)} train / {len(val_rows)} val rows; "
            f"apps in dataset: {seen_apps}"
        )

    x_train, y_train = to_arrays(train_rows, feature_dim)
    x_val, y_val = to_arrays(val_rows, feature_dim)

    print(f"apps: {seen_apps}")
    print(f"holdout apps: {sorted(holdout_apps)}")
    print(
        f"rows: {len(train_rows)} train / {len(val_rows)} val, features: {feature_dim}, "
        f"positive rate: train={y_train.mean():.3f} val={y_val.mean():.3f}"
    )

    w1, b1, w2, b2 = train_numpy(x_train, y_train, args.epochs, seed=args.seed)
    train_pred = predict(x_train, w1, b1, w2, b2)
    val_pred = predict(x_val, w1, b1, w2, b2)
    val_loss = float(
        -np.mean(
            y_val * np.log(np.clip(val_pred, 1e-9, 1.0))
            + (1 - y_val) * np.log(np.clip(1 - val_pred, 1e-9, 1.0))
        )
    )
    val_auc = auc(y_val, val_pred)
    baseline = np.full_like(y_val, y_train.mean())
    baseline_auc = auc(y_val, baseline)
    train_auc = auc(y_train, train_pred)

    per_app = {}
    for app in sorted({row.get("app", "unknown") for row in val_rows}):
        app_rows = [row for row in val_rows if row.get("app") == app]
        _x, y_app = to_arrays(app_rows, feature_dim)
        preds = np.array([float(predict(np.array([r["features"]]), w1, b1, w2, b2)[0]) for r in app_rows])
        per_app[app] = {
            "rows": len(app_rows),
            "positive_rate": float(y_app.mean()),
            "auc": float(auc(y_app, preds)) if not math.isnan(auc(y_app, preds)) else 0.0,
        }

    with open(args.dataset, "rb") as handle:
        dataset_digest = hashlib.sha256(handle.read()).hexdigest()

    artifact = {
        "format": "reforge-explorernet/1",
        "feature_dim": feature_dim,
        "hidden_dim": HIDDEN_DIM,
        "w1": w1.tolist(),
        "b1": b1.tolist(),
        "w2": w2.tolist(),
        "b2": b2,
        "metrics": {
            "val_auc": float(val_auc) if not math.isnan(val_auc) else 0.0,
            "val_loss": val_loss,
            "train_rows": int(len(train_rows)),
            "val_rows": int(len(val_rows)),
            "epochs": args.epochs,
            "baseline_auc": float(baseline_auc) if not math.isnan(baseline_auc) else 0.0,
            "train_apps": sorted({row.get("app") for row in train_rows}),
            "holdout_apps": sorted(holdout_apps),
            "per_app": per_app,
            "target": "label_value_5",
            "notes": (
                f"train AUC {train_auc:.3f}; split by application, not by row or run; "
                "target is discovery within the next five actions"
            ),
        },
        "dataset_digest": dataset_digest,
        "trained_by": "ml/train_explorernet.py (numpy, app-level split)",
        "version": "v0.2.0",
    }

    os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as handle:
        json.dump(artifact, handle, indent=2)
        handle.write("\n")

    print(
        f"val_loss={val_loss:.4f} val_auc={val_auc:.4f} "
        f"baseline_auc={baseline_auc:.4f} train_auc={train_auc:.4f}"
    )
    for app, metrics in per_app.items():
        print(f"  {app}: rows={metrics['rows']} pos={metrics['positive_rate']:.3f} auc={metrics['auc']:.3f}")
    print(f"wrote {args.out}")


if __name__ == "__main__":
    main()
