#!/usr/bin/env bash
# Reject tenant- and client-identifying metadata before it enters the repo.
#
# Detects by SHAPE, not by a list of previously-leaked values: the point is to
# catch the NEXT client's hostname or Azure tenant GUID, not just the last
# one. Literal client names are deliberately NOT in this file — committing a
# denylist of client names would itself commit the client names. Point
# SENSITIVE_DENYLIST at a file outside the repo (or at the gitignored
# .ai/denylist.txt) to add literal patterns; see .ai/local.md.example.
#
# Modes:
#   --staged     added lines of the staged diff        (pre-commit hook)
#   --tracked    every tracked file in the work tree   (default; CI)
#   --history [REV-RANGE]
#                every blob introduced by the range    (audit/CI)
#                defaults to every reachable commit
#   --selftest   prove the rules still fire            (CI runs this first)
#   --redact     filter stdin -> stdout, sanitised    (capture-time)
#                REDACT_VALUES=1 additionally strips ESV valueBase64 payloads
#   --fix        rewrite tracked files in place       (same redactions)
#
# Exit 0 clean, 1 findings, 2 usage/internal error.
set -uo pipefail

MODE=tracked
HISTORY_RANGE=--all
case "${1:-}" in
  --staged) MODE=staged ;;
  --tracked | "") MODE=tracked ;;
  --history)
    MODE=history
    HISTORY_RANGE="${2:---all}"
    ;;
  --selftest) MODE=selftest ;;
  --redact) MODE=redact ;;
  --fix) MODE=fix ;;
  -h | --help)
    sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
    exit 0
    ;;
  *)
    echo "usage: $0 [--staged|--tracked|--history [REV-RANGE]|--selftest|--redact|--fix]" >&2
    exit 2
    ;;
esac

cd "$(git rev-parse --show-toplevel)" || exit 2

# This script necessarily contains the very patterns it hunts for, so a
# --tracked run would flag itself. Same trap as a monitor whose command line
# matches its own pgrep. Excluded by path, deliberately and visibly.
SELF="scripts/check-sensitive-metadata.sh"

# Placeholder vocabulary. Anything matching these is a sanctioned stand-in;
# everything else of the same shape is a finding. Keep in sync with the list
# in .ai/core.md so a developer fixing a violation knows what to write.
PLACEHOLDER_HOST='(^|[./@])(example\.(com|org|net)|localhost|tenant\.example\.com)$|<[^>]+>|\{[^}]+\}|your-tenant|placeholder'
PLACEHOLDER_UUID='^0{8}-0{4}-0{4}-0{4}-0{12}$'
# Reserved tenant labels — the sanctioned stand-ins for a real AIC tenant.
PLACEHOLDER_TENANT='^(<[^>]+>|\{[^}]+\}|example|tenant|your-tenant|my-?tenant|placeholder|openam-mytenant-(sndbx|dev|uat|prod))\.forgeblocks\.com$'
UUID_RE='[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}'

# Literal deny patterns, compiled once. The file lives OUTSIDE the repo (or in
# the gitignored .ai/denylist.txt): committing a list of client names would
# commit the client names. One pattern per line, '#' comments ignored.
DENY_RE=""
if [ -n "${SENSITIVE_DENYLIST_CONTENT:-}" ]; then
  DENY_RE=$(printf '%s\n' "$SENSITIVE_DENYLIST_CONTENT" | grep -vE '^\s*(#|$)' | paste -sd '|' -)
elif [ -n "${SENSITIVE_DENYLIST:-}" ] && [ -f "${SENSITIVE_DENYLIST}" ]; then
  DENY_RE=$(grep -vE '^\s*(#|$)' "${SENSITIVE_DENYLIST}" | paste -sd '|' -)
fi
if [ "${REQUIRE_SENSITIVE_DENYLIST:-0}" = 1 ] && [ -z "$DENY_RE" ]; then
  echo "error: a non-empty sensitive metadata denylist is required" >&2
  exit 2
