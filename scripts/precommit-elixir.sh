#!/usr/bin/env bash
# Check Elixir formatting in the web/ app. Used by .pre-commit-config.yaml and
# runnable on its own. mix format reads web/.formatter.exs, so we cd there.
set -euo pipefail
cd "$(dirname "$0")/../web"
exec mix format --check-formatted
