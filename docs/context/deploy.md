# Deployment Guide — luwu

luwu is a Rust workspace implementing an OpenAI-compatible agent server with a
microkernel architecture. The deployable artifact is the `luwu-server` binary.

## Prerequisites

| Requirement | Version | Check |
|---|---|---|
| Rust toolchain | 2024 edition (stable) | `rustc --version` |
| Cargo | any recent | `cargo --version` |
| Python 3 | 3.10+ (tests only) | `python3 --version` |
| uv | any recent (tests only) | `uv --version` |

Install Rust if needed:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## Build

```bash
# Full workspace (debug)
cargo build

# Release binary
cargo build --release

# Only the server binary
cargo build -p luwu-server

# Release binary for the server only
cargo build --release -p luwu-server
```

Release binary lands at `target/release/luwu-server`.

Workspace crate layout:

```
crates/
  luwu-core/    — traits, types, event bus, session/memory/skill managers
  luwu-llm/     — LLM provider implementations (OpenAI-compatible)
  luwu-tools/   — built-in tools (bash, read, write, edit, grep, web_fetch)
  luwu-server/  — HTTP server binary (axum)
  luwu-memory/  — memory store, checkpoint, consolidation workers
```

## Configuration

luwu reads its config from a single TOML file:

```
~/.luwu/config.toml
```

Minimal example:

```toml
[default]
provider = "minimax"
model = "MiniMax-M3"

[providers.minimax]
api_key = "your-api-key-here"
base_url = "https://api.minimax.chat/v1"
model = "MiniMax-M3"

[providers.deepseek]
api_key = "your-key"
base_url = "https://api.deepseek.com/v1"
model = "deepseek-v4-flash"

[providers.zhipu]
api_key = "your-key"
base_url = "https://open.bigmodel.cn/api/paas/v4"
model = "glm-4.7"
```

Provider fields: `api_key` (required), `base_url`, `model`, `temperature`,
`max_tokens`. If `base_url` is omitted, defaults to `https://api.openai.com/v1`.

The server refuses to start if no default provider resolves.

### Environment Variables

| Variable | Purpose | Default |
|---|---|---|
| `RUST_LOG` | tracing log filter | `info` |

Example:

```bash
RUST_LOG=debug cargo run -p luwu-server
```

## Running the Server

```bash
# From source (debug)
cargo run -p luwu-server

# From source (release)
cargo run --release -p luwu-server

# Pre-built binary
./target/release/luwu-server

# With debug logging
RUST_LOG=luwu_server=debug,luwu_core=debug ./target/release/luwu-server
```

On startup the server:
1. Loads `~/.luwu/config.toml` (exits if invalid).
2. Resolves the default provider (exits if missing).
3. Initializes `~/.luwu/sessions/` and recovers persisted sessions.
4. Discovers skills from `~/.luwu/` and the working directory.
5. Binds to **127.0.0.1:51740**.

The server is hardcoded to bind on localhost only — it is not designed for
direct internet exposure. Put a reverse proxy (nginx, Caddy) in front if
remote access is needed.

## Data Directory

All persistent state lives under `~/.luwu/`:

```
~/.luwu/
  config.toml          — provider configuration
  sessions/            — session JSON files (auto-recovered on restart)
  skills/              — skill definitions (SKILL.md + files)
  <session-id>/
    observations.jsonl — timestamped observations (Observer worker)
    reflections.jsonl  — synthesized reflections (Reflector worker)
    corrections.jsonl  — detected user corrections
    checkpoint.md      — latest deterministic compaction
```

Sessions persist to disk automatically. On restart, the server recovers
them and resumes where it left off.

## API Endpoints

| Method | Path | Description |
|---|---|---|
| GET | `/health` | Health check → `"ok"` |
| GET | `/v1/models` | List configured models |
| POST | `/v1/chat/completions` | OpenAI-compatible chat (stream + non-stream) |
| GET | `/v1/sessions` | List all sessions |
| POST | `/v1/sessions` | Create session `{"model?": "...", "provider?": "..."}` |
| GET | `/v1/sessions/{id}` | Get session details |
| DELETE | `/v1/sessions/{id}` | Delete session |
| POST | `/v1/sessions/{id}/chat` | Agent chat with full event stream + tools |
| POST | `/v1/sessions/{id}/cancel` | Cancel a running turn |
| GET | `/v1/sessions/{id}/checkpoint` | Get latest memory checkpoint |
| GET | `/v1/sessions/{id}/history` | Search session history `?q=keyword` |
| GET | `/v1/skills` | List discovered skills |
| GET | `/v1/skills/{name}` | Get skill detail |

Quick smoke test:

```bash
curl http://127.0.0.1:51740/health
curl http://127.0.0.1:51740/v1/models
```

## Testing

Tests are Python scripts in `tests/`. They are **not** pytest — each file
uses a custom `@test` decorator and a `main()` runner. All tests require the
server to be running first.

```bash
# 1. Start the server in one terminal
cargo run -p luwu-server

# 2. Run a specific test suite
uv run --with httpx --with openai python3 tests/test_api.py

# Full E2E suite (all providers × all tools, ~40 test cases)
uv run --with httpx --with openai python3 tests/test_e2e.py

# Tool-calling tests
uv run --with httpx --with openai python3 tests/test_tools.py

# LINE:HASH anchor tests
uv run --with httpx --with openai python3 tests/test_hashline.py

# Basic OpenAI SDK smoke test
uv run --with openai python3 tests/e2e_test.py
```

Rust unit tests (if any per-crate):

```bash
cargo test                    # all crates
cargo test -p luwu-core       # single crate
```

## CI / CD Pipeline

No CI pipeline is currently configured (no `.github/workflows/`). A typical
setup would be:

```yaml
# .github/workflows/ci.yml
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release -p luwu-server
      - run: cargo test
```

Integration tests require live API keys and a running server, so they are
excluded from CI by default.

## Docker

No Dockerfile exists yet. To containerize:

```dockerfile
FROM rust:1.85-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p luwu-server

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/luwu-server /usr/local/bin/luwu-server
EXPOSE 51740
CMD ["luwu-server"]
```

```bash
docker build -t luwu-server .
docker run -p 51740:51740 -v ~/.luwu:/root/.luwu luwu-server
```

Mount `~/.luwu` as a volume so config and session data persist.

## Infrastructure Requirements

- **Network**: localhost port 51740. Outbound HTTPS to the configured LLM
  provider's API.
- **Storage**: `~/.luwu/` on disk. Sessions, checkpoints, and memory logs
  grow with usage — no automatic cleanup is implemented.
- **Memory**: The server holds all sessions in-memory (Arc-shared). Memory
  footprint scales with active session count and conversation length.
- **Runtime deps**: None beyond the compiled binary and a valid config file.
  No database, no Redis, no external services.