fi

# Cheap pre-filter: only lines that could possibly match a rule are worth the
# per-line analysis. Without this a full-tree scan forks greps for every line
# of every file and takes minutes.
CANDIDATE_RE='forgeblocks|sts\.windows\.net|entityid|trustedproviders|metaalias|SAML2MetaCache|setToPrototolMap|assertionConsumerService|singleLogoutService|nameIdService|AuthConsumer|\|saml2|aHR0c'
if [ -n "$DENY_RE" ]; then
  CANDIDATE_RE="$CANDIDATE_RE|$DENY_RE"
fi

# Shape-preserving redaction. Every substitution keeps the STRUCTURE of what it
# replaces — a host stays a host, a GUID stays a same-shaped GUID — because
# docs/api/ claims are about observed shapes, and a redaction that destroyed
# them would make the evidence useless. Distinct SAML hosts map to distinct
# placeholders (sp-a, sp-b, …) so a document that contrasts two entities still
# contrasts two entities.
redact_stream() {
  # Two passes over buffered input: pass 1 notes which sp-<letter> placeholders
  # the text ALREADY uses, so a newly-assigned one cannot collide with a
  # different entity that is already called sp-b. Then base64url blobs, which
  # awk cannot decode, are handled in shell.
  awk '
    # These two MUST mirror $PLACEHOLDER_TENANT and $PLACEHOLDER_HOST in the
    # shell above. When they drift, --fix rewrites files the checker calls
    # clean, which is exactly how this went wrong the first time: the tenant
    # predicate is the looser of the two (any host containing "example"), and
    # a single shared predicate silently redacted example.forgeblocks.com.
    function is_placeholder_tenant(h,   l) {
      l = tolower(h)
      return l ~ /^<[^>]+>\.forgeblocks\.com$/ ||
             l ~ /^\{[^}]+\}\.forgeblocks\.com$/ ||
             l ~ /^(example|tenant|your-tenant|my-?tenant|placeholder)\.forgeblocks\.com$/ ||
             l ~ /^openam-mytenant-(sndbx|dev|uat|prod)\.forgeblocks\.com$/
    }
    function is_placeholder_host(h,   l) {
      l = tolower(h)
      return l ~ /(^|[.\/@])(example\.(com|org|net)|localhost)$/ ||
             l ~ /<[^>]+>/ || l ~ /\{[^}]+\}/ ||
             l ~ /your-tenant/ || l ~ /placeholder/
    }
    function next_placeholder(   c) {
      do { c = sprintf("sp-%c.example.com", 97 + n); n++ } while (c in taken)
      taken[c] = 1
      return c
    }
    { buf[NR] = $0 }
    /sp-[a-z]\.example\.com/ {
      t = $0
      while (match(t, /sp-[a-z]\.example\.com/)) {
        taken[substr(t, RSTART, RLENGTH)] = 1
        t = substr(t, RSTART + RLENGTH)
      }
    }
    END {
      for (i = 1; i <= NR; i++) {
        line = buf[i]

        while (match(line, /[A-Za-z0-9._-]+\.forgeblocks\.com/)) {
          host = substr(line, RSTART, RLENGTH)
          if (is_placeholder_tenant(host)) break
          line = substr(line, 1, RSTART - 1) "<your-tenant>.forgeblocks.com" \
                 substr(line, RSTART + RLENGTH)
        }

        # Capture-time only (REDACT_VALUES=1). An ESV value is not
        # client-identifying metadata, so it is no check rule and --fix must
        # leave it alone: docs/api/03-esvs.md has deliberate example values
        # that rewriting would destroy. A live body carries real ones though,
        # and a live body is what gets pasted, so capture strips them.
        # (No apostrophes in here: the awk program is single-quoted.)
        if (ENVIRON["REDACT_VALUES"] == "1") {
          gsub(/"valueBase64"[[:space:]]*:[[:space:]]*"[^"]*"/,
               "\"valueBase64\": \"<base64-value>\"", line)
        }

        # Mirrors $PLACEHOLDER_UUID: the all-zeros GUID is a sanctioned
        # placeholder, so a blind gsub here rewrote a doc the checker accepts.
        pos = 1
        while (match(substr(line, pos), /sts\.windows\.net\/[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}/)) {
          st = pos + RSTART - 1
          hit = substr(line, st, RLENGTH)
          guid = hit; sub(/^sts\.windows\.net\//, "", guid)
          if (guid ~ /^0{8}-0{4}-0{4}-0{4}-0{12}$/) {
            pos = st + RLENGTH
          } else {
            line = substr(line, 1, st - 1) "sts.windows.net/<tenant-guid>" substr(line, st + RLENGTH)
            pos = st + length("sts.windows.net/<tenant-guid>")
          }
        }

        if (line ~ /entityId|entityid|trustedProviders|metaAlias|SAML2MetaCache|setToPrototolMap|assertionConsumerService|singleLogoutService|nameIdService|AuthConsumer|\|saml2/) {
          out = ""; rest = line
          while (match(rest, /https?:\/\/[A-Za-z0-9<>{}._-]+/)) {
            pre = substr(rest, 1, RSTART - 1)
            url = substr(rest, RSTART, RLENGTH)
            rest = substr(rest, RSTART + RLENGTH)
            scheme = url; sub(/:\/\/.*/, "://", scheme)
            host = url; sub(/^https?:\/\//, "", host)
            if (!is_placeholder_host(host) && host !~ /forgeblocks\.com$/ && host != "sts.windows.net") {
              if (!(host in seen)) seen[host] = next_placeholder()
              url = scheme seen[host]
            }
            out = out pre url
          }
          line = out rest
        }
        print line
      }
    }
  ' | {
    # base64url that decodes to a real URL -> <entityId64>. Same decode the
    # checker uses, so the two cannot disagree about what counts as encoded.
    while IFS= read -r line; do
      for cand in $(grep -oE 'aHR0c[A-Za-z0-9_-]{11,}' <<<"$line" 2>/dev/null); do
        decoded=$(printf '%s' "$cand" | tr '_-' '/+' | base64 -d 2>/dev/null | tr -d '\0')
        grep -qiE '^https?://' <<<"$decoded" || continue
        host=$(sed -E 's|https?://||; s|[/?#].*||' <<<"$decoded")
        printf '%s' "$host" | grep -qiE "$PLACEHOLDER_HOST" && continue
        line=${line//"$cand"/<entityId64>}
      done
      printf '%s\n' "$line"
    done
  }
}

