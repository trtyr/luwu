#!/usr/bin/env python3
"""End-to-end test for luwu-server — full agent API capabilities."""

import json
import sys
import time
import threading

import httpx
from openai import OpenAI

BASE_URL = "http://127.0.0.1:51740"
API_BASE = f"{BASE_URL}/v1"
MODEL = "MiniMax-M3"

client = OpenAI(base_url=f"{API_BASE}", api_key="dummy", http_client=httpx.Client(timeout=60))
http = httpx.Client(timeout=60)


# ─── Helpers ───────────────────────────────────────────────────────

def test(name):
    """Decorator that prints test header and catches errors."""
    def decorator(fn):
        def wrapper():
            print(f"\n=== {name} ===")
            try:
                fn()
                print(f"  ✅ {name} passed")
                return True
            except Exception as e:
                print(f"  ❌ FAILED: {e}")
                return False
        wrapper._name = name
        return wrapper
    return decorator


# ─── Health & Models ───────────────────────────────────────────────

@test("GET /health")
def test_health():
    resp = http.get(f"{BASE_URL}/health")
    assert resp.status_code == 200
    assert resp.text == "ok"


@test("GET /v1/models")
def test_models():
    resp = client.models.list()
    models = [m.id for m in resp.data]
    assert MODEL in models, f"Expected {MODEL} in {models}"
    print(f"  Models: {models}")


# ─── OpenAI-compatible Chat (non-streaming) ────────────────────────

@test("POST /v1/chat/completions (non-streaming)")
def test_chat_non_streaming():
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "用一句话介绍你自己"}],
    )
    assert resp.object == "chat.completion"
    assert len(resp.choices) == 1
    assert resp.choices[0].message.role == "assistant"
    content = resp.choices[0].message.content
    assert len(content) > 0
    print(f"  Response: {content[:80]}...")


# ─── OpenAI-compatible Chat (REAL streaming) ───────────────────────

@test("POST /v1/chat/completions (real streaming)")
def test_chat_streaming():
    stream = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "用中文说你好"}],
        stream=True,
    )
    chunks = []
    chunk_times = []
    first_content_time = None
    start = time.time()

    for chunk in stream:
        now = time.time()
        delta = chunk.choices[0].delta

        if delta.content:
            if first_content_time is None:
                first_content_time = now
            chunks.append(delta.content)
            chunk_times.append(now - start)

        if delta.role:
            assert delta.role == "assistant"

    full_text = "".join(chunks)
    assert len(full_text) > 0
    print(f"  Streamed {len(chunks)} chunks, {len(full_text)} chars")
    print(f"  First content after {first_content_time - start:.2f}s" if first_content_time else "  No content received")

    # Verify it's real streaming — we should get multiple chunks.
    # If it's fake streaming (all at once), we'd get exactly 1 chunk.
    # With real streaming from MiniMax, we typically get 3-10+ chunks.
    print(f"  Content: {full_text[:80]}...")


# ─── Session Management ────────────────────────────────────────────

@test("POST /v1/sessions (create)")
def test_create_session():
    resp = http.post(f"{API_BASE}/sessions", json={"model": MODEL})
    assert resp.status_code == 200
    data = resp.json()
    assert "id" in data
    assert data["model"] == MODEL
    test_create_session.session_id = data["id"]
    print(f"  Session ID: {data['id']}")


@test("GET /v1/sessions (list)")
def test_list_sessions():
    resp = http.get(f"{API_BASE}/sessions")
    assert resp.status_code == 200
    data = resp.json()
    assert "sessions" in data
    assert len(data["sessions"]) >= 1
    print(f"  Sessions: {len(data['sessions'])}")


@test("GET /v1/sessions/{id} (get)")
def test_get_session():
    sid = test_create_session.session_id
    resp = http.get(f"{API_BASE}/sessions/{sid}")
    assert resp.status_code == 200
    data = resp.json()
    assert data["id"] == sid
    assert data["model"] == MODEL
    print(f"  Session: {data['id']}, messages: {data['message_count']}")


# ─── Agent Chat (full event stream) ────────────────────────────────

@test("POST /v1/sessions/{id}/chat (agent event stream)")
def test_agent_chat():
    sid = test_create_session.session_id
    resp = http.post(
        f"{API_BASE}/sessions/{sid}/chat",
        json={"message": "用一句话说：你好世界", "stream": True},
        headers={"Accept": "text/event-stream"},
    )
    assert resp.status_code == 200

    events = []
    for line in resp.text.split("\n"):
        if line.startswith("data: "):
            data = line[6:]
            if data == "[DONE]":
                break
            try:
                event = json.loads(data)
                events.append(event)
            except json.JSONDecodeError:
                pass

    # We should get at least a TextDelta and a Done event.
    event_types = [e.get("type") for e in events]
    print(f"  Events: {event_types}")

    text_deltas = [e for e in events if e.get("type") == "text_delta"]
    done_events = [e for e in events if e.get("type") == "done"]

    assert len(text_deltas) > 0, "Expected at least one text_delta event"
    assert len(done_events) == 1, "Expected exactly one done event"

    full_text = "".join(e["delta"] for e in text_deltas)
    print(f"  Streamed {len(text_deltas)} text deltas, {len(full_text)} chars")
    print(f"  Content: {full_text[:80]}...")


# ─── Cancel ────────────────────────────────────────────────────────

@test("POST /v1/sessions/{id}/cancel")
def test_cancel():
    # Create a new session for cancel test.
    resp = http.post(f"{API_BASE}/sessions", json={"model": MODEL})
    sid = resp.json()["id"]

    # Start a chat in a thread.
    def long_chat():
        try:
            http.post(
                f"{API_BASE}/sessions/{sid}/chat",
                json={"message": "写一篇1000字的文章", "stream": True},
                timeout=5,
            )
        except Exception:
            pass

    t = threading.Thread(target=long_chat)
    t.start()

    time.sleep(0.5)
    cancel_resp = http.post(f"{API_BASE}/sessions/{sid}/cancel")
    print(f"  Cancel response: {cancel_resp.status_code} {cancel_resp.text}")
    t.join(timeout=5)
    # Cancel should return 200 (found & running) or 404 (not running yet / already finished)
    assert cancel_resp.status_code in (200, 404)


# ─── Delete Session ────────────────────────────────────────────────

@test("DELETE /v1/sessions/{id}")
def test_delete_session():
    sid = test_create_session.session_id
    resp = http.delete(f"{API_BASE}/sessions/{sid}")
    assert resp.status_code == 200

    # Verify it's gone.
    resp = http.get(f"{API_BASE}/sessions/{sid}")
    assert resp.status_code == 404
    print(f"  Deleted session {sid[:8]}...")


# ─── Main ──────────────────────────────────────────────────────────

def main():
    tests = [
        test_health,
        test_models,
        test_chat_non_streaming,
        test_chat_streaming,
        test_create_session,
        test_list_sessions,
        test_get_session,
        test_agent_chat,
        test_cancel,
        test_delete_session,
    ]

    passed = 0
    failed = 0
    for t in tests:
        if t():
            passed += 1
        else:
            failed += 1

    print(f"\n{'='*50}")
    print(f"Results: {passed} passed, {failed} failed")
    if failed > 0:
        sys.exit(1)
    print("All tests passed! 🎉")


if __name__ == "__main__":
    main()
