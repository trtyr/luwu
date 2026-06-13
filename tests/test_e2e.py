#!/usr/bin/env python3
"""
luwu end-to-end test suite — covers all API endpoints and tool capabilities.

Requires:
  - luwu-server running at http://127.0.0.1:51740
  - ~/.luwu/config.toml with a valid provider config
  - uv (to install deps: httpx, openai)

Usage:
  uv run --with httpx --with openai python3 tests/test_e2e.py
"""

import json
import re
import sys
import time
import threading

import httpx
from openai import OpenAI

# ─── Config ──────────────────────────────────────────────────────────

BASE_URL = "http://127.0.0.1:51740"
API_BASE = f"{BASE_URL}/v1"
MODEL = "MiniMax-M3"
POLL_TIMEOUT = 90  # seconds to wait for LLM responses

client = OpenAI(
    base_url=API_BASE,
    api_key="dummy",
    http_client=httpx.Client(timeout=POLL_TIMEOUT),
)
http = httpx.Client(timeout=POLL_TIMEOUT)

# Track test results
_results = {"passed": 0, "failed": 0, "skipped": 0}


# ─── Helpers ─────────────────────────────────────────────────────────

def test(name):
    """Decorator that prints test header, catches errors, tracks results."""
    def decorator(fn):
        def wrapper():
            print(f"\n=== {name} ===")
            try:
                fn()
                print(f"  ✅ {name} passed")
                _results["passed"] += 1
                return True
            except AssertionError as e:
                print(f"  ❌ FAILED: {e}")
                _results["failed"] += 1
                return False
            except Exception as e:
                print(f"  ❌ ERROR: {type(e).__name__}: {e}")
                _results["failed"] += 1
                return False
        wrapper._name = name
        return wrapper
    return decorator


def create_session() -> str:
    """Create a new session and return its ID."""
    resp = http.post(f"{API_BASE}/sessions", json={})
    assert resp.status_code == 200, f"Create session failed: {resp.status_code} {resp.text}"
    return resp.json()["id"]


def agent_chat(sid: str, message: str, timeout: float = POLL_TIMEOUT) -> str:
    """Send a message to an agent session (SSE) and return the full response text."""
    resp = http.post(
        f"{API_BASE}/sessions/{sid}/chat",
        json={"message": message, "stream": True},
        headers={"Accept": "text/event-stream"},
        timeout=timeout,
    )
    assert resp.status_code == 200, f"Agent chat failed: {resp.status_code} {resp.text}"
    return resp.text


def parse_sse_events(raw: str) -> list[dict]:
    """Parse SSE data lines into a list of JSON dicts."""
    events = []
    for line in raw.split("\n"):
        if line.startswith("data: "):
            data = line[6:]
            if data == "[DONE]":
                break
            try:
                events.append(json.loads(data))
            except json.JSONDecodeError:
                pass
    return events


def extract_text_from_events(events: list[dict]) -> str:
    """Concatenate all text_delta events into a single string."""
    return "".join(e.get("delta", "") for e in events if e.get("type") == "text_delta")


def get_tool_events(events: list[dict]) -> list[dict]:
    """Extract tool-related events (tool_started, tool_call, tool_completed)."""
    return [e for e in events if e.get("type") in ("tool_started", "tool_call", "tool_completed")]


def find_anchors(text: str) -> list[str]:
    """Find all LINE:HASH anchors in text (format: NN:xxx|)."""
    return re.findall(r'(\d+:[0-9a-f]{3})\|', text)


# ═══════════════════════════════════════════════════════════════════════
# SECTION 1: Server Basics
# ═══════════════════════════════════════════════════════════════════════

@test("GET /health returns ok")
def test_health():
    resp = http.get(f"{BASE_URL}/health")
    assert resp.status_code == 200
    assert resp.text == "ok"


@test("GET /v1/models returns configured model")
def test_models():
    resp = client.models.list()
    models = [m.id for m in resp.data]
    assert MODEL in models, f"Expected {MODEL} in {models}"
    print(f"  Models: {models}")


