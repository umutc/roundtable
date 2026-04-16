---
description: Run a multi-agent debate on a topic using roundtable. Use when the user wants multiple perspectives, pros/cons analysis, or adversarial review of an idea.
---

# Roundtable Debate

Spawn a multi-agent Claude Code debate on the topic: **$ARGUMENTS**

Run this command using the Bash tool, streaming output to the user:

```bash
npx -y roundtable@latest debate --topic "$ARGUMENTS" --output-json
```

Parse the final JSON (last line) to extract the `synthesis` field. Present the synthesis to the user as the debate conclusion, then offer to show the full transcript if they want detail.

If `npx` is missing, tell the user to install Node.js 16+ first.
