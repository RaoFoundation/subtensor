#!/bin/bash
# This file patches the code in the repository to create a docker image with the ability to run tests in non-fast-runtime
# mode.

set -e

# Function to check for a pattern and apply a replacement
# Args: file_path, search_pattern, replacement_pattern, description
patch_file() {
  local file_path="$1"
  local search_pattern="$2"
  local replacement_pattern="$3"
  local description="$4"

  # Check if the search pattern exists
  if ! grep -qF "$search_pattern" "$file_path" 2>/dev/null && ! grep -qP "$search_pattern" "$file_path" 2>/dev/null; then
    echo "Error: Target pattern '$search_pattern' not found in $file_path"
    echo "Description: $description"
    echo "This may indicate the codebase has changed. Please verify the target code exists."
    exit 1
  fi

  local before_hash after_hash
  before_hash=$(cksum "$file_path")

  # Apply the replacement
  if ! perl -0777 -i -pe "$replacement_pattern" "$file_path"; then
    echo "Error: Failed to apply replacement in $file_path"
    echo "Description: $description"
    exit 1
  fi

  # The search pattern existing is not enough: the replacement regex must
  # actually match, otherwise the patch silently no-ops when the code drifts.
  after_hash=$(cksum "$file_path")
  if [ "$before_hash" = "$after_hash" ]; then
    echo "Error: Replacement pattern did not change $file_path"
    echo "Description: $description"
    echo "The replacement regex no longer matches the code. Update or remove this patch."
    exit 1
  fi
}

echo "Applying patches..."

# NOTE: the former Patch 1 (InitialStartCallDelay) was removed: mainline now
# hardcodes `pub const InitialStartCallDelay: u64 = 0;` which already gives
# local testing an immediate start_call.

# Patch 2: SetChildren rate limit
patch_file \
  "pallets/subtensor/src/utils/rate_limiting.rs" \
  "Self::SetChildren => 150, // 30 minutes" \
  's|Self::SetChildren => 150, // 30 minutes|Self::SetChildren => 15, // 3 min|' \
  "Reduce SetChildren rate limit for local testing"

echo "✓ All patches applied successfully."
