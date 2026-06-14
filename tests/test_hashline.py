import httpx, json, re

BASE = "http://127.0.0.1:51740/v1"

# 1) Create session
r = httpx.post(f"{BASE}/sessions", json={}, timeout=10)
print("Create session:", r.status_code, r.json())
SID = r.json()["id"]

# 2) Ask LLM to read a file via agent
print("\n--- Test 1: read tool via agent chat ---")
r = httpx.post(f"{BASE}/sessions/{SID}/chat", json={
    "message": "请使用 read 工具读取 crates/luwu-tools/src/hashline.rs 的前10行，只读取，不要做任何修改"
}, timeout=60)
print("Status:", r.status_code)
body = r.text
anchors = re.findall(r'\d+:[0-9a-f]{3}\|', body)
if anchors:
    print(f"✅ Found {len(anchors)} LINE:HASH anchors in output!")
    print("First few:", anchors[:5])
else:
    print("❌ No LINE:HASH anchors found")
    print("Response preview:", body[:500])

# 3) Test anchor edit - ask LLM to edit using anchor
print("\n--- Test 2: anchor edit via agent chat ---")
r = httpx.post(f"{BASE}/sessions/{SID}/chat", json={
    "message": "用 read 工具读取 Cargo.toml 的前5行"
}, timeout=60)
body = r.text
anchors = re.findall(r'(\d+:[0-9a-f]{3})\|', body)
print("Anchors found:", anchors[:5] if anchors else "none")

if anchors:
    first_anchor = anchors[0]
    print(f"\nAsking LLM about anchor format...")
    r = httpx.post(f"{BASE}/sessions/{SID}/chat", json={
        "message": f"不要执行任何修改，只告诉我你看到了 LINE:HASH 格式的锚点了吗？比如 {first_anchor} 这样的格式"
    }, timeout=60)
    print("Status:", r.status_code)
    if "anchor" in r.text.lower() or "锚点" in r.text or first_anchor in r.text or "LINE:HASH" in r.text:
        print("✅ LLM recognizes LINE:HASH format!")
    else:
        print("Response preview:", r.text[:300])

print("\nDone!")
