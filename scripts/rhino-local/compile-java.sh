#!/usr/bin/env bash
# compile-java.sh — javac the rhino-local Java sources inside the AM image.
#
# Idempotent: no-op when every .class is newer than its .java. Called by
# run-probe.sh, run-corpus.sh, and run-runner.sh.

set -euo pipefail

IMAGE="${RHINO_LOCAL_IMAGE:-us-docker.pkg.dev/forgeops-public/images/am:2026.3.1-2053}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
DEST="$ROOT/.rhino-local"
CLASSES="$DEST/classes"

"$HERE/fetch-jars.sh" >/dev/null

mkdir -p "$CLASSES"

need=0
shopt -s nullglob
sources=("$HERE"/*.java)
if [ "${#sources[@]}" -eq 0 ]; then
  echo "error: no Java sources in $HERE" >&2
  exit 1
fi
for src in "${sources[@]}"; do
  class="$CLASSES/$(basename "$src" .java).class"
  if [ ! -f "$class" ] || [ "$src" -nt "$class" ]; then
    need=1
    break
  fi
done

if [ "$need" -eq 0 ]; then
  exit 0
fi

rel_sources=()
for src in "${sources[@]}"; do
  rel_sources+=("scripts/rhino-local/$(basename "$src")")
done

docker run --rm \
  --user "$(id -u):$(id -g)" \
  --entrypoint /opt/java/openjdk/bin/javac \
  -v "$ROOT:/work" \
  -w /work \
  "$IMAGE" \
  -encoding UTF-8 \
  -cp ".rhino-local/rhino-1.7.14.1.jar" \
  -d .rhino-local/classes \
  "${rel_sources[@]}"
