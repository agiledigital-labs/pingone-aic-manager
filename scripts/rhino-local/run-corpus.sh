#!/usr/bin/env bash
# run-corpus.sh — evaluate every corpus file under each candidate Rhino
# language version and print a row × config table against AIC's observed
# outcome (from each file's header, sourced from
# docs/api/12-script-bindings-matrix.md).
#
# Usage:
#   scripts/rhino-local/run-corpus.sh
#   RHINO_LOCAL_VERSIONS=0,200 scripts/rhino-local/run-corpus.sh
#   scripts/rhino-local/run-corpus.sh --jsonl    # machine-readable lines
#
# Compiles and runs inside the AM docker image (see run-probe.sh). One
# container per invocation, not one per file.

set -euo pipefail

IMAGE="${RHINO_LOCAL_IMAGE:-us-docker.pkg.dev/forgeops-public/images/am:2026.3.1-2053}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
DEST="$ROOT/.rhino-local"
CORPUS="$HERE/corpus"
PROBE_SRC="$HERE/Probe.java"
PROBE_CLASS="$DEST/classes/Probe.class"
VERSIONS_CSV="${RHINO_LOCAL_VERSIONS:-0,180,200}"

JSONL=0
if [ "${1:-}" = "--jsonl" ]; then
  JSONL=1
