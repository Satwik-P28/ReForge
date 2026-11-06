#!/usr/bin/env python3
"""Small account ledger with balance transitions and a collection projection."""

import json
import os
import sys

COMMANDS = ["open", "deposit", "withdraw", "close", "reopen", "list", "stats"]


def path(directory):
    return os.path.join(directory, "state.json")


def load(directory):
    if not os.path.exists(path(directory)):
        return {
            "view": "list",
            "accounts": {},
            "next_id": 1,
            "selected": None,
            "rejects": 0,
            "deposits": 0,
            "withdrawals": 0,
        }
    with open(path(directory), "r", encoding="utf-8") as handle:
        return json.load(handle)


def save(directory, state):
    with open(path(directory), "w", encoding="utf-8") as handle:
        json.dump(state, handle, sort_keys=True, indent=2)


def envelope(state, outcome="success", error=None, events=None, extra=None):
    accounts = state["accounts"]
    payload = {
        "outcome": outcome,
        "view": state["view"],
        "selected": state.get("selected"),
        "events": events or [],
        "accounts": [
            {
                "id": account_id,
                "owner": account["owner"],
                "balance": account["balance"],
                "status": account["status"],
            }
            for account_id, account in sorted(accounts.items())
        ],
        "account_count": len(accounts),
        "open_count": sum(1 for a in accounts.values() if a["status"] == "open"),
        "closed_count": sum(1 for a in accounts.values() if a["status"] == "closed"),
        "total_balance": sum(a["balance"] for a in accounts.values()),
        "rejects": state["rejects"],
        "deposits": state["deposits"],
        "withdrawals": state["withdrawals"],
    }
    if error is not None:
        payload["error"] = error
    if extra:
        payload.update(extra)
    return payload


def emit(directory, state, outcome="success", error=None, events=None, extra=None):
    if outcome != "success":
        state["rejects"] += 1
    save(directory, state)
    print(json.dumps(envelope(state, outcome, error, events, extra), sort_keys=True))
    return 0


def main(argv):
    if len(argv) < 3 or argv[1] != "--state":
        print(json.dumps({"outcome": "error", "error": "bad_usage"}))
        return 2
    directory = argv[2]
    os.makedirs(directory, exist_ok=True)
    args = argv[3:]
    if not args:
        print(json.dumps(envelope(load(directory))))
        return 0
    command, rest = args[0], args[1:]
    state = load(directory)
    accounts = state["accounts"]

    if command == "open":
        if not rest or not rest[0].strip():
            return emit(directory, state, "rejected", "owner_required")
        account_id = "a%d" % state["next_id"]
        state["next_id"] += 1
        accounts[account_id] = {"owner": rest[0], "balance": 0, "status": "open"}
        state["view"] = "list"
        state["selected"] = None
        return emit(
            directory,
            state,
            "success",
            None,
            ["account.opened"],
            {"created_id": account_id, "created_pool": "account"},
        )

    if command == "list":
        state["view"] = "list"
        state["selected"] = None
        return emit(directory, state)

    if command == "stats":
        return emit(directory, state)

    if command not in ("deposit", "withdraw", "close", "reopen"):
        return emit(directory, state, "rejected", "unknown_command")

    if not rest:
        return emit(directory, state, "rejected", "account_required")
    account_id = rest[0]
    if account_id not in accounts:
        return emit(directory, state, "rejected", "no_such_account")
    account = accounts[account_id]
    state["selected"] = account_id

    if command == "deposit":
        if account["status"] != "open":
            return emit(directory, state, "rejected", "account_closed")
        account["balance"] += 1
        state["deposits"] += 1
        state["view"] = "detail"
        return emit(directory, state, "success", None, ["account.deposited"])

    if command == "withdraw":
        if account["status"] != "open":
            return emit(directory, state, "rejected", "account_closed")
        if account["balance"] == 0:
            return emit(directory, state, "rejected", "insufficient_funds")
        account["balance"] -= 1
        state["withdrawals"] += 1
        state["view"] = "detail"
        return emit(directory, state, "success", None, ["account.withdrawn"])

    if command == "close":
        if account["balance"] != 0:
            return emit(directory, state, "rejected", "balance_not_zero")
        if account["status"] == "closed":
            return emit(directory, state, "rejected", "already_closed")
        account["status"] = "closed"
        state["view"] = "detail"
        return emit(directory, state, "success", None, ["account.closed"])

    if account["status"] != "closed":
        return emit(directory, state, "rejected", "not_closed")
    account["status"] = "open"
    state["view"] = "detail"
    return emit(directory, state, "success", None, ["account.reopened"])


if __name__ == "__main__":
    sys.exit(main(sys.argv))
