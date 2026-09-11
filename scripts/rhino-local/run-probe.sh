#!/usr/bin/env bash
# run-probe.sh — compile the rhino-local Java sources inside the AM image
# and evaluate one source file under a chosen Rhino language version.
#
# Usage:
#   scripts/rhino-local/run-probe.sh <languageVersion> <source.js>
#
# languageVersion is a Rhino Context version integer (0 = VERSION_DEFAULT,
# 180 = VERSION_1_8, 200 = VERSION_ES6) or one of DEFAULT, 1.7, 1.8, ES6.
#
# Compiles and runs inside the AM docker image: this machine has no host JDK,
# and the image's Temurin 25.0.4 is the same runtime AM itself uses. Prints
# Probe's single JSON object on stdout.

set -euo pipefail

IMAGE="${RHINO_LOCAL_IMAGE:-us-docker.pkg.dev/forgeops-public/images/am:2026.3.1-2053}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ] || [ "$#" -ne 2 ]; then
  echo "usage: $0 <languageVersion> <source.js>" >&2
  exit 2
fi

VERSION="$1"
SOURCE="$2"

"$HERE/compile-java.sh"

if [ ! -f "$SOURCE" ]; then
  echo "error: source file not found: $SOURCE" >&2
  exit 1
fi

# Resolve the source to a path inside the bind mount. Probe reads the file
# itself, so it has to be visible at /work/<rel>.
SOURCE_ABS="$(cd "$(dirname "$SOURCE")" && pwd)/$(basename "$SOURCE")"
case "$SOURCE_ABS" in
  "$ROOT"/*) SOURCE_REL="${SOURCE_ABS#"$ROOT"/}" ;;
  *)
    echo "error: source file must live inside the repo (bind-mounted at /work): $SOURCE" >&2
    exit 1
    ;;
esac

docker run --rm \
  --user "$(id -u):$(id -g)" \
  --entrypoint /opt/java/openjdk/bin/java \
  -v "$ROOT:/work" \
  -w /work \
  "$IMAGE" \
  -cp ".rhino-local/classes:.rhino-local/rhino-1.7.14.1.jar" \
  Probe \
  "$VERSION" \
  "$SOURCE_REL"