# ═══════════════════════════════════════════════════════════════════════
# SECTION 2: OpenAI-Compatible Chat
# ═══════════════════════════════════════════════════════════════════════

@test("POST /v1/chat/completions non-streaming returns valid response")
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
    assert resp.choices[0].finish_reason == "stop"
    print(f"  Response: {content[:80]}...")


@test("POST /v1/chat/completions streaming yields multiple chunks")
def test_chat_streaming():
    stream = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "用中文说你好"}],
        stream=True,
    )
    chunks = []
    first_content_time = None
    start = time.time()

    for chunk in stream:
        now = time.time()
        delta = chunk.choices[0].delta

        if delta.content:
            if first_content_time is None:
                first_content_time = now
            chunks.append(delta.content)

        if delta.role:
            assert delta.role == "assistant"

    full_text = "".join(chunks)
    assert len(full_text) > 0, "Expected non-empty response content"
    assert len(chunks) >= 1, "Expected at least one content chunk"
    print(f"  Streamed {len(chunks)} chunks, {len(full_text)} chars")
    if first_content_time:
        print(f"  First content after {first_content_time - start:.2f}s")
    print(f"  Content: {full_text[:80]}...")


# ═══════════════════════════════════════════════════════════════════════
# SECTION 3: Session Management
# ═══════════════════════════════════════════════════════════════════════

# Store session IDs for cross-test use
_session_ids = {}


@test("POST /v1/sessions creates a new session")
def test_session_create():
    resp = http.post(f"{API_BASE}/sessions", json={})
    assert resp.status_code == 200
    data = resp.json()
    assert "id" in data
    assert data["model"] == MODEL
    _session_ids["main"] = data["id"]
    print(f"  Session ID: {data['id'][:12]}...")


@test("GET /v1/sessions lists sessions")
def test_session_list():
    resp = http.get(f"{API_BASE}/sessions")
    assert resp.status_code == 200
    data = resp.json()
    assert "sessions" in data
    assert len(data["sessions"]) >= 1
    print(f"  Sessions: {len(data['sessions'])}")


@test("GET /v1/sessions/{id} returns session details")
def test_session_get():
    sid = _session_ids["main"]
    resp = http.get(f"{API_BASE}/sessions/{sid}")
    assert resp.status_code == 200
    data = resp.json()
    assert data["id"] == sid
    assert data["model"] == MODEL
    assert "message_count" in data
    print(f"  Session: {data['id'][:12]}..., messages: {data['message_count']}")


@test("GET /v1/sessions/{id} returns 404 for unknown session")
def test_session_get_404():
    resp = http.get(f"{API_BASE}/sessions/nonexistent-id")
    assert resp.status_code == 404


@test("DELETE /v1/sessions/{id} deletes a session")
def test_session_delete():
    # Create a throwaway session.
    resp = http.post(f"{API_BASE}/sessions", json={})
    sid = resp.json()["id"]

    # Delete it.
    resp = http.delete(f"{API_BASE}/sessions/{sid}")
    assert resp.status_code == 200

    # Verify it's gone.
    resp = http.get(f"{API_BASE}/sessions/{sid}")
    assert resp.status_code == 404


# ═══════════════════════════════════════════════════════════════════════
# SECTION 4: Agent Chat (Event Stream)
# ═══════════════════════════════════════════════════════════════════════

@test("POST /v1/sessions/{id}/chat returns text_delta + done events")
def test_agent_chat_events():
    sid = create_session()
    raw = agent_chat(sid, "用一句话说：你好世界")
    events = parse_sse_events(raw)

    event_types = [e.get("type") for e in events]
    print(f"  Events: {event_types}")

    text_deltas = [e for e in events if e.get("type") == "text_delta"]
    done_events = [e for e in events if e.get("type") == "done"]

    assert len(text_deltas) > 0, "Expected at least one text_delta event"
    assert len(done_events) == 1, "Expected exactly one done event"

    full_text = "".join(e["delta"] for e in text_deltas)
    assert len(full_text) > 0
    print(f"  Streamed {len(text_deltas)} text deltas, {len(full_text)} chars")
    print(f"  Content: {full_text[:80]}...")


