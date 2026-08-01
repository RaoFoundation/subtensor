#!/usr/bin/env bash

# Shared by clone jobs that run a block monitor alongside a workload. The
# monitor launcher execs Node, so SIGTERM reaches the process that owns the RPC
# subscription and lets it flush its report before exiting.

terminate_process_tree() {
  local pid=$1 child
  while IFS= read -r child; do
    [[ -n "$child" ]] || continue
    terminate_process_tree "$child"
  done < <(pgrep -P "$pid" 2>/dev/null || true)
  kill -TERM "$pid" 2>/dev/null || true
}

supervise_monitor_and_workload() {
  local monitor_pid=$1 workload_pid=$2 description=$3
  local monitor_ended_early=false monitor_status=0 workload_status=0

  while kill -0 "$workload_pid" 2>/dev/null; do
    if ! kill -0 "$monitor_pid" 2>/dev/null; then
      monitor_ended_early=true
      terminate_process_tree "$workload_pid"
      break
    fi
    sleep 1
  done

  wait "$workload_pid" || workload_status=$?
  if kill -0 "$monitor_pid" 2>/dev/null; then
    kill -TERM "$monitor_pid" 2>/dev/null || true
  fi
  wait "$monitor_pid" || monitor_status=$?

  if [[ "$monitor_ended_early" == true ]]; then
    if (( monitor_status == 0 )); then
      echo "clone block monitor exited before $description completed" >&2
      return 1
    fi
    return "$monitor_status"
  fi
  if (( workload_status != 0 )); then
    return "$workload_status"
  fi
  return "$monitor_status"
}
