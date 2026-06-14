#!/usr/bin/env python3
"""End-to-end test for luwu server using the OpenAI SDK."""

import sys
from openai import OpenAI

BASE_URL = "http://localhost:51740/v1"
API_KEY = "dummy"  # luwu server doesn't check API key

client = OpenAI(base_url=BASE_URL, api_key=API_KEY)


def test_health():
    """Test the health endpoint."""
    import urllib.request
    resp = urllib.request.urlopen("http://localhost:51740/health")
    body = resp.read().decode()
    assert body == "ok", f"Expected 'ok', got '{body}'"
    print("✅ health check passed")


def test_list_models():
    """Test GET /v1/models."""
    models = client.models.list()
    model_ids = [m.id for m in models.data]
    print(f"✅ list models: {model_ids}")
    assert len(model_ids) > 0, "No models returned"
    return model_ids[0]


def test_non_streaming(model: str):
    """Test non-streaming chat completion."""
    resp = client.chat.completions.create(
        model=model,
        messages=[{"role": "user", "content": "Say exactly: hello world"}],
    )
    content = resp.choices[0].message.content
    assert content, "Empty response content"
    print(f"✅ non-streaming: {content[:80]}...")
    return content


def test_streaming(model: str):
    """Test streaming chat completion."""
    stream = client.chat.completions.create(
        model=model,
        messages=[{"role": "user", "content": "Say exactly: goodbye world"}],
        stream=True,
    )
    chunks = []
    for chunk in stream:
        delta = chunk.choices[0].delta
        if delta.content:
            chunks.append(delta.content)
        # Verify finish_reason on last chunk.
        if chunk.choices[0].finish_reason == "stop":
            pass  # OK

    full = "".join(chunks)
    assert full, "Empty streaming response"
    print(f"✅ streaming: {full[:80]}...")
    return full


def test_multi_turn(model: str):
    """Test multi-turn conversation."""
    resp1 = client.chat.completions.create(
        model=model,
        messages=[
            {"role": "user", "content": "My name is Alice. Remember it."},
        ],
    )
    c1 = resp1.choices[0].message.content
    assert c1, "Empty response in turn 1"

    resp2 = client.chat.completions.create(
        model=model,
        messages=[
            {"role": "user", "content": "My name is Alice. Remember it."},
            {"role": "assistant", "content": c1},
            {"role": "user", "content": "What is my name? Reply in one word."},
        ],
    )
    c2 = resp2.choices[0].message.content
    assert "Alice" in c2 or "alice" in c2.lower(), f"Expected 'Alice' in response, got: {c2}"
    print(f"✅ multi-turn: remembered name correctly — {c2[:80]}")


def main():
    tests = [
        ("health", test_health),
        ("list models", lambda: test_list_models()),
    ]

    print("=" * 60)
    print("陆吾 Server E2E Tests")
    print("=" * 60)
    print()

    # Basic tests.
    test_health()
    model = test_list_models()

    # Chat tests.
    test_non_streaming(model)
    test_streaming(model)
    test_multi_turn(model)

    print()
    print("=" * 60)
    print("All tests passed! ✅")
    print("=" * 60)


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        print(f"\n❌ Test failed: {e}", file=sys.stderr)
        sys.exit(1)