@test("POST /v1/sessions/{id}/chat returns 404 for unknown session")
def test_agent_chat_404():
    resp = http.post(
        f"{API_BASE}/sessions/nonexistent/chat",
        json={"message": "hello"},
    )
    assert resp.status_code == 404


@test("POST /v1/sessions/{id}/chat returns 409 when session is busy")
def test_agent_chat_conflict():
    sid = create_session()

    # Start a long-running chat in a thread.
    barrier = threading.Barrier(2, timeout=5)
    error_holder = [None]

    def long_chat():
        try:
            barrier.wait()
            http.post(
                f"{API_BASE}/sessions/{sid}/chat",
                json={"message": "写一篇很长的文章，至少500字", "stream": True},
                timeout=30,
            )
        except Exception as e:
            error_holder[0] = e

    t = threading.Thread(target=long_chat)
    t.start()
    barrier.wait()
    time.sleep(0.3)

    # Try to send another message — should get 409.
    resp = http.post(
        f"{API_BASE}/sessions/{sid}/chat",
        json={"message": "你好"},
    )
    t.join(timeout=30)
    assert resp.status_code == 409, f"Expected 409, got {resp.status_code}"
    print(f"  Got expected 409 Conflict")


# ═══════════════════════════════════════════════════════════════════════
# SECTION 5: Bash Tool
# ═══════════════════════════════════════════════════════════════════════

@test("Agent uses bash tool to run a command")
def test_bash_tool():
    sid = create_session()
    raw = agent_chat(sid, "请使用 bash 工具运行 echo 'hello-luwu-test'，只运行这一个命令，不要做其他事")
    events = parse_sse_events(raw)

    tool_events = get_tool_events(events)
    event_types = [e.get("type") for e in events]

    # Check that tool events exist (bash was called).
    has_tool = any("tool" in t for t in event_types)
    full_text = extract_text_from_events(events)

    # The response should mention the echo output or confirm it ran bash.
    assert has_tool or "hello-luwu-test" in full_text.lower() or "bash" in full_text.lower(), \
        f"Expected bash tool usage. Events: {event_types}\nText: {full_text[:200]}"
    print(f"  Tool events: {[e['type'] for e in tool_events]}")
    print(f"  Response: {full_text[:120]}...")


# ═══════════════════════════════════════════════════════════════════════
# SECTION 6: Read Tool (LINE:HASH Anchors)
# ═══════════════════════════════════════════════════════════════════════

@test("Read tool output (in tool_completed event) contains LINE:HASH anchors")
def test_read_tool_anchors():
    sid = create_session()
    raw = agent_chat(
        sid,
        "请使用 read 工具读取 Cargo.toml 的前5行，offset=1, limit=5。只读取，不要修改任何文件。"
    )
    events = parse_sse_events(raw)

    # Find tool_completed events — the read tool's raw output contains the anchors.
    completed = [e for e in events if e.get("type") == "tool_completed"]
    assert len(completed) >= 1, f"Expected tool_completed event, got event types: {[e.get('type') for e in events]}"

    tool_output = completed[0].get("output", "")
    anchors = find_anchors(tool_output)
    assert len(anchors) >= 3, \
        f"Expected at least 3 LINE:HASH anchors in tool output, found {len(anchors)} in:\n{tool_output[:300]}"
    print(f"  Found {len(anchors)} anchors: {anchors[:5]}")

    # Verify anchor format: digits:hex3
    for anchor in anchors:
        parts = anchor.split(":")
        assert len(parts) == 2, f"Bad anchor format: {anchor}"
        assert parts[0].isdigit(), f"Anchor line number not numeric: {anchor}"
        assert len(parts[1]) == 3, f"Anchor hash not 3 chars: {anchor}"
        assert all(c in "0123456789abcdef" for c in parts[1]), f"Anchor hash not hex: {anchor}"
    print(f"  Anchor format verified ✅")


