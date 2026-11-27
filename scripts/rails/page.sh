#!/usr/bin/env bash
# Serves the swap page of the bittensor.com app against the local rig.
#
#   page.sh          the development server, at http://localhost:3000/swap
#   page.sh build    a production build with the rig's addresses
#
# The page has no backend: the rig's addresses reach it as build-time settings, from
# .rails/page.env. Nothing is written into the website's directory.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

[[ -f "$RAILS_STATE/page.env" ]] || die "the rig is not up: run \`just rails-up\` first"
set -a
source "$RAILS_STATE/page.env"
set +a

cd "$RAILS_REPO/website"
case "${1:-dev}" in
    dev) exec corepack yarn workspace @raofoundation/bittensor-website dev ;;
    build) exec corepack yarn turbo run build --filter=@raofoundation/bittensor-website ;;
    *) die "usage: page.sh [dev|build]" ;;
esac
