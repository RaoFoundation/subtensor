#!/bin/zsh
# update-deps-to-path.sh goal is to replace git dependencies with path dependencies
# in the target Cargo.toml file.
#
# The script will scan the source repos for packages and build a mapping of package name to path.
# It will then process the target Cargo.toml file line by line and replace git dependencies with path dependencies.
#
# The script will output the new Cargo.toml file to stdout.
#
# The script will exit with a non-zero status if the target Cargo.toml file is not found.
# Usage: ./scripts/update-deps-to-path.sh ./Cargo.toml ../polkadot-sdk-otf  > Cargo.toml.new

set -e

TARGET_TOML="${1:?Usage: $0 <target-cargo-toml> <source-repo-1> [source-repo-2] ...}"
shift

if [[ $# -eq 0 ]]; then
    echo "Error: At least one source repo path required" >&2
    exit 1
fi

# Build package name -> path mapping from all source repos
typeset -A PKG_PATHS

for SOURCE_PATH in "$@"; do
    SOURCE_PATH="$(cd "$SOURCE_PATH" && pwd)"
    echo "Scanning $SOURCE_PATH for packages..." >&2
    
    for cargo_toml in $(find "$SOURCE_PATH" -name "Cargo.toml" -type f 2>/dev/null); do
        pkg_name=$(yq -p toml -o yaml '.package.name // ""' "$cargo_toml" 2>/dev/null | tr -d '"')
        
        if [[ -n "$pkg_name" && "$pkg_name" != "null" ]]; then
            pkg_dir="$(dirname "$cargo_toml")"
            PKG_PATHS[$pkg_name]="$pkg_dir"
            echo "  Found: $pkg_name" >&2
        fi
    done
done

echo "Found ${#PKG_PATHS[@]} total packages" >&2
echo "" >&2

# Process target Cargo.toml line by line
echo "Updating dependencies in $TARGET_TOML..." >&2

while IFS= read -r line; do
    # Check if this line has a git dependency
    if [[ "$line" =~ ^([a-zA-Z0-9_-]+|\"[^\"]+\")\ *=\ *\{.*git\ *=\ *\" ]]; then
        # Extract package name (handle both quoted and unquoted)
        dep_name=$(echo "$line" | sed -E 's/^"?([a-zA-Z0-9_-]+)"? *=.*/\1/')
        
        # Check for package alias
        if [[ "$line" =~ package\ *=\ *\"([^\"]+)\" ]]; then
            lookup_name="${match[1]}"
        else
            lookup_name="$dep_name"
        fi
        
        # Check if we have this package
        if [[ -n "${PKG_PATHS[$lookup_name]}" ]]; then
            pkg_path="${PKG_PATHS[$lookup_name]}"
            echo "  $dep_name -> $pkg_path" >&2
            
            # Extract features/default-features/package if present
            extras=""
            if [[ "$line" =~ default-features\ *=\ *false ]]; then
                extras="$extras, default-features = false"
            fi
            if [[ "$line" =~ package\ *=\ *\"([^\"]+)\" ]]; then
                extras="$extras, package = \"${match[1]}\""
            fi
            if [[ "$line" =~ features\ *=\ *\[([^\]]*)\] ]]; then
                extras="$extras, features = [${match[1]}]"
            fi
            
            # Output new line with just path
            echo "${dep_name} = { path = \"${pkg_path}\"${extras} }"
        else
            # Package not found in sources, keep original
            echo "$line"
        fi
    else
        # Not a git dependency, keep as-is
        echo "$line"
    fi
done < "$TARGET_TOML"

echo "" >&2
echo "Done!" >&2
