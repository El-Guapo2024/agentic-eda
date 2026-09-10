---
description: Turn a short board description into an agentic-eda intent YAML and show it back for correction
argument-hint: <what the board is, in a sentence or two>
---
The user described a board: $ARGUMENTS

Follow the agentic-eda skill. Draft `intent.yaml` in the current directory from the closest template under `${CLAUDE_PLUGIN_ROOT}/../examples/` (or `/opt/agentic-eda/examples` inside the container, list it with `${CLAUDE_PLUGIN_ROOT}/bin/eda --help` if unsure). Ask only for what the checklist in the skill says you cannot guess. Then run `${CLAUDE_PLUGIN_ROOT}/bin/eda lint intent.yaml` and show the user a short spec: parts, nets, outline, layers allowed, placers allowed. Do not launch runs yet.
