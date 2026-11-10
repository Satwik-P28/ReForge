#!/usr/bin/env python3
"""Thin HTTP adapter for the unmodified json-server 0.17.4 CLI.

The target is a real JSON REST service. This adapter starts one isolated server
for the requested action, performs the HTTP request, stops the server, and emits
one normalized observation envelope. It contains no reconstructed business
logic; all mutations and identity allocation are performed by json-server.
"""
from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

SPEC = Path(__file__).resolve().parent
CLI = SPEC / "runtime/node_modules/json-server/lib/cli/bin.js"
DB = Path("db.json")
STATE = Path("state.json")
NODE = "node"


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def start_server(log):
    port = free_port()
    proc = subprocess.Popen(
        [
            NODE,
            str(CLI),
            str(DB),
            "--host",
            "127.0.0.1",
            "--port",
            str(port),
            "--no-cors",
            "--quiet",
        ],
        cwd=os.getcwd(),
        stdout=log,
        stderr=log,
        env={**os.environ, "NO_UPDATE_NOTIFIER": "1"},
    )
    deadline = time.time() + 20
    while time.time() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"json-server exited with {proc.returncode}")
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/posts", timeout=0.5):
                return proc, port
        except Exception:
            time.sleep(0.05)
    proc.terminate()
    raise RuntimeError("json-server did not become ready")


def request(port: int, method: str, path: str, payload=None):
    data = None
    headers = {}
    if payload is not None:
        data = json.dumps(payload).encode("utf-8")
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}{path}", data=data, headers=headers, method=method
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as response:
            text = response.read().decode("utf-8")
            return response.status, json.loads(text) if text else None
    except urllib.error.HTTPError as exc:
        text = exc.read().decode("utf-8")
        try:
            body = json.loads(text) if text else None
        except json.JSONDecodeError:
            body = None
        return exc.code, body


def load_db() -> dict:
    if not DB.exists():
        return {"posts": [], "comments": []}
    raw = json.loads(DB.read_text(encoding="utf-8"))
    return {
        "posts": sorted(raw.get("posts", []), key=lambda item: item.get("id", 0)),
        "comments": sorted(raw.get("comments", []), key=lambda item: item.get("id", 0)),
    }


def wait_for(predicate, description: str) -> None:
    """Wait for the server's asynchronous file write to become observable.

    json-server applies a mutation in memory before its persistence callback
    rewrites db.json. Reading the file immediately after the HTTP response can
    therefore capture the pre-mutation document. The state observation is only
    valid once the requested mutation is visible in the persisted document.
    """
    deadline = time.time() + 5
    while time.time() < deadline:
        if predicate(load_db()):
            return
        time.sleep(0.02)
    raise RuntimeError(f"persisted state did not reflect {description}")


def save_state() -> dict:
    db = load_db()
    state = {
        "posts": db["posts"],
        "comments": db["comments"],
        "post_count": len(db["posts"]),
        "comment_count": len(db["comments"]),
    }
    STATE.write_text(json.dumps(state, sort_keys=True, separators=(",", ":")), encoding="utf-8")
    return state


def emit(outcome: str, **extra) -> None:
    save_state()
    print(json.dumps({"outcome": outcome, **extra}, sort_keys=True))


def post_by_id(posts: list, ident):
    try:
        wanted = int(ident)
    except (TypeError, ValueError):
        return None
    return next((post for post in posts if post.get("id") == wanted), None)


def main(argv: list[str]) -> int:
    if not argv or not CLI.exists():
        emit("error", error="target_unavailable")
        return 0
    action, args = argv[0], argv[1:]
    with open(".json-server.log", "a", encoding="utf-8") as log:
        proc = None
        try:
            proc, port = start_server(log)
            status = 0
            body = None
            if action == "create_post":
                status, body = request(
                    port,
                    "POST",
                    "/posts",
                    {
                        "title": args[0] if args else "",
                        "views": 0,
                        "published": False,
                        "authorId": 1,
                    },
                )
                if status in (200, 201):
                    wait_for(
                        lambda db: any(post.get("id") == body["id"] for post in db["posts"]),
                        "created post",
                    )
                    emit(
                        "success",
                        created_id=body["id"],
                        created_pool="post",
                        title=body.get("title"),
                        published=body.get("published"),
                        events=["post_created"],
                    )
                    return 0
            elif action == "read_post":
                status, body = request(port, "GET", f"/posts/{args[0] if args else ''}")
                if status == 200:
                    emit(
                        "success",
                        id=body["id"],
                        title=body.get("title"),
                        published=body.get("published"),
                        views=body.get("views"),
                    )
                    return 0
            elif action == "publish_post":
                ident = args[0] if args else ""
                status, existing = request(port, "GET", f"/posts/{ident}")
                if status == 200:
                    status, body = request(port, "PATCH", f"/posts/{ident}", {"published": True})
                    if status == 200:
                        wait_for(
                            lambda db: any(
                                post.get("id") == body["id"] and post.get("published") is True
                                for post in db["posts"]
                            ),
                            "published post",
                        )
                        emit("success", id=body["id"], published=body.get("published"), events=["post_published"])
                        return 0
            elif action == "delete_post":
                ident = args[0] if args else ""
                status, body = request(port, "DELETE", f"/posts/{ident}")
                if status == 200:
                    wait_for(
                        lambda db: not any(post.get("id") == int(ident) for post in db["posts"]),
                        "deleted post",
                    )
                    emit("success", deleted=True, deleted_id=int(ident), deleted_pool="post", events=["post_deleted"])
                    return 0
            elif action == "add_comment":
                post_id = args[0] if args else ""
                status, existing = request(port, "GET", f"/posts/{post_id}")
                if status == 200:
                    status, body = request(
                        port,
                        "POST",
                        "/comments",
                        {"postId": int(post_id), "body": args[1] if len(args) > 1 else ""},
                    )
                    if status == 201:
                        wait_for(
                            lambda db: any(comment.get("id") == body["id"] for comment in db["comments"]),
                            "created comment",
                        )
                        emit(
                            "success",
                            created_id=body["id"],
                            created_pool="comment",
                            postId=body.get("postId"),
                            body=body.get("body"),
                            events=["comment_created"],
                        )
                        return 0
            elif action == "list_comments":
                post_id = args[0] if args else ""
                status, _ = request(port, "GET", f"/posts/{post_id}")
                if status == 200:
                    status, body = request(port, "GET", f"/comments?postId={post_id}")
                    if status == 200:
                        emit("success", comments=body, comment_count=len(body))
                        return 0
            elif action == "post_count":
                status_posts, posts = request(port, "GET", "/posts")
                status_comments, comments = request(port, "GET", "/comments")
                if status_posts == 200 and status_comments == 200:
                    emit(
                        "success",
                        post_count=len(posts),
                        comment_count=len(comments),
                    )
                    return 0
            else:
                emit("error", error="unknown_action")
                return 0
        except Exception as exc:
            emit("error", error="target_failed", detail=str(exc)[:300])
            return 0
        finally:
            if proc is not None:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait(timeout=5)
        if status in (404, 410):
            emit("error", error="not_found", id=args[0] if args else None)
        else:
            emit("error", error="http_error", status=status, detail=body)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
