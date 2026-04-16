# roundtable

Multiple Claude Code agents debate. One moderator synthesizes.

## Why

Claude Code's sub-agents can't talk to each other — they only report back to the parent. Roundtable solves this by orchestrating N independent Claude Code processes through a shared transcript, enabling real multi-perspective debate on any topic.

A **moderator agent** drives the discussion: identifies gaps, asks targeted questions, detects convergence, and produces a structured synthesis.

## Install

**npx (zero install):**
```bash
npx roundtable debate -m haiku -r 2
```

**npm global:**
```bash
npm install -g roundtable
```

**From source:**
```bash
git clone https://github.com/umutcelik/roundtable.git
cd roundtable
make install   # builds + copies to ~/.local/bin/
```

**Requires:** [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) installed and authenticated (`claude` in PATH).

## Quick Start

### Zero-config debate

```bash
roundtable --topic "Should we use microservices?" -m haiku -r 2
```

No TOML file needed — generates a default Advocate vs Critic debate.

### Built-in configs

```bash
roundtable debate -m haiku -r 2
roundtable microservices -m sonnet -r 3
```

### Code review on your project

```bash
roundtable code-review \
  --cwd "$(pwd)" \
  --context-file CLAUDE.md \
  --topic "Review the auth architecture"
```

### Use from inside Claude Code

```bash
result=$(roundtable debate --topic "Monorepo vs multi-repo?" --output-json -m sonnet)
echo "$result" | jq -r '.synthesis'
```

`--output-json` returns machine-readable output that a parent Claude Code session can parse and act on.

## How It Works

```
+---------------------------------------------+
|              roundtable binary               |
+---------------------------------------------+
|                                              |
|  Opening: All agents give initial views      |
|                                              |
|  Round N:                                    |
|    1. Moderator analyzes transcript          |
|    2. Moderator returns:                     |
|       {"action":"continue","guidance":"..."}  |
|       or {"action":"synthesize"}             |
|    3. If continue: agents respond            |
|    4. If synthesize: final report            |
|                                              |
|  Output: Structured synthesis report         |
|  Transcript: JSONL file for audit            |
+---------------------------------------------+
```

Each agent maintains its own Claude Code session (`--resume`), so context builds across rounds.

## Configuration

Configs are TOML files. See [`examples/`](examples/) for templates. You can also use built-in names (`debate`, `code-review`, `microservices`) or skip config entirely with `--topic`.

```toml
[session]
topic = "Should we migrate to microservices?"
model = "sonnet"            # default model for agents
moderator_model = "opus"    # smarter model for moderator
max_rounds = 10             # safety limit
max_turns = 2               # claude turns per call
# fallback_model = "sonnet" # auto-fallback on overload
# max_budget_usd = 5.0      # cost limit
# effort = "high"           # Opus only: low/medium/high/max
# permission_mode = "plan"  # default/acceptEdits/plan/auto/bypassPermissions

[project]                   # optional: ground discussion in real code
# description = "Next.js e-commerce app"
# context_files = ["CLAUDE.md", "package.json"]
# add_dirs = ["../shared-lib"]
# mcp_config = ".claude/mcp.json"

[moderator]
role = """
Your moderator instructions here.
Must end with JSON: {"action":"continue","guidance":"..."} or {"action":"synthesize"}
"""

[[agents]]
name = "Backend Dev"
role = "Your role description"
color = "red"               # red/blue/green/yellow/magenta/cyan (default: white)
# model = "opus"            # per-agent model override
# effort = "high"           # per-agent effort
# max_turns = 3             # per-agent turn limit
# allowed_tools = ["Read", "Grep", "Glob"]
# disallowed_tools = ["Edit", "Write"]
```

## CLI Reference

```
roundtable [OPTIONS] [CONFIG]

Arguments:
  [CONFIG]                    TOML config file or built-in name (debate, code-review, microservices)

Options:
  -m, --model <MODEL>         Override all agent models
      --moderator-model       Override moderator model
  -r, --rounds <N>            Max rounds
      --max-budget-usd <N>    Cost limit (auto-synthesize when exceeded)
      --effort <LEVEL>        low/medium/high/max (Opus only)
      --fallback-model        Fallback on model overload
      --permission-mode       Permission mode (default/acceptEdits/plan/auto/bypassPermissions)
      --topic <TEXT>          Override config topic (or use without config for zero-config mode)
      --cwd <DIR>             Working directory for child processes
      --context-file <FILE>   Inject file contents into prompts (repeatable)
      --add-dir <DIR>         Extra dirs for claude (repeatable)
      --mcp-config <FILE>     MCP server config JSON
      --output-json           Machine-readable JSON output
      --transcript <FILE>     Custom transcript path
  -v, --verbose               Show claude stderr
  -h, --help                  Print help
```

## Output Formats

### `--output-json`

```json
{
  "topic": "Should we use microservices?",
  "total_cost_usd": 1.23,
  "transcript_path": "/tmp/roundtable/20260415-143000.jsonl",
  "rounds": 7,
  "synthesis": "## Conclusion\n...",
  "agents": ["Backend Dev", "DevOps", "PM", "Security"]
}
```

### Transcript JSONL

Each line in the transcript file:
```json
{"timestamp":"2026-04-15T14:30:00+03:00","agent":"Backend Dev","round":1,"message":"..."}
```

## Two Binaries

| Binary | Use Case |
|---|---|
| `roundtable` | N agents + moderator, TOML config or zero-config, convergence detection |
| `roundtable-chat` | Simple 2-agent debate, CLI args only |

### roundtable-chat

```bash
roundtable-chat "Monorepo vs multi-repo?" -r 3 -m haiku
roundtable-chat "TDD necessary?" -r 2 \
  --name-a "Pragmatist" --name-b "Idealist"
```

## Cost

Each round = (N agents + 1 moderator) Claude calls. Typical costs:

| Setup | Per Round | 3-Round Panel |
|---|---|---|
| 4 agents, haiku | ~$0.10 | ~$0.35 |
| 4 agents, sonnet | ~$0.50 | ~$1.80 |
| 4 agents, opus | ~$2.00 | ~$7.00 |

Use `--max-budget-usd` to cap spending. Budget exceeded = auto-synthesize.

## License

MIT
