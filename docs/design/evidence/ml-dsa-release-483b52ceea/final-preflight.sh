#!/bin/bash
export PATH="/tmp/ms-reference-39060946210/bin:$PATH"
export PREFLIGHT_PUSH_ACTOR=UnArbosSix
scripts/preflight.sh > /tmp/ms-release-483b52ceea/preflight-final.log 2>&1
status=$?
printf '%s\n' "$status" > /tmp/ms-release-483b52ceea/preflight-final.exit
exit "$status"
