#!/usr/bin/env python3
"""Repair atelier with map entities, nested shop state and conditional errors."""

import json
import os
import sys


def state_path(directory):
    return os.path.join(directory, "state.json")


def initial():
    return {
        "jobs": {},
        "next_id": 301,
        "accepted": 0,
        "completed": 0,
        "rejects": 0,
        "active_tech": None,
        "selected": None,
        "view": "queue",
        "shop": {"name": "north", "capacity": 3, "open": True},
    }


def load(directory):
    if not os.path.exists(state_path(directory)):
        return initial()
    with open(state_path(directory), "r", encoding="utf-8") as handle:
        return json.load(handle)


def save(directory, state):
    with open(state_path(directory), "w", encoding="utf-8") as handle:
        json.dump(state, handle, sort_keys=True, indent=2)


def find(state, job_id):
    return state["jobs"].get(job_id)


def envelope(state, outcome="success", error=None, events=None, extra=None):
    jobs = state["jobs"]
    records = []
    for job_id in sorted(jobs):
        job = jobs[job_id]
        records.append(
            {
                "id": job_id,
                "customer": job["customer"],
                "status": job["status"],
                "tech": job["tech"],
                "estimate": job["estimate"],
                "parts": job["parts"],
            }
        )
    payload = {
        "outcome": outcome,
        "view": state["view"],
        "selected": state["selected"],
        "active_tech": state["active_tech"],
        "shop": state["shop"],
        "events": events or [],
        "jobs": records,
        "job_count": len(records),
        "new_count": sum(1 for job in jobs.values() if job["status"] == "new"),
        "quoted_count": sum(1 for job in jobs.values() if job["status"] == "quoted"),
        "active_count": sum(1 for job in jobs.values() if job["status"] == "active"),
        "closed_count": sum(1 for job in jobs.values() if job["status"] == "closed"),
        "estimate_total": sum(job["estimate"] for job in jobs.values()),
        "parts_total": sum(len(job["parts"]) for job in jobs.values()),
        "accepted": state["accepted"],
        "completed": state["completed"],
        "rejects": state["rejects"],
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
    state = load(directory)
    if not args:
        print(json.dumps(envelope(state), sort_keys=True))
        return 0
    command, rest = args[0], args[1:]

    if command == "intake":
        if not rest or not rest[0].strip():
            return emit(directory, state, "rejected", "customer_required")
        job_id = "j%d" % state["next_id"]
        state["next_id"] += 1
        state["jobs"][job_id] = {
            "customer": rest[0],
            "status": "new",
            "tech": None,
            "estimate": 0,
            "parts": [],
        }
        state["accepted"] += 1
        state["selected"] = None
        state["view"] = "queue"
        return emit(
            directory,
            state,
            "success",
            None,
            ["job.intake"],
            {"created_id": job_id, "created_pool": "job"},
        )

    if command == "quote":
        if not rest:
            return emit(directory, state, "rejected", "job_required")
        job = find(state, rest[0])
        if job is None:
            return emit(directory, state, "rejected", "unknown_job")
        state["selected"] = rest[0]
        if job["status"] != "new":
            return emit(directory, state, "rejected", "already_quoted")
        job["status"] = "quoted"
        state["view"] = "detail"
        return emit(directory, state, "success", None, ["job.quoted"])

    if command == "assign":
        if not rest:
            return emit(directory, state, "rejected", "job_required")
        job = find(state, rest[0])
        if job is None:
            return emit(directory, state, "rejected", "unknown_job")
        state["selected"] = rest[0]
        if job["status"] != "quoted":
            return emit(directory, state, "rejected", "not_quoted")
        tech = rest[1] if len(rest) > 1 else "tech"
        job["status"] = "active"
        job["tech"] = tech
        state["active_tech"] = tech
        state["view"] = "detail"
        return emit(directory, state, "success", None, ["job.assigned"])

    if command == "add-part":
        if not rest:
            return emit(directory, state, "rejected", "job_required")
        job = find(state, rest[0])
        if job is None:
            return emit(directory, state, "rejected", "unknown_job")
        state["selected"] = rest[0]
        if job["status"] == "closed":
            return emit(directory, state, "rejected", "closed_job")
        part = rest[1] if len(rest) > 1 else "part"
        job["parts"].append(part)
        job["estimate"] += 25
        state["view"] = "detail"
        return emit(directory, state, "success", None, ["part.added"])

    if command == "finish":
        if not rest:
            return emit(directory, state, "rejected", "job_required")
        job = find(state, rest[0])
        if job is None:
            return emit(directory, state, "rejected", "unknown_job")
        state["selected"] = rest[0]
        if job["status"] != "active":
            return emit(directory, state, "rejected", "not_active")
        job["status"] = "closed"
        state["active_tech"] = None
        state["completed"] += 1
        state["view"] = "detail"
        return emit(directory, state, "success", None, ["job.finished"])

    if command == "drop":
        if not rest:
            return emit(directory, state, "rejected", "job_required")
        job = find(state, rest[0])
        if job is None:
            return emit(directory, state, "rejected", "unknown_job")
        state["selected"] = rest[0]
        if job["status"] != "new":
            return emit(directory, state, "rejected", "cannot_drop")
        del state["jobs"][rest[0]]
        state["accepted"] -= 1
        state["selected"] = None
        state["view"] = "queue"
        return emit(directory, state, "success", None, ["job.dropped"])

    if command == "status":
        return emit(directory, state)

    return emit(directory, state, "rejected", "unknown_command")


if __name__ == "__main__":
    sys.exit(main(sys.argv))