elif [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
  echo "usage: $0 [--jsonl]" >&2
  exit 2
elif [ -n "${1:-}" ]; then
  echo "usage: $0 [--jsonl]" >&2
  exit 2
fi

"$HERE/fetch-jars.sh" >/dev/null

mkdir -p "$DEST/classes"
if [ ! -f "$PROBE_CLASS" ] || [ "$PROBE_SRC" -nt "$PROBE_CLASS" ]; then
  docker run --rm \
    --user "$(id -u):$(id -g)" \
    --entrypoint /opt/java/openjdk/bin/javac \
    -v "$ROOT:/work" \
    -w /work \
    "$IMAGE" \
    -encoding UTF-8 \
    -cp ".rhino-local/rhino-1.7.14.1.jar" \
    -d .rhino-local/classes \
    scripts/rhino-local/Probe.java
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "error: python3 is required to parse Probe JSON and print the table." >&2
  exit 1
fi

# One container, every (version, file) pair. Probe prints one JSON object
# per line; we prefix version and basename so the host formatter can join
# against the corpus headers. Results go to a temp file so the Python
# formatter (fed via heredoc) does not steal this pipe as stdin.
JSONL_FILE="$(mktemp -t rhino-local-corpus.XXXXXX)"
trap 'rm -f "$JSONL_FILE"' EXIT

VERSIONS_CSV="$VERSIONS_CSV" docker run --rm \
  --user "$(id -u):$(id -g)" \
  --entrypoint /bin/bash \
  -v "$ROOT:/work" \
  -w /work \
  -e VERSIONS_CSV \
  "$IMAGE" \
  -c '
set -euo pipefail
IFS=,
set -- $VERSIONS_CSV
unset IFS
JAVA=/opt/java/openjdk/bin/java
CP=".rhino-local/classes:.rhino-local/rhino-1.7.14.1.jar"
for ver in "$@"; do
  for f in scripts/rhino-local/corpus/*.js; do
    base=$(basename "$f" .js)
    printf "%s\t%s\t" "$ver" "$base"
    "$JAVA" -cp "$CP" Probe "$ver" "$f"
  done
done
' >"$JSONL_FILE"

python3 - "$JSONL_FILE" "$CORPUS" "$DEST/PROVENANCE.txt" "$VERSIONS_CSV" "$JSONL" "$IMAGE" <<'PY'
import json, re, sys
from pathlib import Path

jsonl_path = Path(sys.argv[1])
corpus_dir = Path(sys.argv[2])
provenance_path = Path(sys.argv[3])
versions = [v.strip() for v in sys.argv[4].split(",") if v.strip()]
jsonl_only = sys.argv[5] == "1"
image = sys.argv[6]

ROW_ORDER = [
    "json",
    "var",
    "let-any-scope",
    "const-in-function",
    "const-top-level",
    "const-in-loop-body",
    "const-in-loop-in-function",
    "const-dup-across-blocks",
    "const-in-for-init",
    "const-in-for-in",
    "const-in-for-of",
    "for-of-var",
    "object-shorthand",
    "object-destructuring",
    "default-parameters",
    "arrow-functions",
    "template-literals",
    "es2015-methods",
    "string-normalize",
    "map",
    "set",
    "weakmap",
    "weakset",
    "symbol",
    "promise",
    "proxy",
    "reflect",
]

VERSION_LABEL = {
    "0": "DEFAULT (0)",
    "170": "1.7 (170)",
    "180": "1.8 (180)",
    "200": "ES6 (200)",
}


def parse_header(path: Path) -> dict:
    meta = {
        "row": path.stem,
        "cite": "",
        "probed": "",
        "aic_compiled": "",
        "aic_evaluated": "",
        "aic_exception_contains": "",
        "aic_result": "",
        "aic_summary": "",
        "verdict": "check",
        "file": str(path),
    }
    in_header = False
    with path.open(encoding="utf-8") as fh:
        for line in fh:
            line = line.rstrip("\n")
            if not in_header:
                if line.startswith("// rhino-local-corpus"):
                    in_header = True
                    continue
                if line.startswith("//"):
                    continue
                break
            if not line.startswith("//"):
                break
            body = line[2:]
            if body.startswith(" "):
                body = body[1:]
            if body == "" or body.startswith(" "):
                continue
            if ":" not in body:
                continue
            key, _, value = body.partition(":")
            key = key.strip()
            value = value.lstrip(" ")
            field = {
                "row": "row",
                "cite": "cite",
                "probed": "probed",
                "aic-compiled": "aic_compiled",
                "aic-evaluated": "aic_evaluated",
                "aic-exception-contains": "aic_exception_contains",
                "aic-result": "aic_result",
                "aic-summary": "aic_summary",
                "verdict": "verdict",
            }.get(key)
            if field:
                meta[field] = value
    return meta


def strip_loc(msg: str) -> str:
    return re.sub(r" \([^)]+#\d+\)$", "", msg)


def result_as_string(probe: dict) -> str:
    kind = probe.get("resultKind")
    value = probe.get("result")
    if kind == "undefined":
        return "undefined"
    if kind == "missing":
        return ""
    if value is None:
        return "null"
    if kind == "number" and isinstance(value, float) and value.is_integer():
        return str(int(value))
    return str(value)


def local_outcome(probe: dict) -> str:
    if not probe.get("compiled"):
        return "parse: " + strip_loc(probe.get("exceptionMessage") or "")
    if not probe.get("evaluated"):
        return "throw: " + strip_loc(probe.get("exceptionMessage") or "")
    return "value: " + result_as_string(probe)


def matches(probe: dict, meta: dict):
    if meta["verdict"] != "check":
        return None
    if meta["aic_compiled"] != "":
        want = meta["aic_compiled"] == "true"
        if probe.get("compiled") is not want:
            return False
    if meta["aic_evaluated"] != "":
        want = meta["aic_evaluated"] == "true"
        if probe.get("evaluated") is not want:
            return False
    needle = meta["aic_exception_contains"]
    if needle:
        msg = probe.get("exceptionMessage") or ""
        if needle not in msg:
            return False
    if meta["aic_result"] != "":
        if result_as_string(probe) != meta["aic_result"]:
            return False
    return True


headers = {}
for path in sorted(corpus_dir.glob("*.js")):
    meta = parse_header(path)
    headers[meta["row"]] = meta

probes = {}  # (row, version) -> probe json
impl = None
for raw in jsonl_path.read_text(encoding="utf-8").splitlines():
    raw = raw.rstrip("\n")
    if not raw:
        continue
    ver, row, payload = raw.split("\t", 2)
    probe = json.loads(payload)
    probes[(row, ver)] = probe
    if impl is None:
        impl = probe.get("implementationVersion")
    if jsonl_only:
        rec = {"row": row, "languageVersion": int(ver) if ver.isdigit() else ver, "probe": probe}
        rec.update({k: headers.get(row, {}).get(k) for k in (
            "cite", "probed", "aic_summary", "aic_compiled", "aic_evaluated",
            "aic_exception_contains", "aic_result", "verdict")})
        rec["match"] = matches(probe, headers[row]) if row in headers else None
        print(json.dumps(rec, ensure_ascii=False))

if jsonl_only:
    sys.exit(0)

rows = [r for r in ROW_ORDER if r in headers]
for r in sorted(headers):
    if r not in rows:
        rows.append(r)

prov = provenance_path.read_text(encoding="utf-8") if provenance_path.is_file() else ""
digest = ""
for line in prov.splitlines():
    if line.startswith("repoDigest:"):
        digest = line.split(":", 1)[1].strip()
        break

labels = [VERSION_LABEL.get(v, v) for v in versions]

print(f"Rhino: {impl or '(unknown)'}")
print(f"image: {image}")
if digest:
    print(f"digest: {digest}")
print("AIC column: docs/api/12-script-bindings-matrix.md (probe dates per row)")
print(f"configs: {', '.join(labels)}")
print()

col_w = [28, 42] + [36] * len(versions)
headers_row = ["row", "AIC"] + labels


def fmt_row(cells, widths):
    parts = []
    for cell, width in zip(cells, widths):
        parts.append(f"{cell:<{width}}")
    return "  ".join(parts)


print(fmt_row(headers_row, col_w))
print(fmt_row(["-" * min(w, len(h) + 4) for h, w in zip(headers_row, col_w)], col_w))

match_counts = {v: 0 for v in versions}
checked = 0
none_match = []

for row in rows:
    meta = headers[row]
    aic = meta["aic_summary"] or "(no aic-summary)"
    cells = [row, aic]
    row_matches = []
    for ver in versions:
        probe = probes.get((row, ver))
        if probe is None:
            cells.append("MISSING")
            row_matches.append(False)
            continue
        outcome = local_outcome(probe)
        m = matches(probe, meta)
        if m is True:
            cells.append(outcome + "  MATCH")
            match_counts[ver] += 1
            row_matches.append(True)
        elif m is False:
            cells.append(outcome + "  !=")
            row_matches.append(False)
        else:
            cells.append(outcome + "  obs")
            row_matches.append(None)
    if meta["verdict"] == "check":
        checked += 1
        if row_matches and all(m is False for m in row_matches):
            local_by_ver = []
            for ver in versions:
                probe = probes.get((row, ver))
                local_by_ver.append(
                    f"{VERSION_LABEL.get(ver, ver)}: {local_outcome(probe) if probe else 'MISSING'}"
                )
            none_match.append((row, aic, local_by_ver, meta))
    print(fmt_row(cells, col_w))

print()
print("Summary")
print(f"  checked rows: {checked}")
closest = None
closest_n = -1
for ver in versions:
    n = match_counts[ver]
    print(f"  {VERSION_LABEL.get(ver, ver)} matches AIC: {n}/{checked}")
    if n > closest_n:
        closest = ver
        closest_n = n
if closest is not None:
    print(f"  closest config: {VERSION_LABEL.get(closest, closest)}")

print()
print("Rows where no local config reproduces AIC:")
if not none_match:
    print("  (none)")
else:
    for row, aic, local_by_ver, meta in none_match:
        print(f"  * {row}")
        print(f"      AIC:   {aic}")
        print(f"      cite:  {meta['cite']}")
        for line in local_by_ver:
            print(f"      local: {line}")
PY
