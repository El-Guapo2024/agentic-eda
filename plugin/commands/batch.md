---
description: Launch several hard-gated solve runs of an intent in parallel (seeds × placers)
argument-hint: [intent.yaml] [--seeds 0,1,2,3] [--placers cypress,anneal] [--jobs 4]
---
Run `${CLAUDE_PLUGIN_ROOT}/bin/eda-batch $ARGUMENTS` (default intent `intent.yaml`, output directory `runs/<date>-<n>`). Poll the output file inline until it finishes; do not background it and return. Then summarise: which runs passed, at which ladder rung (read each run's strategy.txt), wall time, and the first named gate failure for every run that did not pass. Never soften a failure or retry with different settings on your own; a failed run is a finding for the user.
