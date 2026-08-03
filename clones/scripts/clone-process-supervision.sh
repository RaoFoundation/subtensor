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

wait_for_monitor_ready() {
  local monitor_pid=$1 ready_file=$2 timeout_seconds=${3:-60}
  local elapsed_tenths=0

  while (( elapsed_tenths < timeout_seconds * 10 )); do
    [[ -s "$ready_file" ]] && return 0
    if ! kill -0 "$monitor_pid" 2>/dev/null; then
      local status=0
      wait "$monitor_pid" || status=$?
      if (( status == 0 )); then
        echo "clone block monitor exited before becoming ready" >&2
        return 1
      fi
      return "$status"
    fi
    sleep 0.1
    elapsed_tenths=$((elapsed_tenths + 1))
  done

  echo "clone block monitor did not become ready within ${timeout_seconds}s" >&2
  return 1
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