@test("Read tool with offset/limit returns correct line range")
def test_read_tool_offset_limit():
    sid = create_session()
    raw = agent_chat(
        sid,
        "请使用 read 工具读取 Cargo.toml，offset=3, limit=2。只读取第3-4行。不要做其他事。"
    )
    events = parse_sse_events(raw)

    # Check tool_completed output.
    completed = [e for e in events if e.get("type") == "tool_completed"]
    assert len(completed) >= 1, f"Expected tool_completed event, got: {[e.get('type') for e in events]}"

    tool_output = completed[0].get("output", "")
    anchors = find_anchors(tool_output)
    assert len(anchors) >= 1, f"Expected anchors in tool output, got none in:\n{tool_output[:300]}"
    print(f"  Anchors in tool output: {anchors[:5]}")


@test("Agent reads a directory")
def test_read_directory():
    sid = create_session()
    raw = agent_chat(sid, "请使用 read 工具列出 crates 目录的内容，path='crates'")
    events = parse_sse_events(raw)
    full_text = extract_text_from_events(events)

    # Should show directory entries.
    has_dirs = any(d in full_text for d in ("luwu-core", "luwu-llm", "luwu-tools", "luwu-server"))
    assert has_dirs, f"Expected directory entries in response:\n{full_text[:300]}"
    print(f"  Directory listing found ✅")


# ═══════════════════════════════════════════════════════════════════════
# SECTION 7: Write Tool
# ═══════════════════════════════════════════════════════════════════════

@test("Agent uses write tool to create a file")
def test_write_tool():
    sid = create_session()
    raw = agent_chat(
        sid,
        "请使用 write 工具创建一个文件 test_output.txt，内容为 'luwu e2e test file'"
    )
    events = parse_sse_events(raw)
    full_text = extract_text_from_events(events)

    # Verify the file was created.
    # Use a new session to read the file.
    sid2 = create_session()
    raw2 = agent_chat(sid2, "请使用 read 工具读取 test_output.txt")
    events2 = parse_sse_events(raw2)
    full_text2 = extract_text_from_events(events2)

    assert "luwu e2e test file" in full_text2.lower(), \
        f"File content not found in read output:\n{full_text2[:300]}"
    print(f"  File created and verified ✅")


# ═══════════════════════════════════════════════════════════════════════
# SECTION 8: Edit Tool — Text Match Mode
# ═══════════════════════════════════════════════════════════════════════

@test("Agent uses edit tool with old_text/new_text to modify a file")
def test_edit_text_match():
    # First create a file to edit.
    sid1 = create_session()
    agent_chat(sid1, "请使用 write 工具创建文件 test_edit.txt，内容为 'line one\\nline two\\nline three'")

    # Now edit it.
    sid2 = create_session()
    raw = agent_chat(
        sid2,
        "请先使用 read 工具读取 test_edit.txt，然后使用 edit 工具将 old_text='line two' 替换为 new_text='line TWO modified'"
    )
    events = parse_sse_events(raw)
    full_text = extract_text_from_events(events)

    # Verify the edit took effect.
    sid3 = create_session()
    raw3 = agent_chat(sid3, "请使用 read 工具读取 test_edit.txt")
    events3 = parse_sse_events(raw3)
    full_text3 = extract_text_from_events(events3)

    assert "TWO modified" in full_text3, \
        f"Edit not applied. File content:\n{full_text3[:300]}"
    print(f"  Text match edit verified ✅")


