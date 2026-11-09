#!/usr/bin/env python3
"""Black-box adapter for hledger 1.32.3.

The investigator sees the declared action schema, not this source. The adapter
uses hledger's documented `add` command for mutations and its documented JSON
`print` output for the observable ledger state. Transaction dates are omitted
from the state document because they are volatile and irrelevant to the exposed
accounting subset; descriptions, accounts and amounts are retained.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
HLEDGER = ROOT / "target" / "phase64-tools" / "hledger" / "hledger"
JOURNAL = Path("ledger.journal")
STATE = Path("state.json")

ACCOUNTS = {
    "record_food": ("Expenses:Food", 12.50),
    "record_rent": ("Expenses:Rent", 25.00),
}
FIXED_DATE = "2026-01-02"


def run(args: list[str], stdin: str | None = None) -> subprocess.CompletedProcess[str]:
    env = {
        "PATH": os.environ.get("PATH", ""),
        "HOME": str(Path.cwd() / ".home"),
        "LANG": "C",
        "TZ": "UTC",
    }
    return subprocess.run(
        [str(HLEDGER), "-f", str(JOURNAL), *args],
        input=stdin,
        text=True,
        capture_output=True,
        env=env,
        timeout=20,
    )


def amount_of(posting: dict) -> float:
    total = 0.0
    for component in posting.get("pamount") or []:
        quantity = component.get("aquantity") or {}
        value = quantity.get("floatingPoint")
        if value is None:
            mantissa = quantity.get("decimalMantissa", 0)
            places = quantity.get("decimalPlaces", 0)
            value = float(mantissa) / (10 ** int(places) if places else 1)
        total += float(value)
    return round(total, 2)


def snapshot() -> dict:
    result = run(["print", "-O", "json"])
    if result.returncode != 0:
        raise RuntimeError((result.stderr or result.stdout).strip() or "hledger print failed")
    raw = json.loads(result.stdout or "[]")
    transactions = []
    balances: dict[str, float] = {}
    expense_total = 0.0
    income_total = 0.0
    for index, item in enumerate(raw):
        postings = []
        for posting in item.get("tpostings") or []:
            account = posting.get("paccount", "")
            amount = amount_of(posting)
            postings.append({"account": account, "amount": amount})
            balances[account] = round(balances.get(account, 0.0) + amount, 2)
            if account.startswith("Expenses:") and amount > 0:
                expense_total = round(expense_total + amount, 2)
            if account.startswith("Income:") and amount < 0:
                income_total = round(income_total - amount, 2)
        transactions.append(
            {
                "id": int(item.get("tindex", index + 1)),
                "description": item.get("tdescription", ""),
                "postings": postings,
            }
        )
    return {
        "transactions": transactions,
        "balances": {key: balances[key] for key in sorted(balances)},
        "transaction_count": len(transactions),
        "expense_total": expense_total,
        "income_total": income_total,
    }


def save(state: dict) -> None:
    STATE.write_text(json.dumps(state, sort_keys=True, separators=(",", ":")), encoding="utf-8")


def emit(outcome: str, error: str | None = None, **extra: object) -> int:
    state = snapshot()
    save(state)
    envelope: dict[str, object] = {"outcome": outcome}
    if error:
        envelope["error"] = error
    envelope.update(extra)
    print(json.dumps(envelope, sort_keys=True, separators=(",", ":")))
    return 0 if outcome == "success" else 0


def record(action: str, description: str) -> int:
    if not description.strip():
        return emit("rejected", "description_required")
    account, amount = ACCOUNTS[action]
    posting = f"{account}\n{amount:.2f}\nAssets:Cash\n\n\ny\n"
    before = snapshot()
    result = run(
        ["add", "--no-new-accounts"],
        stdin=f"{FIXED_DATE}\n{description}\n{posting}",
    )
    after = snapshot()
    # hledger 1.32.3 can save the transaction and still end the interactive
    # prompt without printing "Saved." in non-TTY mode. Success is the observed
    # new persisted transaction, not a particular progress string.
    saved = any(
        transaction.get("description") == description
        for transaction in after.get("transactions", [])
    ) and len(after.get("transactions", [])) > len(before.get("transactions", []))
    if result.returncode != 0 or not saved:
        detail = (result.stderr or result.stdout).strip().splitlines()[-1:]
        return emit("rejected", "hledger_add_failed", detail=" ".join(detail))
    return emit("success")


def record_income(description: str) -> int:
    if not description.strip():
        return emit("rejected", "description_required")
    posting = "Assets:Cash\n40.00\nIncome:Salary\n\n\ny\n"
    before = snapshot()
    result = run(
        ["add", "--no-new-accounts"],
        stdin=f"{FIXED_DATE}\n{description}\n{posting}",
    )
    after = snapshot()
    saved = any(
        transaction.get("description") == description
        for transaction in after.get("transactions", [])
    ) and len(after.get("transactions", [])) > len(before.get("transactions", []))
    if result.returncode != 0 or not saved:
        detail = (result.stderr or result.stdout).strip().splitlines()[-1:]
        return emit("rejected", "hledger_add_failed", detail=" ".join(detail))
    return emit("success")


def main(argv: list[str]) -> int:
    if len(argv) < 3 or argv[1] != "--state":
        print(json.dumps({"outcome": "error", "error": "bad_usage"}, sort_keys=True))
        return 2
    workdir = Path(argv[2])
    os.chdir(workdir)
    args = argv[3:]
    if not args:
        save(snapshot())
        print(json.dumps({"outcome": "observed"}, sort_keys=True))
        return 0
    action, rest = args[0], args[1:]
    if action in ACCOUNTS:
        if not rest:
            return emit("rejected", "description_required")
        return record(action, rest[0])
    if action == "record_income":
        if not rest:
            return emit("rejected", "description_required")
        return record_income(rest[0])
    if action == "balance":
        state = snapshot()
        save(state)
        print(json.dumps({"outcome": "success", "expense_total": state["expense_total"]}, sort_keys=True))
        return 0
    if action == "list":
        save(snapshot())
        print(json.dumps({"outcome": "success"}, sort_keys=True))
        return 0
    return emit("rejected", "unknown_action")


if __name__ == "__main__":
    sys.exit(main(sys.argv))
