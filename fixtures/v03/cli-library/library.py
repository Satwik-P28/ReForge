#!/usr/bin/env python3
"""Independent library fixture with loan, archive, and purge prerequisites."""

import json
import os
import sys

COMMANDS = ["add-book", "detail", "borrow", "return", "archive", "purge",
            "list", "stats", "audit"]


def path(directory):
    return os.path.join(directory, "state.json")


def load(directory):
    if not os.path.exists(path(directory)):
        return {"view": "catalog", "books": {}, "next_id": 1, "selected": None,
                "on_loan_total": 0, "archived_count": 0}
    with open(path(directory), "r", encoding="utf-8") as handle:
        return json.load(handle)


def save(directory, state):
    books = state["books"]
    state["on_loan_total"] = sum(book["on_loan"] for book in books.values())
    state["archived_count"] = sum(1 for book in books.values() if book["archived"])
    with open(path(directory), "w", encoding="utf-8") as handle:
        json.dump(state, handle, sort_keys=True, indent=2)


def available(state):
    actions = ["add-book", "list", "stats", "audit"]
    if state["books"]:
        actions += ["detail", "borrow", "return", "archive"]
    if state["archived_count"] > 0:
        actions.append("purge")
    return actions


def envelope(state, outcome="success", error=None, events=None, extra=None):
    books = state["books"]
    payload = {
        "outcome": outcome,
        "view": state["view"],
        "selected": state.get("selected"),
        "available_actions": available(state),
        "events": events or [],
        "book_count": len(books),
        "on_loan_total": state["on_loan_total"],
        "archived_count": state["archived_count"],
        "books": [
            {"id": bid, "status": "archived" if book["archived"] else "catalog",
             "copies": book["copies"], "on_loan": book["on_loan"]}
            for bid, book in sorted(books.items())
        ],
    }
    if error is not None:
        payload["error"] = error
    if extra:
        payload.update(extra)
    return payload


def emit(directory, state, outcome="success", error=None, events=None, extra=None):
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
    books = state["books"]

    if command == "add-book":
        if len(rest) < 2:
            return emit(directory, state, "rejected", "title_and_copies_required")
        title, raw_copies = rest[0], rest[1]
        try:
            copies = int(raw_copies)
        except ValueError:
            return emit(directory, state, "rejected", "copies_not_integer")
        if copies <= 0:
            return emit(directory, state, "rejected", "copies_not_positive")
        bid = "b%d" % state["next_id"]
        state["next_id"] += 1
        books[bid] = {"title": title, "copies": copies, "on_loan": 0, "archived": False}
        state["view"] = "catalog"
        state["selected"] = None
        return emit(directory, state, "success", None, ["book.added"],
                    {"created_id": bid, "created_pool": "book"})

    if command in ("list", "stats", "audit"):
        if command == "list":
            state["view"] = "catalog"
            state["selected"] = None
        return emit(directory, state)

    if command == "detail":
        if not rest:
            return emit(directory, state, "rejected", "book_required")
        if rest[0] not in books:
            return emit(directory, state, "rejected", "no_such_book")
        state["view"] = "detail"
        state["selected"] = rest[0]
        return emit(directory, state)

    if command in ("borrow", "return", "archive"):
        if not rest:
            return emit(directory, state, "rejected", "book_required")
        bid = rest[0]
        if bid not in books:
            return emit(directory, state, "rejected", "no_such_book")
        book = books[bid]
        state["selected"] = bid
        if command == "borrow":
            if book["archived"]:
                return emit(directory, state, "rejected", "archived_book")
            if book["on_loan"] >= book["copies"]:
                return emit(directory, state, "rejected", "no_copies")
            book["on_loan"] += 1
            state["view"] = "detail"
            return emit(directory, state, "success", None, ["book.borrowed"])
        if command == "return":
            if book["on_loan"] == 0:
                return emit(directory, state, "rejected", "not_borrowed")
            book["on_loan"] -= 1
            state["view"] = "detail"
            return emit(directory, state, "success", None, ["book.returned"])
        if book["on_loan"] > 0:
            return emit(directory, state, "rejected", "copies_on_loan")
        book["archived"] = True
        state["view"] = "archive"
        state["selected"] = None
        return emit(directory, state, "success", None, ["book.archived"])

    if command == "purge":
        if state["view"] != "archive":
            return emit(directory, state, "rejected", "wrong_view")
        if state["archived_count"] == 0:
            return emit(directory, state, "rejected", "nothing_archived")
        state["books"] = {bid: book for bid, book in books.items() if not book["archived"]}
        state["view"] = "catalog"
        state["selected"] = None
        return emit(directory, state, "success", None, ["archive.purged"])

    return emit(directory, state, "rejected", "unknown_command")


if __name__ == "__main__":
    sys.exit(main(sys.argv))
