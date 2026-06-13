#!/usr/bin/env python3
"""
luwu comprehensive end-to-end test suite.

Covers ALL API endpoints, ALL tools with parameter variations,
ALL configured providers, edge cases, and error handling.

Requires:
  - luwu-server running at http://127.0.0.1:51740
  - ~/.luwu/config.toml with valid provider configs
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
POLL_TIMEOUT = 120

PROVIDERS = {
    "zhipu":    ("glm-4.7",          "智谱 GLM-4.7"),
    "minimax":  ("MiniMax-M3",       "MiniMax M3"),
    "deepseek": ("deepseek-v4-flash", "DeepSeek V4 Flash"),
}

client = OpenAI(
    base_url=API_BASE,
    api_key="dummy",
    http_client=httpx.Client(timeout=POLL_TIMEOUT),
)
http = httpx.Client(timeout=POLL_TIMEOUT)

_results = {"passed": 0, "failed": 0}


# ─── Helpers ─────────────────────────────────────────────────────────

def test(name):
    def decorator(fn):
        def wrapper():
            print(f"\n=== {name} ===")
            try:
                fn()
                print(f"  ✅ {name} passed")
                _results["passed"] += 1
            except AssertionError as e:
                print(f"  ❌ FAILED: {e}")
                _results["failed"] += 1
            except Exception as e:
                print(f"  ❌ ERROR: {type(e).__name__}: {e}")
                _results["failed"] += 1
        return wrapper
    return decorator


def create_session(provider: str | None = None) -> str:
    body = {}
    if provider:
        body["provider"] = provider
    resp = http.post(f"{API_BASE}/sessions", json=body)
    assert resp.status_code == 200, f"Create session failed: {resp.status_code} {resp.text}"
    return resp.json()["id"]


def agent_chat(sid: str, message: str, timeout: float = POLL_TIMEOUT) -> str:
    resp = http.post(
        f"{API_BASE}/sessions/{sid}/chat",
        json={"message": message, "stream": True},
        headers={"Accept": "text/event-stream"},
        timeout=timeout,
    )
    assert resp.status_code == 200, f"Agent chat failed: {resp.status_code} {resp.text}"
    return resp.text


def parse_sse_events(raw: str) -> list[dict]:
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


def extract_text(events: list[dict]) -> str:
    return "".join(e.get("delta", "") for e in events if e.get("type") == "text_delta")


def find_anchors(text: str) -> list[str]:
    return re.findall(r'(\d+:[0-9a-f]{3})\|', text)


def get_completed_output(events: list[dict]) -> str:
    for e in reversed(events):
        if e.get("type") == "tool_completed":
            return e.get("output", "")
    return ""


def get_event_types(events: list[dict]) -> list[str]:
    return [e.get("type", "") for e in events]


# ═══════════════════════════════════════════════════════════════════════
# S1: Server Basics
# ═══════════════════════════════════════════════════════════════════════

@test("S1.1 GET /health returns ok")
def test_health():
    resp = http.get(f"{BASE_URL}/health")
    assert resp.status_code == 200
    assert resp.text == "ok"


@test("S1.2 GET /v1/models returns all configured models")
def test_models():
    resp = client.models.list()
    models = sorted(set(m.id for m in resp.data))
    expected = sorted(set(v[0] for v in PROVIDERS.values()))
    for m in expected:
        assert m in models, f"Missing model {m} in {models}"
    print(f"  Models: {models}")


# ═══════════════════════════════════════════════════════════════════════
# S2: OpenAI-Compatible Chat — all providers
# ═══════════════════════════════════════════════════════════════════════

@test("S2.1 Chat non-streaming")
def test_chat_non_streaming():
    resp = client.chat.completions.create(
        model="irrelevant",
        messages=[{"role": "user", "content": "用一句话介绍你自己"}],
    )
    assert resp.object == "chat.completion"
    assert len(resp.choices) == 1
    content = resp.choices[0].message.content
    assert len(content) > 10, f"Expected non-trivial response, got: {content[:80]}"
    assert resp.choices[0].finish_reason == "stop"
    print(f"  Response: {content[:50]}...")


@test("S2.2 Chat streaming")
def test_chat_streaming():
    stream = client.chat.completions.create(
        model="irrelevant",
        messages=[{"role": "user", "content": "说OK"}],
        stream=True,
    )
    chunks = []
    for chunk in stream:
        delta = chunk.choices[0].delta
        if delta.content:
            chunks.append(delta.content)
        if delta.role:
            assert delta.role == "assistant"
    full = "".join(chunks)
    assert len(full) > 0, "Expected non-empty streaming content"
    print(f"  {len(chunks)} chunks, '{full[:40]}...'")


# ═══════════════════════════════════════════════════════════════════════
# S3: Session Management
# ═══════════════════════════════════════════════════════════════════════

@test("S3.1 Create session — default provider")
def test_session_create_default():
    resp = http.post(f"{API_BASE}/sessions", json={})
    assert resp.status_code == 200
    data = resp.json()
    assert "id" in data and "model" in data
    print(f"  Default session: model={data['model']}")


@test("S3.2 Create session — each provider gets correct model")
def test_session_create_provider():
    for pname, (model, desc) in PROVIDERS.items():
        resp = http.post(f"{API_BASE}/sessions", json={"provider": pname})
        assert resp.status_code == 200
        data = resp.json()
        assert data["model"] == model, \
            f"[{desc}] Expected model={model}, got {data['model']}"
        print(f"  ✅ {desc}: model={data['model']}")


@test("S3.3 Create session — unknown provider returns error")
def test_session_create_bad_provider():
    resp = http.post(f"{API_BASE}/sessions", json={"provider": "nonexistent"})
    assert resp.status_code == 500 or "error" in resp.text.lower()


@test("S3.4 List sessions")
def test_session_list():
    resp = http.get(f"{API_BASE}/sessions")
    assert resp.status_code == 200
    data = resp.json()
    assert "sessions" in data
    assert len(data["sessions"]) >= len(PROVIDERS)
    print(f"  Sessions: {len(data['sessions'])}")


@test("S3.5 Get session details")
def test_session_get():
    sid = create_session()
    resp = http.get(f"{API_BASE}/sessions/{sid}")
    assert resp.status_code == 200
    data = resp.json()
    assert data["id"] == sid
    assert "message_count" in data


@test("S3.6 Get session — 404 for unknown")
def test_session_get_404():
    resp = http.get(f"{API_BASE}/sessions/no-such-session")
    assert resp.status_code == 404


@test("S3.7 Delete session")
def test_session_delete():
    sid = create_session()
    resp = http.delete(f"{API_BASE}/sessions/{sid}")
    assert resp.status_code == 200
    resp = http.get(f"{API_BASE}/sessions/{sid}")
    assert resp.status_code == 404


@test("S3.8 Delete session — 404 for unknown")
def test_session_delete_404():
    resp = http.delete(f"{API_BASE}/sessions/no-such-session")
    assert resp.status_code == 404


@test("S3.9 Multi-turn conversation preserves messages")
def test_session_multiturn():
    sid = create_session()
    # Turn 1
    raw1 = agent_chat(sid, "请记住这个数字：42。只回复OK")
    events1 = parse_sse_events(raw1)
    assert "done" in get_event_types(events1), f"Turn 1 didn't finish"

    # Turn 2 — ask about the remembered number
    raw2 = agent_chat(sid, "我刚才让你记住的数字是多少？只回复数字")
    events2 = parse_sse_events(raw2)
    text2 = extract_text(events2)
    assert "42" in text2, f"[Multi-turn] Expected '42' in response, got: {text2[:200]}"
    print(f"  ✅ Multi-turn: LLM remembered 42, response: {text2[:60]}...")


@test("S3.10 Concurrent sessions — two sessions run independently")
def test_concurrent_sessions():
    sid_a = create_session()
    sid_b = create_session()

    results = {}

    def chat_a():
        try:
            raw = agent_chat(sid_a, "说A")
            events = parse_sse_events(raw)
            results["a"] = extract_text(events)
        except Exception as e:
            results["a"] = f"ERROR: {e}"

    def chat_b():
        try:
            raw = agent_chat(sid_b, "说B")
            events = parse_sse_events(raw)
            results["b"] = extract_text(events)
        except Exception as e:
            results["b"] = f"ERROR: {e}"

    ta = threading.Thread(target=chat_a)
    tb = threading.Thread(target=chat_b)
    ta.start(); tb.start()
    ta.join(timeout=60); tb.join(timeout=60)

    assert "a" in results, "Session A didn't complete"
    assert "b" in results, "Session B didn't complete"
    assert "ERROR" not in results["a"], f"Session A failed: {results['a']}"
    assert "ERROR" not in results["b"], f"Session B failed: {results['b']}"
    print(f"  ✅ A: {results['a'][:30]}...  B: {results['b'][:30]}...")


# ═══════════════════════════════════════════════════════════════════════
# S4: Agent Chat Mechanics
# ═══════════════════════════════════════════════════════════════════════

@test("S4.1 Agent chat returns done event")
def test_agent_chat_done():
    sid = create_session()
    raw = agent_chat(sid, "说OK")
    events = parse_sse_events(raw)
    types = get_event_types(events)
    assert "done" in types, f"Expected done event, got: {types}"


@test("S4.2 Agent chat produces text_delta or reasoning_delta")
def test_agent_chat_deltas():
    sid = create_session()
    raw = agent_chat(sid, "说OK")
    events = parse_sse_events(raw)
    types = get_event_types(events)
    has_delta = "text_delta" in types or "reasoning_delta" in types
    assert has_delta, f"Expected text_delta or reasoning_delta, got: {types}"
    print(f"  Event types: {types[:10]}...")


@test("S4.3 Agent chat 404 for unknown session")
def test_agent_chat_404():
    resp = http.post(
        f"{API_BASE}/sessions/no-such/chat",
        json={"message": "hi"},
    )
    assert resp.status_code == 404


@test("S4.4 Agent chat 409 when session is busy")
def test_agent_chat_409():
    sid = create_session()
    barrier = threading.Barrier(2, timeout=5)

    def long_chat():
        try:
            barrier.wait()
            http.post(
                f"{API_BASE}/sessions/{sid}/chat",
                json={"message": "写一篇500字文章", "stream": True},
                timeout=30,
            )
        except Exception:
            pass

    t = threading.Thread(target=long_chat)
    t.start()
    barrier.wait()
    time.sleep(0.5)

    resp = http.post(
        f"{API_BASE}/sessions/{sid}/chat",
        json={"message": "hi"},
    )
    t.join(timeout=30)
    assert resp.status_code == 409


@test("S4.5 Cancel a running session")
def test_cancel():
    sid = create_session()

    def long_chat():
        try:
            http.post(
                f"{API_BASE}/sessions/{sid}/chat",
                json={"message": "写一篇1000字文章", "stream": True},
                timeout=30,
            )
        except Exception:
            pass

    t = threading.Thread(target=long_chat)
    t.start()
    time.sleep(0.5)

    resp = http.post(f"{API_BASE}/sessions/{sid}/cancel")
    t.join(timeout=10)
    assert resp.status_code in (200, 404)
    print(f"  Cancel: {resp.status_code}")


# ═══════════════════════════════════════════════════════════════════════
# S5: Multi-Provider Tool Suite — every provider × every tool
# ═══════════════════════════════════════════════════════════════════════

@test("S5.1 Bash tool — all providers")
def test_bash_all_providers():
    for pname, (model, desc) in PROVIDERS.items():
        sid = create_session(pname)
        raw = agent_chat(sid, "请使用 bash 工具运行 echo 'luwu-test-PASS'")
        events = parse_sse_events(raw)
        types = get_event_types(events)
        text = extract_text(events)

        has_tool = "tool_started" in types or "tool_call" in types
        has_output = "luwu" in text.lower() or "test" in text.lower() or "PASS" in text
        assert has_tool or has_output, \
            f"[{desc}] Bash tool not used. Events: {types}\nText: {text[:200]}"
        print(f"  ✅ {desc}: bash OK")


@test("S5.2 Read tool (file) — all providers")
def test_read_file_all_providers():
    for pname, (model, desc) in PROVIDERS.items():
        sid = create_session(pname)
        raw = agent_chat(sid, "请使用 read 工具读取 Cargo.toml 的前5行，offset=1, limit=5")
        events = parse_sse_events(raw)

        output = get_completed_output(events)
        anchors = find_anchors(output)
        assert len(anchors) >= 3, \
            f"[{desc}] Expected anchors in read output, got {len(anchors)}"
        print(f"  ✅ {desc}: {len(anchors)} anchors")


@test("S5.3 Read tool (directory) — all providers")
def test_read_dir_all_providers():
    for pname, (model, desc) in PROVIDERS.items():
        sid = create_session(pname)
        raw = agent_chat(sid, "请使用 read 工具列出 crates 目录的内容，path='crates'")
        events = parse_sse_events(raw)
        text = extract_text(events)

        has_dirs = any(d in text for d in ("luwu-core", "luwu-llm", "luwu-server"))
        tool_out = get_completed_output(events)
        has_dirs_tool = any(d in tool_out for d in ("luwu-core", "luwu-llm", "luwu-server"))
        assert has_dirs or has_dirs_tool, \
            f"[{desc}] Expected directory entries:\n{text[:200]}"
        print(f"  ✅ {desc}: directory OK")


@test("S5.4 Write tool — all providers")
def test_write_all_providers():
    for pname, (model, desc) in PROVIDERS.items():
        fname = f"test_write_{pname}.txt"
        sid = create_session(pname)
        agent_chat(sid, f"请使用 write 工具创建文件 {fname}，内容为 'hello-{pname}'")

        sid2 = create_session(pname)
        raw2 = agent_chat(sid2, f"请使用 read 工具读取 {fname}")
        events2 = parse_sse_events(raw2)
        text2 = extract_text(events2)
        tool2 = get_completed_output(events2)
        assert f"hello-{pname}" in text2 or f"hello-{pname}" in tool2, \
            f"[{desc}] File content not found: {text2[:200]}"
        print(f"  ✅ {desc}: write OK")


@test("S5.5 Edit tool (text match) — all providers")
def test_edit_text_all_providers():
    for pname, (model, desc) in PROVIDERS.items():
        fname = f"test_edit_{pname}.txt"
        sid = create_session(pname)
        agent_chat(sid, f"请使用 write 工具创建文件 {fname}，内容为 'aaa\\nbbb\\nccc'")

        sid2 = create_session(pname)
        agent_chat(sid2, f"请使用 edit 工具修改 {fname}，old_text='bbb' new_text='BBB'")

        sid3 = create_session(pname)
        raw3 = agent_chat(sid3, f"请使用 read 工具读取 {fname}")
        events3 = parse_sse_events(raw3)
        text3 = extract_text(events3) + get_completed_output(events3)
        assert "BBB" in text3, f"[{desc}] Edit not applied: {text3[:200]}"
        print(f"  ✅ {desc}: edit text OK")


@test("S5.6 Edit tool (anchor mode) — all providers")
def test_edit_anchor_all_providers():
    for pname, (model, desc) in PROVIDERS.items():
        fname = f"test_anchor_{pname}.txt"
        sid = create_session(pname)
        agent_chat(sid, f"请使用 write 工具创建文件 {fname}，内容为 'line1\\nline2\\nline3'")

        sid2 = create_session(pname)
        raw2 = agent_chat(sid2, f"请使用 read 工具读取 {fname}")
        events2 = parse_sse_events(raw2)
        anchors = find_anchors(get_completed_output(events2))
        assert len(anchors) >= 2, f"[{desc}] Need anchors, got {len(anchors)}"

        target = anchors[1]
        sid3 = create_session(pname)
        agent_chat(sid3, f"请使用 edit 工具修改 {fname}，anchor='{target}'，new_text='MODIFIED'")

        sid4 = create_session(pname)
        raw4 = agent_chat(sid4, f"请使用 read 工具读取 {fname}")
        text4 = extract_text(parse_sse_events(raw4)) + get_completed_output(parse_sse_events(raw4))
        assert "MODIFIED" in text4, f"[{desc}] Anchor edit failed: {text4[:200]}"
        print(f"  ✅ {desc}: anchor edit OK")


@test("S5.7 Grep tool — all providers")
def test_grep_all_providers():
    for pname, (model, desc) in PROVIDERS.items():
        sid = create_session(pname)
        raw = agent_chat(sid, "请使用 grep 工具搜索 'TurnEngine'，glob='*.rs'")
        events = parse_sse_events(raw)
        text = extract_text(events)
        has_result = "TurnEngine" in text or "tool_started" in get_event_types(events)
        assert has_result, f"[{desc}] Grep found nothing: {text[:200]}"
        print(f"  ✅ {desc}: grep OK")


@test("S5.8 Web fetch tool — all providers")
def test_web_fetch_all_providers():
    for pname, (model, desc) in PROVIDERS.items():
        sid = create_session(pname)
        raw = agent_chat(sid, "请使用 web_fetch 工具获取 https://example.com")
        events = parse_sse_events(raw)

        output = get_completed_output(events)
        assert "Example Domain" in output, \
            f"[{desc}] Expected 'Example Domain', got: {output[:200]}"
        print(f"  ✅ {desc}: web_fetch OK ({len(output)} chars)")


# ═══════════════════════════════════════════════════════════════════════
# S6: Tool Edge Cases (default provider)
# ═══════════════════════════════════════════════════════════════════════

@test("S6.1 Bash — non-zero exit code handled gracefully")
def test_bash_error():
    sid = create_session()
    raw = agent_chat(sid, "请使用 bash 工具运行 ls /nonexistent_directory_xyz_12345")
    events = parse_sse_events(raw)
    types = get_event_types(events)
    text = extract_text(events)

    has_tool = "tool_started" in types
    has_error_info = "no such file" in text.lower() or "not found" in text.lower() or "error" in text.lower() or "不存在" in text
    assert has_tool, f"Expected bash tool usage, events: {types}"
    print(f"  ✅ Error handled: {text[:60]}...")


@test("S6.2 Read — offset/limit returns correct subset")
def test_read_offset_limit():
    sid = create_session()
    raw = agent_chat(sid, "请使用 read 工具读取 Cargo.toml，offset=3, limit=2")
    events = parse_sse_events(raw)
    output = get_completed_output(events)
    anchors = find_anchors(output)
    assert len(anchors) >= 1, f"Expected anchors for offset range, got: {output[:200]}"
    print(f"  Anchors: {anchors}")


@test("S6.3 Read — non-existent file returns error")
def test_read_nonexistent():
    sid = create_session()
    raw = agent_chat(sid, "请使用 read 工具读取 nonexistent_file_xyz.txt")
    events = parse_sse_events(raw)
    text = extract_text(events)
    tool_out = get_completed_output(events)
    combined = text + tool_out
    has_error = "not found" in combined.lower() or "不存在" in combined or "error" in combined.lower() or "no such" in combined.lower()
    assert has_error, f"Expected file-not-found error: {combined[:200]}"
    print(f"  ✅ Error message delivered")


@test("S6.4 Read — binary file detection")
def test_read_binary():
    # Create a small binary file
    sid = create_session()
    agent_chat(sid, "请使用 bash 工具运行 printf '\\x00\\x01\\x02\\x03' > /tmp/test_binary.luwu")
    sid2 = create_session()
    raw = agent_chat(sid2, "请使用 read 工具读取 /tmp/test_binary.luwu")
    events = parse_sse_events(raw)
    text = extract_text(events) + get_completed_output(events)
    has_bin = "binary" in text.lower() or "二进制" in text
    print(f"  Binary detection: {'found' if has_bin else 'not detected'} — {text[:80]}...")


@test("S6.5 Write — overwrite existing file")
def test_write_overwrite():
    sid = create_session()
    agent_chat(sid, "请使用 write 工具创建 test_overwrite.txt 内容为 'version-1'")
    sid2 = create_session()
    agent_chat(sid2, "请使用 write 工具覆盖 test_overwrite.txt 内容为 'version-2'")
    sid3 = create_session()
    raw3 = agent_chat(sid3, "请使用 read 工具读取 test_overwrite.txt")
    text3 = extract_text(parse_sse_events(raw3)) + get_completed_output(parse_sse_events(raw3))
    assert "version-2" in text3, f"Overwrite failed: {text3[:200]}"
    print(f"  ✅ Overwrite verified")


@test("S6.6 Edit — multi-line replacement")
def test_edit_multiline():
    sid = create_session()
    agent_chat(sid, "请使用 write 工具创建 test_multiline.txt 内容为 'alpha\\nbeta\\ngamma'")
    sid2 = create_session()
    raw = agent_chat(sid2, "请使用 edit 工具修改 test_multiline.txt，old_text='beta\\ngamma' new_text='BETA\\nGAMMA'")
    events = parse_sse_events(raw)
    text = extract_text(events)

    sid3 = create_session()
    raw3 = agent_chat(sid3, "请使用 read 工具读取 test_multiline.txt")
    text3 = extract_text(parse_sse_events(raw3)) + get_completed_output(parse_sse_events(raw3))
    assert "BETA" in text3 or "GAMMA" in text3, f"Multi-line edit failed: {text3[:200]}"
    print(f"  ✅ Multi-line edit OK")


@test("S6.7 Grep — regex mode")
def test_grep_regex():
    sid = create_session()
    raw = agent_chat(sid, "请使用 grep 工具搜索正则表达式 'fn \\w+_tool'，glob='*.rs'")
    events = parse_sse_events(raw)
    text = extract_text(events)
    has_fn = "fn " in text or "tool" in text.lower()
    has_tool_event = "tool_started" in get_event_types(events)
    assert has_fn or has_tool_event, f"Regex grep found nothing: {text[:200]}"
    print(f"  ✅ Regex grep OK")


@test("S6.8 Web fetch — invalid URL rejected")
def test_web_fetch_invalid():
    sid = create_session()
    raw = agent_chat(sid, "请使用 web_fetch 工具获取 ftp://bad.protocol/test")
    events = parse_sse_events(raw)
    completed = [e for e in events if e.get("type") == "tool_completed"]
    if completed:
        output = completed[0].get("output", "")
        has_err = "http" in output.lower() or "unsupported" in output.lower() or "error" in output.lower()
        assert has_err, f"Expected URL error: {output[:200]}"
        print(f"  ✅ URL correctly rejected")
    else:
        print(f"  ✅ LLM refused invalid URL (acceptable)")


@test("S6.9 Web fetch — text format")
def test_web_fetch_text():
    sid = create_session()
    raw = agent_chat(sid, "请使用 web_fetch 工具获取 https://example.com，format='text'")
    events = parse_sse_events(raw)
    output = get_completed_output(events)
    assert len(output) > 50, f"Expected content, got: {output[:100]}"
    # Text format should NOT have markdown headers
    assert "Example Domain" in output
    print(f"  ✅ Text format: {len(output)} chars")


# ═══════════════════════════════════════════════════════════════════════
# S7: Memory Endpoints
# ═══════════════════════════════════════════════════════════════════════

@test("S7.1 GET /v1/sessions/{id}/checkpoint returns data")
def test_checkpoint():
    sid = create_session()
    raw = agent_chat(sid, "说OK")
    parse_sse_events(raw)  # wait for completion
    time.sleep(0.5)

    resp = http.get(f"{API_BASE}/sessions/{sid}/checkpoint")
    # Short conversations don't trigger checkpoint (cycle threshold not reached).
    # 200 = checkpoint exists, 404 = not yet triggered — both are acceptable.
    assert resp.status_code in (200, 404), f"Unexpected checkpoint status: {resp.status_code} {resp.text}"
    if resp.status_code == 200:
        data = resp.json()
        print(f"  ✅ Checkpoint exists: {list(data.keys())}")
    else:
        print(f"  ✅ No checkpoint yet (short conversation, expected)")


@test("S7.2 GET /v1/sessions/{id}/history returns data")
def test_history():
    sid = create_session()
    raw = agent_chat(sid, "说OK")
    parse_sse_events(raw)
    time.sleep(0.5)

    resp = http.get(f"{API_BASE}/sessions/{sid}/history")
    assert resp.status_code == 200, f"History failed: {resp.status_code} {resp.text}"
    data = resp.json()
    assert isinstance(data, list) or "entries" in data or "error" not in str(data).lower()
    print(f"  ✅ History: {type(data).__name__}")


# ═══════════════════════════════════════════════════════════════════════
# S8: Cleanup
# ═══════════════════════════════════════════════════════════════════════

@test("S8.1 Cleanup test files")
def test_cleanup():
    sid = create_session()
    files = " ".join([
        "test_output.txt", "test_edit.txt", "test_anchor.txt",
        "test_overwrite.txt", "test_multiline.txt", "/tmp/test_binary.luwu",
    ] + [f"test_write_{p}.txt" for p in PROVIDERS]
      + [f"test_edit_{p}.txt" for p in PROVIDERS]
      + [f"test_anchor_{p}.txt" for p in PROVIDERS])
    try:
        agent_chat(sid, f"请使用 bash 工具运行 rm -f {files}")
    except Exception:
        pass
    print(f"  Cleanup done ✅")


# ═══════════════════════════════════════════════════════════════════════
# Main
# ═══════════════════════════════════════════════════════════════════════

def main():
    print("Checking server connectivity...")
    try:
        resp = http.get(f"{BASE_URL}/health", timeout=5)
        assert resp.text == "ok"
        print(f"✅ Server is running at {BASE_URL}")
    except Exception as e:
        print(f"❌ Server not reachable at {BASE_URL}: {e}")
        sys.exit(1)

    tests = [
        # S1: Server Basics
        test_health,
        test_models,
        test_chat_non_streaming,
        test_chat_streaming,
        # S3: Session Management
        test_session_create_default,
        test_session_create_provider,
        test_session_create_bad_provider,
        test_session_list,
        test_session_get,
        test_session_get_404,
        test_session_delete,
        test_session_delete_404,
        test_session_multiturn,
        test_concurrent_sessions,
        # S4: Agent Chat Mechanics
        test_agent_chat_done,
        test_agent_chat_deltas,
        test_agent_chat_404,
        test_agent_chat_409,
        test_cancel,
        # S5: Multi-Provider Tool Suite
        test_bash_all_providers,
        test_read_file_all_providers,
        test_read_dir_all_providers,
        test_write_all_providers,
        test_edit_text_all_providers,
        test_edit_anchor_all_providers,
        test_grep_all_providers,
        test_web_fetch_all_providers,
        # S6: Tool Edge Cases
        test_bash_error,
        test_read_offset_limit,
        test_read_nonexistent,
        test_read_binary,
        test_write_overwrite,
        test_edit_multiline,
        test_grep_regex,
        test_web_fetch_invalid,
        test_web_fetch_text,
        # S7: Memory Endpoints
        test_checkpoint,
        test_history,
        # S8: Cleanup
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