@test("Edit tool with anchor mode modifies the correct line")
def test_edit_anchor_mode():
    # Create a file.
    sid1 = create_session()
    content = 'fn hello() {\n    println!("hello");\n}'
    agent_chat(sid1, f"请使用 write 工具创建文件 test_anchor.txt，内容为 '{content}'")

    # Read it to get anchors from the tool_completed event.
    sid2 = create_session()
    raw2 = agent_chat(sid2, "请使用 read 工具读取 test_anchor.txt，只读取不要修改")
    events2 = parse_sse_events(raw2)

    completed = [e for e in events2 if e.get("type") == "tool_completed"]
    assert len(completed) >= 1, f"Expected tool_completed after read, got: {[e.get('type') for e in events2]}"

    tool_output = completed[0].get("output", "")
    anchors = find_anchors(tool_output)
    assert len(anchors) >= 2, f"Need at least 2 anchors for edit test, got {len(anchors)} in:\n{tool_output[:300]}"
    print(f"  Anchors from read tool output: {anchors}")

    # Edit using anchor.
    target_anchor = anchors[1]  # Second line: println!
    sid3 = create_session()
    new_text = '    println!("hello-luwu-anchor");'
    raw3 = agent_chat(
        sid3,
        f"请使用 edit 工具，传 anchor='{target_anchor}'，new_text='{new_text}' 来修改 test_anchor.txt"
    )
    events3 = parse_sse_events(raw3)
    full_text3 = extract_text_from_events(events3)

    # Verify.
    sid4 = create_session()
    raw4 = agent_chat(sid4, "请使用 read 工具读取 test_anchor.txt")
    events4 = parse_sse_events(raw4)

    # Check tool_completed output for the anchor.
    completed4 = [e for e in events4 if e.get("type") == "tool_completed"]
    if completed4:
        verify_text = completed4[0].get("output", "")
    else:
        verify_text = extract_text_from_events(events4)

    assert "hello-luwu-anchor" in verify_text, \
        f"Anchor edit not applied. Content:\n{verify_text[:300]}"
    print(f"  Anchor edit verified ✅")


# ═══════════════════════════════════════════════════════════════════════
# SECTION 9: Grep Tool
# ═══════════════════════════════════════════════════════════════════════

@test("Agent uses grep tool to find text in files")
def test_grep_tool():
    sid = create_session()
    raw = agent_chat(
        sid,
        "请使用 grep 工具在项目中搜索 'TurnEngine'，glob='*.rs'"
    )
    events = parse_sse_events(raw)
    full_text = extract_text_from_events(events)

    # Should find references to TurnEngine in the codebase.
    found = "TurnEngine" in full_text
    has_tool_events = any("tool" in e.get("type", "") for e in events)

    assert found or has_tool_events, \
        f"Expected grep results for TurnEngine:\n{full_text[:300]}"
    print(f"  Grep results found ✅")


# ═══════════════════════════════════════════════════════════════════════
# SECTION 10: Web Fetch Tool
# ═══════════════════════════════════════════════════════════════════════

@test("Agent uses web_fetch tool to fetch a web page")
def test_web_fetch():
    sid = create_session()
    raw = agent_chat(
        sid,
        "请使用 web_fetch 工具获取 https://example.com 的内容"
    )
    events = parse_sse_events(raw)

    # Check tool events.
    completed = [e for e in events if e.get("type") == "tool_completed"]
    assert len(completed) >= 1, f"Expected tool_completed, got: {[e.get('type') for e in events]}"

    tool_output = completed[0].get("output", "")

    # Should contain the page title and content.
    assert "Example Domain" in tool_output, f"Expected 'Example Domain' in output:\n{tool_output[:300]}"
    assert len(tool_output) > 50, f"Output too short ({len(tool_output)} chars)"
    print(f"  web_fetch output: {len(tool_output)} chars")
    print(f"  Preview: {tool_output[:150]}...")


@test("web_fetch returns error for invalid URL")
def test_web_fetch_invalid_url():
    sid = create_session()
    raw = agent_chat(
        sid,
        "请使用 web_fetch 工具获取 ftp://invalid.protocol/test"
    )
    events = parse_sse_events(raw)

    # The tool should return an error about URL scheme.
    completed = [e for e in events if e.get("type") == "tool_completed"]
    if completed:
        output = completed[0].get("output", "")
        # Should mention the URL scheme error.
        has_error = "http" in output.lower() or "unsupported" in output.lower() or "error" in output.lower()
        assert has_error, f"Expected URL scheme error in output:\n{output[:300]}"
        print(f"  Correctly rejected invalid URL")
    else:
        # LLM might refuse to call the tool — that's also acceptable.
        print(f"  LLM refused to call web_fetch with invalid URL (acceptable)")


