---
description: Run a multi-agent code review panel (security, performance, readability reviewers + moderator) on the current project using roundtable.
---

# Roundtable Code Review

Spawn a multi-disciplinary code review panel on the current working directory.

Arguments: **$ARGUMENTS** (optional — specific file or scope to review; if empty, review recent changes)

Run:

```bash
npx -y roundtable@latest code-review --cwd "$(pwd)" --topic "$ARGUMENTS" --output-json
```

Parse the final JSON line and present the `synthesis` to the user. Flag any critical findings prominently.
