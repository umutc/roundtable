# roundtable

## Build & Test

```bash
cargo build --release

# Quick test with haiku (cheap + fast)
cargo run -- debate -m haiku -r 1
cargo run -- --topic "test question" -m haiku -r 1
cargo run --bin roundtable-chat -- "test topic" -r 1 --model haiku

# Install to PATH
make install
```

## Architecture

### src/main.rs — `roundtable` binary
N agents + moderator orchestration. Flow:
1. Parse config (file, built-in name, or generate default from `--topic`)
2. Build `Panel` struct with agents, moderator, and project context
3. Opening round: each agent gives initial view
4. Loop: moderator analyzes → returns `{"action":"continue"}` or `{"action":"synthesize"}`
5. If continue: agents respond to moderator guidance
6. If synthesize: moderator writes final synthesis report

Key structs:
- `Config` / `SessionConfig` / `AgentConfig` — TOML deserialization
- `Panel` — runtime state (agents, transcript, cost tracking)
- `PanelAgent` — per-agent state (session_id, model override, tools)
- `ClaudeCallParams` — all parameters for a single `claude -p` invocation
- `ModeratorDecision` — parsed `{"action", "guidance"}` from moderator response

Key functions:
- `resolve_config()` — maps "debate" → embedded TOML, or reads file
- `default_config()` — generates 2-agent config for `--topic` mode
- `call_claude()` — spawns `claude -p` process, parses JSON array response
- `parse_moderator_decision()` — reverse line scan for JSON, keyword fallback
- `enrich_prompt()` — appends project context to prompts

Built-in configs: `include_str!()` embeds `examples/*.toml` at compile time.

### src/duo.rs — `roundtable-chat` binary
Simple 2-agent ping-pong. No TOML config needed — all via CLI flags.
Agent A opens → rounds of B-responds/A-responds → final summary.

## Output Formats

### `--output-json`
```json
{
  "topic": "string",
  "total_cost_usd": 0.0,
  "transcript_path": "/path/to/transcript.jsonl",
  "rounds": 5,
  "synthesis": "Markdown report...",
  "agents": ["Agent A", "Agent B"]
}
```

### Transcript JSONL (one per line)
```json
{"timestamp":"2026-04-15T14:30:00+03:00","agent":"Backend Dev","round":1,"message":"..."}
```

## Conventions

- All user-facing strings: English
- TOML schema key: `[[agents]]` (not "experts")
- Moderator JSON contract: `{"action": "continue|synthesize", "guidance": "..."}`
- Terminal colors via `colored` crate: red/blue/green/yellow/magenta/cyan/white
- Cost tracked from claude JSON response `total_cost_usd` field

## Project Structure

```
src/main.rs          — roundtable (N agents + moderator)
src/duo.rs           — roundtable-chat (2 agents)
examples/*.toml      — config templates (embedded in binary via include_str!)
npm/roundtable/      — npm main package (JS shims)
npm/{platform}/      — platform-specific binary packages
.github/workflows/   — CI: build 4 platforms + publish npm + GitHub release
```
