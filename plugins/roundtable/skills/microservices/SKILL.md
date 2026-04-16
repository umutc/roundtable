---
description: Run a multi-agent architecture debate about microservices design decisions using roundtable.
---

# Roundtable Microservices Panel

Spawn an architecture debate panel on: **$ARGUMENTS**

Run:

```bash
npx -y roundtable@latest microservices --topic "$ARGUMENTS" --output-json
```

Parse the final JSON and present the `synthesis` as the architectural recommendation.