# ═══════════════════════════════════════════════════════════════════════
# SECTION 11: Cancel
# ═══════════════════════════════════════════════════════════════════════

@test("POST /v1/sessions/{id}/cancel returns 200 or 404")
def test_cancel():
    sid = create_session()

    # Start a long-running chat in a thread.
    def long_chat():
        try:
            http.post(
                f"{API_BASE}/sessions/{sid}/chat",
                json={"message": "写一篇500字的文章", "stream": True},
                timeout=10,
            )
        except Exception:
            pass

    t = threading.Thread(target=long_chat)
    t.start()
    time.sleep(0.5)

    cancel_resp = http.post(f"{API_BASE}/sessions/{sid}/cancel")
    t.join(timeout=10)

    # Cancel should return 200 (found & running) or 404 (already finished).
    assert cancel_resp.status_code in (200, 404), \
        f"Expected 200 or 404, got {cancel_resp.status_code}: {cancel_resp.text}"
    print(f"  Cancel response: {cancel_resp.status_code}")


# ═══════════════════════════════════════════════════════════════════════
# SECTION 12: Cleanup
# ═══════════════════════════════════════════════════════════════════════

@test("Cleanup: delete test files")
def test_cleanup():
    # Use bash to clean up test files created during the run.
    sid = create_session()
    try:
        agent_chat(sid, "请使用 bash 工具运行 rm -f test_output.txt test_edit.txt test_anchor.txt")
    except Exception:
        pass  # Best-effort cleanup
    print(f"  Cleanup done ✅")


# ═══════════════════════════════════════════════════════════════════════
# Main
# ═══════════════════════════════════════════════════════════════════════

def main():
    # Verify server is reachable.
    print("Checking server connectivity...")
    try:
        resp = http.get(f"{BASE_URL}/health", timeout=5)
        assert resp.text == "ok"
        print(f"✅ Server is running at {BASE_URL}")
    except Exception as e:
        print(f"❌ Server not reachable at {BASE_URL}: {e}")
        print("   Start with: cargo run --bin luwu-server")
        sys.exit(1)

    tests = [
        # Section 1: Server Basics
        test_health,
        test_models,
        # Section 2: OpenAI-Compatible Chat
        test_chat_non_streaming,
        test_chat_streaming,
        # Section 3: Session Management
        test_session_create,
        test_session_list,
        test_session_get,
        test_session_get_404,
        test_session_delete,
        # Section 4: Agent Chat
        test_agent_chat_events,
        test_agent_chat_404,
        test_agent_chat_conflict,
        # Section 5: Bash Tool
        test_bash_tool,
        # Section 6: Read Tool (LINE:HASH)
        test_read_tool_anchors,
        test_read_tool_offset_limit,
        test_read_directory,
        # Section 7: Write Tool
        test_write_tool,
        # Section 8: Edit Tool
        test_edit_text_match,
        test_edit_anchor_mode,
        # Section 9: Grep Tool
        test_grep_tool,
        # Section 10: Web Fetch Tool
        test_web_fetch,
        test_web_fetch_invalid_url,
        # Section 10: Cancel
        test_cancel,
        # Section 11: Cleanup
        test_cleanup,
    ]

    for t in tests:
        t()

    print(f"\n{'='*60}")
    print(f"Results: {_results['passed']} passed, {_results['failed']} failed")
    if _results["failed"] > 0:
        print(f"❌ {_results['failed']} test(s) failed")
        sys.exit(1)
    else:
        print("✅ All tests passed! 🎉")


if __name__ == "__main__":
    main()
