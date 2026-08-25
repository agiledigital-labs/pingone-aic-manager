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
#   --history    every blob in every reachable commit  (audit)
#   --selftest   prove the rules still fire            (CI runs this first)
#
# Exit 0 clean, 1 findings, 2 usage/internal error.
set -uo pipefail

MODE=tracked
case "${1:-}" in
  --staged) MODE=staged ;;
  --tracked | "") MODE=tracked ;;
  --history) MODE=history ;;
  --selftest) MODE=selftest ;;
  -h | --help)
    sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
    exit 0
    ;;
  *)
    echo "usage: $0 [--staged|--tracked|--history|--selftest]" >&2
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
PLACEHOLDER_TENANT='<|\{|example|your-tenant|my-?tenant|placeholder|^tenant\.'
UUID_RE='[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}'

# Literal deny patterns, compiled once. The file lives OUTSIDE the repo (or in
# the gitignored .ai/denylist.txt): committing a list of client names would
# commit the client names. One pattern per line, '#' comments ignored.
DENY_RE=""
if [ -n "${SENSITIVE_DENYLIST:-}" ] && [ -f "${SENSITIVE_DENYLIST}" ]; then
  DENY_RE=$(grep -vE '^\s*(#|$)' "${SENSITIVE_DENYLIST}" | paste -sd '|' -)
fi

# Cheap pre-filter: only lines that could possibly match a rule are worth the
# per-line analysis. Without this a full-tree scan forks greps for every line
# of every file and takes minutes.
CANDIDATE_RE='forgeblocks|sts\.windows\.net|entityid|trustedproviders|metaalias|SAML2MetaCache|setToPrototolMap|aHR0c'

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
    if grep -qiE 'entityid|trustedproviders|metaalias|SAML2MetaCache|setToPrototolMap' <<<"$content"; then
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
    probe "azure tenant guid"    'check https://sts.windows.net/7f1e2d3c-4b5a-6978-8a9b-0c1d2e3f4a5b/|saml2'
    probe "saml entity hostname" '"entityId": "https://sso.acme.com.au"'
    probe "mixed-case entityId"  '"entityId": "https://sso.acme.com.au"'
    probe "base64url url"        '"_id": "aHR0cHM6Ly9zc28uYWNtZS5jb20uYXU"'
    negative "placeholder host"   'https://<your-tenant>.forgeblocks.com/am'
    negative "placeholder guid"   'https://sts.windows.net/00000000-0000-0000-0000-000000000000/|saml2'
    negative "placeholder entity" '"entityId": "https://sp-b.example.com"'
    negative "generic azure host" 'COTUtils.setToPrototolMap: check https://sts.windows.net/<tenant-guid>/|saml2'
    negative "camelCase not a host" 'export interface CrestFault { readonly __aicCrestFault: true }'
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
  history)
    while IFS= read -r obj; do
      scan_text "blob:$obj" < <(git cat-file -p "$obj" 2>/dev/null | grep -IniE "$CANDIDATE_RE")
    done < <(git rev-list --objects --all | awk 'NF==2 {print $1}')
    ;;
esac

if [ "$findings" -gt 0 ]; then
  printf '\n%s findings. Nothing was committed.\n' "$findings"
  printf 'Redact the value — do NOT base64 it; rule 4 decodes that.\n'
  exit 1
fi
echo "check-sensitive-metadata: clean (${MODE})"
