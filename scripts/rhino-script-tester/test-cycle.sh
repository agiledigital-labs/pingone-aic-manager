#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT_SOURCE="${1:-$SCRIPT_DIR/scripts/rhino-let-behaviour.script.js}"

"$SCRIPT_DIR/update-script.sh" "$SCRIPT_SOURCE"
"$SCRIPT_DIR/run-journey.sh"