findings=0

# report FILE LINE RULE DETAIL REMEDY
report() {
  findings=$((findings + 1))
  printf '\n%s:%s\n  rule:   %s\n  found:  %s\n  fix:    %s\n' "$1" "$2" "$3" "$4" "$5"
}

is_placeholder_host() {
  printf '%s' "$1" | grep -qiE "$PLACEHOLDER_HOST"
}

# scan_text FILE  — reads candidate text on stdin as "LINENO:CONTENT"
scan_text() {
  local file="$1" line content host uuid cand decoded
  while IFS= read -r rec; do
    line=${rec%%:*}
    content=${rec#*:}

    # Rule 1 — AIC tenant hostname. `openam-<tenant>.forgeblocks.com` names a
    # real tenant; only a bracketed placeholder is allowed.
    while read -r host; do
      [ -n "$host" ] || continue
      grep -qiE "$PLACEHOLDER_TENANT" <<<"$host" && continue
      report "$file" "$line" "aic-tenant-hostname" "$host" \
        "use <your-tenant>.forgeblocks.com"
    done < <(grep -oiE '[a-z0-9<>{}._-]*\.forgeblocks\.com' <<<"$content")

    # Rule 2 — Azure AD tenant GUID in an STS issuer.
    while read -r uuid; do
      [ -n "$uuid" ] || continue
      grep -qE "$PLACEHOLDER_UUID" <<<"$uuid" && continue
      report "$file" "$line" "azure-tenant-guid" "sts.windows.net/$uuid" \
        "use <tenant-guid> or 00000000-0000-0000-0000-000000000000"
    done < <(grep -oiE "sts\.windows\.net/$UUID_RE" <<<"$content" | grep -oiE "$UUID_RE")

    # Rule 3 — SAML entity / CoT hostnames. Scoped to lines that are actually
    # SAML metadata, because a blanket hostname rule would drown in false
    # positives from ordinary docs.
    if grep -qiE 'entityid|trustedproviders|metaalias|SAML2MetaCache|setToPrototolMap|assertionConsumerService|singleLogoutService|nameIdService|AuthConsumer|\|saml2' <<<"$content"; then
      while read -r host; do
        [ -n "$host" ] || continue
        is_placeholder_host "$host" && continue
        # Hosts another rule already owns, judged on their identifying part
        # rather than the hostname: the tenant label (rule 1) and the Azure
        # tenant GUID (rule 2). sts.windows.net itself identifies nobody.
        case "$host" in
          *forgeblocks.com | sts.windows.net) continue ;;
        esac
        report "$file" "$line" "saml-entity-hostname" "$host" \
          "use sp-a.example.com / sp-b.example.com / tenant.example.com"
      done < <(grep -oiE 'https?://[a-z0-9<>{}._-]+' <<<"$content" | sed -E 's|https?://||')
    fi

    # Rule 4 — base64url that decodes to a URL. Closes the escape hatch where
    # a violation is "fixed" by encoding the value instead of redacting it.
    # Anchored on aHR0c — base64url of "http" — so we decode only things that
    # really are encoded URLs instead of forking base64 for every long token.
    while read -r cand; do
      [ ${#cand} -ge 16 ] || continue
      decoded=$(printf '%s' "$cand" | tr '_-' '/+' \
        | base64 -d 2>/dev/null | tr -d '\0') || continue
      grep -qiE '^https?://' <<<"$decoded" || continue
      host=$(sed -E 's|https?://||; s|[/?#].*||' <<<"$decoded")
      is_placeholder_host "$host" && continue
      report "$file" "$line" "base64url-encoded-url" "$cand -> $decoded" \
        "redact the value before encoding, or use <entityId64>"
    done < <(grep -oE 'aHR0c[A-Za-z0-9_-]{11,}' <<<"$content")

    # Rule 5 — literal deny patterns held outside the repo (compiled once
    # into DENY_RE at startup, never echoed, so a finding does not reprint
    # the very name it is protecting).
    if [ -n "$DENY_RE" ] && grep -qiE "$DENY_RE" <<<"$content"; then
      report "$file" "$line" "denylist" "matched an external deny pattern" \
        "remove the value; see \$SENSITIVE_DENYLIST"
    fi
  done
}

# Rule 6 — the ignore rules that keep live tenant data out of the repo.
check_ignores() {
  local want missing=0
  for want in .envrc '.aic/' '/workspace/' '*.har' '/.ai/local.md'; do
    grep -qxF "$want" .gitignore || {
      echo "  MISSING from .gitignore: $want"
      missing=1
    }
  done
  if [ "$missing" = 1 ]; then
    findings=$((findings + 1))
    printf '\n.gitignore\n  rule:   ignore-rules-intact\n  fix:    restore the rules above; they keep live tenant data untracked\n'
  fi
}

case "$MODE" in
  selftest)
    # A scanner that silently stops matching reports "clean" forever. Prove
    # each rule still fires before trusting a clean run. Fixtures are inline,
    # never files, so they cannot themselves be committed.
    fails=0
    probe() {
      local name="$1" text="$2" out
      out=$(findings=0; scan_text "selftest" <<<"1:$text"; echo "F=$findings")
      if [[ "$out" == *"F=0"* ]]; then
        echo "FAIL  $name — rule did not fire"
        fails=$((fails + 1))
      elif ! prefilter <<<"1:$text" | grep -q .; then
        echo "FAIL  $name — rule fires, but the line is dropped by CANDIDATE_RE"
        fails=$((fails + 1))
      else
        echo "ok    $name"
      fi
    }
    # EXACTLY the pre-filter production uses, so the two cannot drift.
    prefilter() { grep -iE "^[0-9]+:.*($CANDIDATE_RE)"; }
    negative() {
      local name="$1" text="$2" out
      out=$(findings=0; scan_text "selftest" <<<"1:$text"; echo "F=$findings")
      if [[ "$out" == *"F=0"* ]]; then
        echo "ok    $name (correctly ignored)"
      else
        echo "FAIL  $name — false positive:"; echo "$out"
        fails=$((fails + 1))
      fi
    }
    probe "aic tenant hostname"  'https://openam-acme-sndbx.forgeblocks.com/am'
    probe "example substring is real" 'https://openam-realexampleclient-prod.forgeblocks.com/am'
    probe "azure tenant guid"    'check https://sts.windows.net/7f1e2d3c-4b5a-6978-8a9b-0c1d2e3f4a5b/|saml2'
    probe "saml entity hostname" '"entityId": "https://sso.acme.com.au"'
    probe "trusted provider array value" '  "https://sso.acme.com.au|saml2"'
    probe "ACS array value"       '  "https://sso.acme.com.au/am/AuthConsumer/metaAlias/alpha/sp"'
    probe "mixed-case entityId"  '"entityId": "https://sso.acme.com.au"'
    probe "base64url url"        '"_id": "aHR0cHM6Ly9zc28uYWNtZS5jb20uYXU"'
    negative "placeholder host"   'https://<your-tenant>.forgeblocks.com/am'
    negative "placeholder guid"   'https://sts.windows.net/00000000-0000-0000-0000-000000000000/|saml2'
    negative "placeholder entity" '"entityId": "https://sp-b.example.com"'
    negative "generic azure host" 'COTUtils.setToPrototolMap: check https://sts.windows.net/<tenant-guid>/|saml2'
    negative "camelCase not a host" 'export interface CrestFault { readonly __aicCrestFault: true }'
    # --fix must never touch a file the checker calls clean. Every negative
    # fixture must survive redaction byte-identical; a redactor stricter than
    # the checker silently rewrites good files.
    for_idempotence=(
      'https://<your-tenant>.forgeblocks.com/am'
      'base_url: "https://example.forgeblocks.com"'
      'https://tenant.forgeblocks.com/am'
      'openam-mytenant-prod.forgeblocks.com'
      'COTUtils.setToPrototolMap: check https://sts.windows.net/<tenant-guid>/|saml2'
      '    "https://sts.windows.net/00000000-0000-0000-0000-000000000000/|saml2",'
      '"entityId": "https://sp-b.example.com"'
      '    "https://sp-b.example.com|saml2"'
      '  "valueBase64": "aGVsbG8="'
    )
    for fixture in "${for_idempotence[@]}"; do
      if [ "$(printf '%s\n' "$fixture" | redact_stream)" = "$fixture" ]; then
        echo "ok    unchanged by redact: ${fixture:0:46}"
      else
        echo "FAIL  redact rewrote an already-clean line:"
        echo "        in:  $fixture"
        echo "        out: $(printf '%s\n' "$fixture" | redact_stream)"
        fails=$((fails + 1))
      fi
    done

    # The property that ties the two halves together: anything the checker
    # rejects, the redactor must neutralise. Without this they drift, and
    # --fix quietly leaves findings behind.
    roundtrip=$(printf '%s\n' \
      'GET https://openam-acme-sndbx.forgeblocks.com/am' \
      '  "entityId": "https://sso.acme.com.au",' \
      '  "trustedProviders": [' \
      '    "https://peer.acme.com.au|saml2"' \
      '  ]' \
      '  "assertionConsumerService": [' \
      '    "https://acs.acme.com.au/am/AuthConsumer/metaAlias/alpha/sp"' \
      '  ]' \
      '  "trustedProviders": ["https://sts.windows.net/7f1e2d3c-4b5a-6978-8a9b-0c1d2e3f4a5b/|saml2"]' \
      '  "_id": "aHR0cHM6Ly9zc28uYWNtZS5jb20uYXU"' | redact_stream)
    left=$(findings=0; scan_text "roundtrip" < <(grep -iEn "$CANDIDATE_RE" <<<"$roundtrip"); echo "F=$findings")
    if [[ "$left" == *"F=0"* ]]; then
      echo "ok    redact output passes the checker (round-trip)"
    else
      echo "FAIL  redact left findings behind:"; echo "$left"
      fails=$((fails + 1))
    fi

    if [ "$fails" -gt 0 ]; then
      echo; echo "selftest FAILED ($fails)"; exit 1
    fi
    echo; echo "selftest passed — all rules fire, no false positives on the fixtures"
    exit 0
    ;;
  staged)
    check_ignores
    # Added lines only. A commit that REMOVES a leaked value must not be
    # rejected for containing it on the '-' side.
    while IFS= read -r file; do
      [ "$file" = "$SELF" ] && continue
      scan_text "$file" < <(git diff --cached -U0 -- "$file" \
        | awk '/^@@/ { split($3, a, ","); n = a[1]; sub(/^\+/, "", n); next }
               /^\+/ && !/^\+\+\+/ { print n":"substr($0,2); n++ }' \
        | grep -iE "^[0-9]+:.*($CANDIDATE_RE)")
    done < <(git diff --cached --name-only --diff-filter=ACMR)
    ;;
  tracked)
    check_ignores
    while IFS= read -r file; do
      [ "$file" = "$SELF" ] && continue
      scan_text "$file" < <(grep -IniE "$CANDIDATE_RE" -- "$file" 2>/dev/null)
    done < <(git ls-files)
    ;;
  redact)
    redact_stream
    exit 0
    ;;
  fix)
    changed=0
    while IFS= read -r file; do
      [ "$file" = "$SELF" ] && continue
      grep -IqiE "$CANDIDATE_RE" -- "$file" 2>/dev/null || continue
      redact_stream <"$file" >"$file.redacted" || { rm -f "$file.redacted"; continue; }
      if cmp -s "$file" "$file.redacted"; then
        rm -f "$file.redacted"
      else
        mv "$file.redacted" "$file"
        echo "redacted $file"
        changed=$((changed + 1))
      fi
    done < <(git ls-files)
    echo "--fix rewrote $changed file(s). Re-run --tracked and read the diff."
    exit 0
    ;;
  history)
    while IFS= read -r obj; do
      scan_text "blob:$obj" < <(git cat-file -p "$obj" 2>/dev/null | grep -IniE "$CANDIDATE_RE")
    done < <(
      git rev-list --objects "$HISTORY_RANGE" \
        | while read -r object path; do
            [ "$path" = "$SELF" ] && continue
            printf '%s\n' "$object"
          done \
        | git cat-file --batch-check='%(objectname) %(objecttype)' \
        | awk '$2 == "blob" {print $1}' \
        | sort -u
    )
    ;;
esac

if [ "$findings" -gt 0 ]; then
  printf '\n%s findings. Nothing was committed.\n' "$findings"
  printf 'Redact the value — do NOT base64 it; rule 4 decodes that.\n'
  exit 1
fi
echo "check-sensitive-metadata: clean (${MODE})"
