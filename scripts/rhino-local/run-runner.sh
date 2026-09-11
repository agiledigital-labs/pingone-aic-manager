#!/usr/bin/env bash
# run-runner.sh — compile the long-lived JSON runner and exec it inside the AM
# image with stdin/stdout attached. The Node client spawns this script.
#
# Usage:
#   scripts/rhino-local/run-runner.sh
#
# Protocol: one JSON object per stdin line, one JSON object per stdout line.
# Runner diagnostics go to stderr.

set -euo pipefail

IMAGE="${RHINO_LOCAL_IMAGE:-us-docker.pkg.dev/forgeops-public/images/am:2026.3.1-2053}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  echo "usage: $0" >&2
  exit 2
fi

"$HERE/compile-java.sh"

# Optional container name so the Node client can docker rm -f on a crash
# rather than leaking a JVM whose docker CLI it just killed.
name_args=()
if [ -n "${RHINO_LOCAL_CONTAINER:-}" ]; then
  name_args=(--name "$RHINO_LOCAL_CONTAINER")
fi

exec docker run --rm -i \
  "${name_args[@]}" \
  --user "$(id -u):$(id -g)" \
  --entrypoint /opt/java/openjdk/bin/java \
  -v "$ROOT:/work" \
  -w /work \
  "$IMAGE" \
  -cp ".rhino-local/classes:.rhino-local/rhino-1.7.14.1.jar" \
  Runner
