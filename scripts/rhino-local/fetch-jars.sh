#!/usr/bin/env bash
# fetch-jars.sh — copy the AM Rhino jars this harness needs out of the local
# docker image into gitignored .rhino-local/ at the repo root.
#
# Idempotent: no-op when the jars and PROVENANCE.txt already match the image
# that is currently present. Does **not** pull the image — if it is absent,
# exits 1 with a message naming it rather than fetching ~850MB.
#
# Usage:
#   scripts/rhino-local/fetch-jars.sh
#
# Override the image with RHINO_LOCAL_IMAGE if you are calibrating a different
# tag; the default is the AM 8.1.1 image this slice measured.

set -euo pipefail

IMAGE="${RHINO_LOCAL_IMAGE:-us-docker.pkg.dev/forgeops-public/images/am:2026.3.1-2053}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
DEST="$ROOT/.rhino-local"
AM_LIB="/usr/local/tomcat/webapps/am/WEB-INF/lib"

JARS=(
  rhino-1.7.14.1.jar
  openam-scripting-8.1.1.jar
)

die() {
  echo "error: $*" >&2
  exit 1
}

if ! command -v docker >/dev/null 2>&1; then
  die "docker is not on PATH. This harness runs javac/java inside the AM image."
fi

if ! docker info >/dev/null 2>&1; then
  die "docker is installed but the daemon is not reachable."
fi

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  cat >&2 <<EOF
error: docker image is not present locally:
  $IMAGE

This script will not pull it (~850MB). Load or pull the image, then re-run:
  docker pull $IMAGE
EOF
  exit 1
fi

# Repo digest is the content-addressed reference; fall back to the image id
# if the image was loaded from a tarball and has no RepoDigests.
repo_digest="$(docker image inspect --format '{{if .RepoDigests}}{{index .RepoDigests 0}}{{end}}' "$IMAGE")"
image_id="$(docker image inspect --format '{{.Id}}' "$IMAGE")"
if [ -z "$repo_digest" ]; then
  repo_digest="$image_id"
fi

already_present=1
if [ ! -f "$DEST/PROVENANCE.txt" ]; then
  already_present=0
else
  recorded="$(awk -F': ' '/^repoDigest:/{print $2; exit}' "$DEST/PROVENANCE.txt")"
  [ "$recorded" = "$repo_digest" ] || already_present=0
fi
for jar in "${JARS[@]}"; do
  [ -f "$DEST/$jar" ] || already_present=0
done

if [ "$already_present" -eq 1 ]; then
  echo "already present ($repo_digest)"
  exit 0
fi

mkdir -p "$DEST"

cid="$(docker create "$IMAGE")"
cleanup() { docker rm -f "$cid" >/dev/null 2>&1 || true; }
trap cleanup EXIT

for jar in "${JARS[@]}"; do
  src="$AM_LIB/$jar"
  if ! docker cp "$cid:$src" "$DEST/$jar" 2>/dev/null; then
    die "image $IMAGE does not contain $src"
  fi
done

{
  echo "image: $IMAGE"
  echo "repoDigest: $repo_digest"
  echo "imageId: $image_id"
  echo "extractedAt: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "jars:"
  for jar in "${JARS[@]}"; do
    size="$(wc -c <"$DEST/$jar" | tr -d ' ')"
    sum="$(sha256sum "$DEST/$jar" | awk '{print $1}')"
    echo "  $jar  sha256:$sum  $size bytes"
  done
} >"$DEST/PROVENANCE.txt"

echo "extracted ${#JARS[@]} jars to $DEST"
echo "  $repo_digest"
for jar in "${JARS[@]}"; do
  echo "  $jar"
done
