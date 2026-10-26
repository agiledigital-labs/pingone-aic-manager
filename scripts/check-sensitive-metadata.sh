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
#                defaults to every reachable commit; over the whole history,
#                SENSITIVE_HISTORY_EXPECTED=N passes on EXACTLY N findings
#   --selftest   prove the rules still fire            (CI runs this first)
#   --redact     filter stdin -> stdout, sanitised    (capture-time)
#                REDACT_VALUES=1 additionally strips ESV valueBase64 payloads
#   --fix        rewrite tracked files in place       (same redactions)
#
# Exit 0 clean, 1 findings, 2 usage/internal error.
#
# Known historic findings. A value added to the denylist after it was committed
# is in old blobs for good: history is append-only, and the remote keeps
# force-pushed objects anyway. SENSITIVE_HISTORY_EXPECTED records how many
# findings the whole-history scan is known to produce, and the scan passes only
# on exactly that many. Exact, not a ceiling: a rewrite that drops old findings
# would otherwise leave room for new ones to hide under the old total. It is a
# per-denylist number, so it lives beside the denylist — .envrc locally, a
# repository variable in CI — never in a committed file. Range scans ignore it:
# a range is new commits, and any finding there is new.
set -uo pipefail
# Pathspec magic is how `staged_lines` names exactly one file. An inherited
# GIT_LITERAL_PATHSPECS=1 turns `:(literal)name` into a file literally called
# that, so the staged diff came back empty and every staged leak passed; the
# glob/icase variants change what a name matches. None is ours to inherit.
unset GIT_LITERAL_PATHSPECS GIT_GLOB_PATHSPECS GIT_NOGLOB_PATHSPECS GIT_ICASE_PATHSPECS
# Every rule here matches ASCII hostnames case-insensitively, and both
# halves fold case through the locale: `grep -i` in the checker, `tolower` in
# the redactor. Under a Turkish locale ASCII `I` folds to dotless `ı`, so an
# uppercase `.ID.FORGEROCK.IO` host passed --redact unchanged and the checker
# reported nothing (measured 2026-10-26). Pin the byte locale for the whole run.
#
# Except the external denylist: it holds literal names, which may not be
# ASCII, and folding `É` onto `é` needs the caller's locale — under C, GNU
# grep matched neither way (measured 2026-10-26). So the caller's LC_ALL is
# kept, and every denylist match runs through `in_caller_locale`.
CALLER_LC_ALL=${LC_ALL-}
CALLER_LC_ALL_SET=${LC_ALL+set}
export LC_ALL=C
in_caller_locale() {
  if [ -n "$CALLER_LC_ALL_SET" ]; then
    LC_ALL=$CALLER_LC_ALL "$@"
  else
    env -u LC_ALL "$@"
  fi
}

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
# A tenant lives under either domain: forgeblocks.com, or id.forgerock.io —
# missed until 2026-10-26, when a capture printed an id.forgerock.io tenant
# host unredacted. `apidocs` is Ping's public API reference, not a tenant.
PLACEHOLDER_TENANT='^(<[^>]+>|\{[^}]+\}|example|tenant|your-tenant|my-?tenant|placeholder|apidocs|openam-mytenant-(sndbx|dev|uat|prod))\.(forgeblocks\.com|id\.forgerock\.io)$'
TENANT_DOMAIN_RE='\.(forgeblocks\.com|id\.forgerock\.io)'
UUID_RE='[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}'

# Literal deny patterns, compiled once. The file lives OUTSIDE the repo (or in
# the gitignored .ai/denylist.txt): committing a list of client names would
# commit the client names. One pattern per line, '#' comments ignored.
#
# A denylist that is configured but cannot be read stops the scan: an empty
# DENY_RE is indistinguishable from "no denylist" downstream, so a read error
# here would otherwise pass every literal it was meant to catch. Only a list
# that holds nothing but comments and blank lines is legitimately empty.
deny_lines() {
  local lines status
  lines=$(grep -vE '^\s*(#|$)' "$@")
  status=$?
  [ "$status" -le 1 ] || return 1
  [ -n "$lines" ] || return 0
  printf '%s\n' "$lines" | paste -sd '|' -
}
deny_unreadable() {
  echo "error: the sensitive metadata denylist ($1) could not be read" >&2
  exit 2
}
DENY_RE=""
if [ -n "${SENSITIVE_DENYLIST_CONTENT:-}" ]; then
  DENY_RE=$(printf '%s\n' "$SENSITIVE_DENYLIST_CONTENT" | deny_lines) \
    || deny_unreadable SENSITIVE_DENYLIST_CONTENT
elif [ -n "${SENSITIVE_DENYLIST:-}" ]; then
  [ -f "$SENSITIVE_DENYLIST" ] || deny_unreadable "$SENSITIVE_DENYLIST is not a file"
  DENY_RE=$(deny_lines -- "$SENSITIVE_DENYLIST") || deny_unreadable "$SENSITIVE_DENYLIST"
fi
EXPECTED="${SENSITIVE_HISTORY_EXPECTED:-}"
if [ -n "$EXPECTED" ] && ! [[ "$EXPECTED" =~ ^[0-9]+$ ]]; then
  echo "error: SENSITIVE_HISTORY_EXPECTED must be a count, got '$EXPECTED'" >&2
  exit 2
fi
# Only the whole-history scan is counted; see the header. The requirement
# applies to that scan alone, so one environment serves every mode.
COUNTED=0
if [ "$MODE" = history ] && [ "$HISTORY_RANGE" = --all ]; then
  if [ "${REQUIRE_SENSITIVE_HISTORY_EXPECTED:-0}" = 1 ] && [ -z "$EXPECTED" ]; then
    echo "error: SENSITIVE_HISTORY_EXPECTED is required (0 if the history is clean)" >&2
    exit 2
  fi
  [ -n "$EXPECTED" ] && COUNTED=1
fi

if [ "${REQUIRE_SENSITIVE_DENYLIST:-0}" = 1 ] && [ -z "$DENY_RE" ]; then
  echo "error: a non-empty sensitive metadata denylist is required" >&2
  exit 2
fi

# Decode a base64url candidate. Pads to a multiple of 4 first: base64url drops
# the padding, and a STRICT `base64 -d` (GNU coreutils 8.32, as on the CI
# runner) then refuses the whole string, while a lenient one (9.x, as on some
# dev machines) decodes it happily. That difference is invisible — the decode
# fails quietly, the rule never fires, and CI passes a file a developer's
# machine would have caught. Measured 2026-09-14: the selftest's own
# `aHR0cHM6Ly9zc28uYWNtZS5jb20uYXU` fixture is 31 characters, so it needs one
# `=`, and it is exactly the case that failed only in CI.
decode_base64url() {
  _b64_value=$(printf '%s' "$1" | tr '_-' '/+')
  while [ $(( ${#_b64_value} % 4 )) -ne 0 ]; do
    _b64_value="${_b64_value}="
  done
  printf '%s' "$_b64_value" | base64 -d 2>/dev/null | tr -d '\0'
}

# Cheap pre-filter: only lines that could possibly match a rule are worth the
# per-line analysis. Without this a full-tree scan forks greps for every line
# of every file and takes minutes.
# A sibling-checkout path whose directory segment has been anonymised. The
# anonymising IS the tell: nobody writes `~/w/<client>/…` about a checkout that
# belongs to this project, so the placeholder marks the path as a client's and
# the path shape then announces that a client corpus exists and what is in it.
# Cite the evidence in a committed doc; let `.ai/local.md` say where it lives.
CLIENT_CHECKOUT_RE='~/w/(<[^/>]*>|\{[^/}]*\}|client-[a-z])'

CANDIDATE_RE='~/w/|forgeblocks|forgerock\.io|sts\.windows\.net|entityid|trustedproviders|metaalias|SAML2MetaCache|setToPrototolMap|assertionConsumerService|singleLogoutService|nameIdService|AuthConsumer|\|saml2|aHR0c'

# The pre-filter every mode uses: stdin -> the lines worth scanning, in input
# order, as `N:line` with -n. The ASCII rules match under C; the denylist
# under the caller's locale (see `in_caller_locale`), and the two are merged
# by line number. Without a denylist it is one grep, as before.
#
# It runs inside process substitutions and pipelines, whose exit status the
# scan never sees: a pre-filter that fails there feeds the scan nothing, and
# nothing scans clean. So a failure — mktemp, grep exiting above 1, the merge
# — is reported and kills the main shell (`scan_failed`), which exits 2. The
# same holds for what feeds it and for the lists the modes iterate: those go
# through `filtered` and `listing` below, never a bare process substitution.
MAIN_PID=$$
trap 'exit 2' TERM
scan_failed() {
  echo "check-sensitive-metadata: pre-filter failed: $1" >&2
  kill -TERM "$MAIN_PID"
  exit 2
}
candidates() {
  local number="${1:-}" tmp status
  if [ -z "$DENY_RE" ]; then
    grep "-I${number#-}iE" "$CANDIDATE_RE"
    status=$?
    [ "$status" -le 1 ] || scan_failed "grep exited $status"
    return 0
  fi
  tmp=$(mktemp -d) || scan_failed "mktemp failed"
  cat >"$tmp/in" || scan_failed "could not buffer the input"
  grep -IniE "$CANDIDATE_RE" -- "$tmp/in" >"$tmp/shape"
  status=$?
  [ "$status" -le 1 ] || { rm -rf "$tmp"; scan_failed "grep exited $status"; }
  in_caller_locale grep -IniE "$DENY_RE" -- "$tmp/in" >"$tmp/deny"
  status=$?
  [ "$status" -le 1 ] || { rm -rf "$tmp"; scan_failed "denylist grep exited $status"; }
  sort -t: -k1,1n -u "$tmp/shape" "$tmp/deny" \
    | if [ -n "$number" ]; then cat; else cut -d: -f2-; fi
  status=$?
  rm -rf "$tmp"
  [ "$status" = 0 ] || scan_failed "merging the pre-filter's halves exited $status"
}

# `filtered [-n] CMD...`: CMD's output through the pre-filter, failing the
# scan when CMD fails — a file that cannot be opened, a `git` that errors —
# instead of handing the scan an empty stream. pipefail makes the pipeline's
# status CMD's.
filtered() {
  local number=""
  if [ "${1:-}" = -n ]; then number=-n; shift; fi
  "$@" | candidates $number || scan_failed "reading the scan's input failed: $*"
}

# `listing CMD...`: what a mode iterates (files, blobs), failing the scan when
# the listing itself fails — an empty list scans clean.
listing() {
  "$@" || scan_failed "listing what to scan failed: $*"
}

# Every blob the history range introduces, except this script's own. One
# function so `listing` sees the whole pipeline's status under pipefail.
history_blobs() {
  git rev-list --objects "$HISTORY_RANGE" \
    | while read -r object path; do
        [ "$path" = "$SELF" ] && continue
        printf '%s\n' "$object"
      done \
    | git cat-file --batch-check='%(objectname) %(objecttype)' \
    | awk '$2 == "blob" {print $1}' \
    | sort -u
}

# The added lines of one staged file, numbered as `N:line`.
staged_lines() {
  # `:(literal)` so a name holding `*` or `?` is that one file, not a glob.
  git diff --cached -U0 -- ":(literal)$1" \
    | awk '/^@@/ { split($3, a, ","); n = a[1]; sub(/^\+/, "", n); next }
           /^\+/ && !/^\+\+\+/ { print n":"substr($0,2); n++ }'
}

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
    # Every tenant host on the line, in any case, rewritten to the
    # placeholder; an allowed host is stepped over, never a reason to stop
    # (a `break` here once left every host after `apidocs` on the line raw).
    # Matching runs on a lowercased copy, whose offsets are the same.
    function redact_tenants(line, domain,   out, low, host) {
      out = ""
      while (1) {
        low = tolower(line)
        if (!match(low, "[a-z0-9._-]+\\." dom_re(domain))) break
        host = substr(line, RSTART, RLENGTH)
        if (is_placeholder_tenant(host)) {
          out = out substr(line, 1, RSTART + RLENGTH - 1)
        } else {
          out = out substr(line, 1, RSTART - 1) "<your-tenant>." domain
        }
        line = substr(line, RSTART + RLENGTH)
      }
      return out line
    }
    function dom_re(d,   r) {
      r = d
      gsub(/\./, "\\.", r)
      return r
    }
    function is_placeholder_tenant(h,   l) {
      l = tolower(h)
      return l ~ /^<[^>]+>\.(forgeblocks\.com|id\.forgerock\.io)$/ ||
             l ~ /^\{[^}]+\}\.(forgeblocks\.com|id\.forgerock\.io)$/ ||
             l ~ /^(example|tenant|your-tenant|my-?tenant|placeholder|apidocs)\.(forgeblocks\.com|id\.forgerock\.io)$/ ||
             l ~ /^openam-mytenant-(sndbx|dev|uat|prod)\.(forgeblocks\.com|id\.forgerock\.io)$/
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

        line = redact_tenants(line, "forgeblocks.com")
        line = redact_tenants(line, "id.forgerock.io")

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
            if (!is_placeholder_host(host) && host !~ /(forgeblocks\.com|id\.forgerock\.io)$/ && host != "sts.windows.net") {
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
        decoded=$(decode_base64url "$cand")
        grep -qiE '^https?://' <<<"$decoded" || continue
        host=$(sed -E 's|https?://||; s|[/?#].*||' <<<"$decoded")
        printf '%s' "$host" | grep -qiE "$PLACEHOLDER_HOST" && continue
        line=${line//"$cand"/<entityId64>}
      done
      # Rule 7's counterpart. Dropping the `~/w/` prefix is the whole point:
      # there is no shape worth preserving here, because the shape IS the leak.
      # A failure here would print an empty line and carry on: exit, so
      # pipefail reports it instead of a redaction that dropped the text.
      line=$(sed -E "s#$CLIENT_CHECKOUT_RE#<client-checkout>#g" <<<"$line") || exit 1
      printf '%s\n' "$line"
    done
  }
}

findings=0
held=""

# report FILE LINE RULE DETAIL REMEDY
# A counted scan holds its findings: printed only if the count is wrong, so a
# pass is not 69 blocks of known noise.
report() {
  findings=$((findings + 1))
  local block
  block=$(printf '\n%s:%s\n  rule:   %s\n  found:  %s\n  fix:    %s' "$1" "$2" "$3" "$4" "$5")
  if [ "$COUNTED" = 1 ]; then
    held+="$block"$'\n'
  else
    printf '%s\n' "$block"
  fi
}

# history_verdict FOUND EXPECTED — 0 when they match exactly, 1 otherwise.
# Split out so --selftest drives the real comparison.
history_verdict() {
  [ "$1" -eq "$2" ]
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
        "use <your-tenant>.forgeblocks.com or <your-tenant>.id.forgerock.io"
    done < <(grep -oiE "[a-z0-9<>{}._-]*$TENANT_DOMAIN_RE" <<<"$content")

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
          *forgeblocks.com | *id.forgerock.io | sts.windows.net) continue ;;
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
      decoded=$(decode_base64url "$cand") || continue
      grep -qiE '^https?://' <<<"$decoded" || continue
      host=$(sed -E 's|https?://||; s|[/?#].*||' <<<"$decoded")
      is_placeholder_host "$host" && continue
      report "$file" "$line" "base64url-encoded-url" "$cand -> $decoded" \
        "redact the value before encoding, or use <entityId64>"
    done < <(grep -oE 'aHR0c[A-Za-z0-9_-]{11,}' <<<"$content")

    # Rule 5 — literal deny patterns held outside the repo (compiled once
    # into DENY_RE at startup, never echoed, so a finding does not reprint
    # the very name it is protecting).
    if [ -n "$DENY_RE" ] && in_caller_locale grep -qiE "$DENY_RE" <<<"$content"; then
      report "$file" "$line" "denylist" "matched an external deny pattern" \
        "remove the value; see \$SENSITIVE_DENYLIST"
    fi

    # Rule 7 — a client checkout's path, placeholder and all. Unlike every
    # rule above it, this one fires on text that has ALREADY been redacted:
    # swapping the client's name for `<client>` leaves the path shape, and the
    # shape is what says a client corpus exists and roughly what is in it.
    if grep -qE "$CLIENT_CHECKOUT_RE" <<<"$content"; then
      report "$file" "$line" "client-checkout-path" \
        "$(grep -oE "$CLIENT_CHECKOUT_RE[^ \`\"',)]*" <<<"$content" | head -1)" \
        "cite the evidence, not the path; \`.ai/local.md\` records where it lives"
    fi
  done
}

# Rule 6 — the ignore rules that keep live tenant data out of the repo.
check_ignores() {
  local want missing=0
  [ -r .gitignore ] || scan_failed ".gitignore cannot be read"
  for want in .envrc '.aic/' '/workspace/' '*.har' '/.ai/local.md'; do
    grep -qxF -- "$want" .gitignore
    case $? in
      0) ;;
      1)
        echo "  MISSING from .gitignore: $want"
        missing=1
        ;;
      *) scan_failed "reading .gitignore failed" ;;
    esac
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
    prefilter() { candidates; }
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
    probe "id.forgerock.io tenant" 'GET https://openam-acme-dev.id.forgerock.io/environment'
    probe "example substring is real" 'https://openam-realexampleclient-prod.forgeblocks.com/am'
    probe "azure tenant guid"    'check https://sts.windows.net/7f1e2d3c-4b5a-6978-8a9b-0c1d2e3f4a5b/|saml2'
    probe "saml entity hostname" '"entityId": "https://sso.acme.com.au"'
    probe "trusted provider array value" '  "https://sso.acme.com.au|saml2"'
    probe "ACS array value"       '  "https://sso.acme.com.au/am/AuthConsumer/metaAlias/alpha/sp"'
    probe "mixed-case entityId"  '"entityId": "https://sso.acme.com.au"'
    probe "base64url url"        '"_id": "aHR0cHM6Ly9zc28uYWNtZS5jb20uYXU"'
    # One fixture per padding residue. base64url strips the '=' padding, and a
    # strict base64 -d refuses an unpadded string outright — so a decoder that
    # forgot to re-pad fires on neither, and does it silently. Only a strict
    # base64 (GNU coreutils 8.32, the CI runner) tells these apart; a lenient
    # 9.x decodes both either way.
    probe "base64url url (2-char pad)" '"_id": "aHR0cHM6Ly9zc28uYWNtZS5jb20uYQ"'
    probe "client checkout path" 'the corpus lives in `~/w/<client>/sandbox-scripts`'
    probe "client checkout, reserved name" 'cited against `~/w/client-a/logs/prod-logs.json`'
    negative "our own sibling checkout" 'patterns borrowed from `~/w/tally` and `~/w/aic/who-changed`'
    negative "placeholder host"   'https://<your-tenant>.forgeblocks.com/am'
    negative "placeholder id.forgerock.io" 'https://<your-tenant>.id.forgerock.io/am'
    negative "public api docs"    '- Ping docs: <https://apidocs.id.forgerock.io/> (SAML2 section).'
    negative "bare domain prose"  'same host as AM, NOT `id.forgerock.io`'
    negative "placeholder guid"   'https://sts.windows.net/00000000-0000-0000-0000-000000000000/|saml2'
    negative "placeholder entity" '"entityId": "https://sp-b.example.com"'
    negative "generic azure host" 'COTUtils.setToPrototolMap: check https://sts.windows.net/<tenant-guid>/|saml2'
    negative "camelCase not a host" 'export interface CrestFault { readonly __aicCrestFault: true }'

    # The denylist folds case in the caller's locale, not the pinned C one: a
    # non-ASCII literal must still match its other case, and a line holding
    # only that literal must survive the pre-filter. Run under C.UTF-8 here
    # whatever the caller's locale, with a synthetic pattern, in a subshell so
    # the real denylist is untouched. Under C this fixture fails (measured).
    if (
      DENY_RE='synthetiqué'
      CALLER_LC_ALL=C.UTF-8
      CALLER_LC_ALL_SET="set"
      out=$(findings=0; scan_text "selftest" < <(printf 'line SYNTHETIQUÉ\n' | candidates -n); echo "F=$findings")
      [[ "$out" != *"F=0"* ]]
    ); then
      echo "ok    denylist folds non-ASCII case in the caller's locale"
    else
      echo "FAIL  denylist folds non-ASCII case in the caller's locale"
      fails=$((fails + 1))
    fi
    # A pre-filter that cannot run must fail the scan, not empty it: in a
    # process substitution its status is invisible, so it kills the main
    # shell. Driven in a subshell standing in for main, with mktemp made to
    # fail (with a denylist) and grep made to fail (without one).
    prefilter_fails() {
      local name="$1" deny="$2" tmpdir="$3" status
      (
        MAIN_PID=$BASHPID
        trap 'exit 2' TERM
        DENY_RE=$deny
        export TMPDIR=$tmpdir
        scan_text "selftest" < <(printf 'https://x.forgeblocks.com\n' | candidates -n 2>/dev/null)
        echo "reached the end of the scan"
        exit 0
      ) >/dev/null
      status=$?
      if [ "$status" = 2 ]; then
        echo "ok    $name"
      else
        echo "FAIL  $name — the scan finished (exit $status)"
        fails=$((fails + 1))
      fi
    }
    prefilter_fails "a pre-filter whose mktemp fails stops the scan" 'synthetic' /nonexistent/dir
    # What feeds the pre-filter, and what a mode iterates, fail the same way:
    # the helpers every mode uses, driven with a failing producer.
    feeder_fails() {
      local name="$1" status
      shift
      (
        MAIN_PID=$BASHPID
        trap 'exit 2' TERM
        scan_text "selftest" < <("$@" 2>/dev/null)
        exit 0
      ) >/dev/null 2>&1
      status=$?
      if [ "$status" = 2 ]; then
        echo "ok    $name"
      else
        echo "FAIL  $name — the scan finished (exit $status)"
        fails=$((fails + 1))
      fi
    }
    feeder_fails "an input that cannot be opened stops the scan" filtered -n cat -- /nonexistent/file
    feeder_fails "a failing producer stops the scan" filtered -n false
    feeder_fails "a failing listing stops the scan" listing false
    (
      CANDIDATE_RE='('
      fails=0
      prefilter_fails "a pre-filter whose grep fails stops the scan" '' "${TMPDIR:-/tmp}"
      exit "$fails"
    ) || fails=$((fails + 1))
    # End to end, in a scratch repository: the listings and the denylist are
    # read before any rule runs, so their failures only show as a whole run.
    (
      script="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
      repo=$(mktemp -d) || exit 1
      trap 'chmod -R u+w "$repo"; rm -rf "$repo"' EXIT
      cd "$repo" || exit 1
      git init -q . && git config user.email t@example.com && git config user.name t
      printf '%s\n' .envrc '.aic/' '/workspace/' '*.har' '/.ai/local.md' >.gitignore
      leak='https://openam-acme-sndbx.forgeblocks.com/am'
      fails=0
      run() {
        local name="$1" want="$2" got
        shift 2
        env -u SENSITIVE_DENYLIST -u SENSITIVE_DENYLIST_CONTENT \
          -u REQUIRE_SENSITIVE_DENYLIST "$@" >/dev/null 2>&1
        got=$?
        if [ "$got" = "$want" ]; then
          echo "ok    $name"
        else
          echo "FAIL  $name — exit $got, wanted $want"
          fails=$((fails + 1))
        fi
      }
      # A name git C-quotes in a newline listing: a tab, and a non-ASCII byte.
      printf '%s\n' "$leak" >"$(printf 'tab\there.md')"
      printf '%s\n' "$leak" >"$(printf 'caf\303\251.md')"
      git add -A >/dev/null
      run "a quoted file name is staged-scanned" 1 bash "$script" --staged
      git commit -qm t >/dev/null
      run "a quoted file name is tracked-scanned" 1 bash "$script" --tracked
      rm -f -- *.md && git commit -qam clean >/dev/null
      run "control: the clean tree passes" 0 bash "$script" --tracked
      run "a denylist that is not a file stops the scan" 2 \
        SENSITIVE_DENYLIST=/nonexistent/denylist bash "$script" --tracked
      printf 'acme\n' >"$repo/.git/deny" && chmod 000 "$repo/.git/deny"
      if [ -r "$repo/.git/deny" ]; then
        echo "skip  an unreadable denylist stops the scan (running as root)"
      else
        run "an unreadable denylist stops the scan" 2 \
          SENSITIVE_DENYLIST="$repo/.git/deny" bash "$script" --tracked
      fi
      printf '# only comments\n\n' >"$repo/.git/empty"
      run "control: a denylist of comments only is empty, not an error" 0 \
        SENSITIVE_DENYLIST="$repo/.git/empty" bash "$script" --tracked
      # An awk that fails, ahead of the real one on PATH.
      mkdir "$repo/.git/shim" && printf '#!/bin/sh\nexit 3\n' >"$repo/.git/shim/awk"
      chmod +x "$repo/.git/shim/awk"
      run "a failing --redact exits non-zero" 2 \
        PATH="$repo/.git/shim:$PATH" bash "$script" --redact <<<"$leak"
      # A sed that fails only on the redactor's last substitution, so every
      # earlier step still runs: the one failure that printed an empty line.
      real_sed=$(command -v sed)
      mkdir "$repo/.git/sedshim"
      printf '#!/bin/sh\ncase "$*" in *client-checkout*) exit 3 ;; esac\nexec %s "$@"\n' \
        "$real_sed" >"$repo/.git/sedshim/sed"
      chmod +x "$repo/.git/sedshim/sed"
      run "a failing redactor sed exits non-zero" 2 \
        PATH="$repo/.git/sedshim:$PATH" bash "$script" --redact <<<"$leak"
      run "control: --redact with the real sed exits zero" 0 \
        bash "$script" --redact <<<"$leak"
      # Pathspec settings a caller may have exported must not empty the
      # staged diff.
      printf '%s\n' "$leak" >staged.md && git add staged.md >/dev/null
      run "an inherited GIT_LITERAL_PATHSPECS still scans staged lines" 1 \
        GIT_LITERAL_PATHSPECS=1 bash "$script" --staged
      run "an inherited GIT_ICASE_PATHSPECS still scans staged lines" 1 \
        GIT_ICASE_PATHSPECS=1 bash "$script" --staged
      git rm -q --cached staged.md >/dev/null && rm -f staged.md
      mkdir locked && printf '%s\n' "$leak" >locked/f.md && git add locked >/dev/null
      git commit -qm locked >/dev/null && chmod a-w locked
      run "a --fix that cannot write exits non-zero" 2 bash "$script" --fix
      exit "$fails"
    ) || fails=$((fails + 1))
    # --fix must never touch a file the checker calls clean. Every negative
    # fixture must survive redaction byte-identical; a redactor stricter than
    # the checker silently rewrites good files.
    for_idempotence=(
      'https://<your-tenant>.forgeblocks.com/am'
      'base_url: "https://example.forgeblocks.com"'
      'https://tenant.forgeblocks.com/am'
      'openam-mytenant-prod.forgeblocks.com'
      'https://<your-tenant>.id.forgerock.io/am'
      '- Ping docs: <https://apidocs.id.forgerock.io/> (SAML2 section).'
      'COTUtils.setToPrototolMap: check https://sts.windows.net/<tenant-guid>/|saml2'
      '    "https://sts.windows.net/00000000-0000-0000-0000-000000000000/|saml2",'
      '"entityId": "https://sp-b.example.com"'
      '    "https://sp-b.example.com|saml2"'
      '  "valueBase64": "aGVsbG8="'
      'patterns borrowed from `~/w/tally` and `~/w/aic/who-changed`'
      'the corpus lives in `<client-checkout>/sandbox-scripts`'
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
      'GET https://openam-acme-dev.id.forgerock.io/environment' \
      '{"docs":"https://apidocs.id.forgerock.io","tenant":"https://acme-dev.id.forgerock.io"}' \
      'x example.forgeblocks.com then https://openam-acme-uat.forgeblocks.com' \
      'GET https://OPENAM-ACME.FORGEBLOCKS.COM/am and https://Acme-Dev.ID.FORGEROCK.IO' \
      '  "entityId": "https://sso.acme.com.au",' \
      '  "trustedProviders": [' \
      '    "https://peer.acme.com.au|saml2"' \
      '  ]' \
      '  "assertionConsumerService": [' \
      '    "https://acs.acme.com.au/am/AuthConsumer/metaAlias/alpha/sp"' \
      '  ]' \
      '  "trustedProviders": ["https://sts.windows.net/7f1e2d3c-4b5a-6978-8a9b-0c1d2e3f4a5b/|saml2"]' \
      '  "_id": "aHR0cHM6Ly9zc28uYWNtZS5jb20uYXU"' \
      'corpus at ~/w/<client>/sandbox-scripts and ~/w/client-a/logs' | redact_stream)
    left=$(findings=0; scan_text "roundtrip" < <(candidates -n <<<"$roundtrip"); echo "F=$findings")
    if [[ "$left" == *"F=0"* ]]; then
      echo "ok    redact output passes the checker (round-trip)"
    else
      echo "FAIL  redact left findings behind:"; echo "$left"
      fails=$((fails + 1))
    fi

    # The known-count comparison is exact in both directions: fewer findings
    # than expected means history changed and the number needs resetting
    # deliberately, not that a new finding has room to hide.
    for case in "69 69 0" "70 69 1" "68 69 1" "0 0 0" "1 0 1"; do
      read -r f e want <<<"$case"
      history_verdict "$f" "$e"; got=$?
      if [ "$got" = "$want" ]; then
        echo "ok    history count: $f found, $e expected -> $want"
      else
        echo "FAIL  history count: $f found, $e expected -> $got, want $want"
        fails=$((fails + 1))
      fi
    done

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
    # NUL-delimited: a newline-delimited listing C-quotes any name with a
    # space-adjacent oddity, tab, quote or non-ASCII byte, and a quoted name
    # is not the file — it would be skipped, or scanned as the wrong path.
    while IFS= read -r -d '' file; do
      [ "$file" = "$SELF" ] && continue
      scan_text "$file" < <(filtered staged_lines "$file")
    done < <(listing git diff --cached --name-only -z --diff-filter=ACMR)
    ;;
  tracked)
    check_ignores
    while IFS= read -r -d '' file; do
      [ "$file" = "$SELF" ] && continue
      # A tracked file deleted from the work tree has nothing to scan;
      # anything else that cannot be read fails the scan.
      [ -e "$file" ] || [ -L "$file" ] || continue
      scan_text "$file" < <(filtered -n cat -- "$file")
    done < <(listing git ls-files -z)
    ;;
  redact)
    # Output already written cannot be recalled, so a failure part way is
    # said loudly: what was printed is incomplete, not sanitised.
    redact_stream || {
      echo "check-sensitive-metadata: --redact failed; its output is incomplete" >&2
      exit 2
    }
    exit 0
    ;;
  fix)
    changed=0
    failed=0
    while IFS= read -r -d '' file; do
      [ "$file" = "$SELF" ] && continue
      # Not `| grep -q .`: it closes the pipe at the first line, and the
      # pre-filter's grep would die of SIGPIPE and read as a failure.
      [ -e "$file" ] || [ -L "$file" ] || continue
      [ -n "$(filtered cat -- "$file")" ] || continue
      if ! redact_stream <"$file" >"$file.redacted"; then
        rm -f "$file.redacted"
        echo "FAILED to redact $file; left as it was" >&2
        failed=$((failed + 1))
        continue
      fi
      if cmp -s "$file" "$file.redacted"; then
        rm -f "$file.redacted"
      elif mv "$file.redacted" "$file"; then
        echo "redacted $file"
        changed=$((changed + 1))
      else
        rm -f "$file.redacted"
        echo "FAILED to replace $file; left as it was" >&2
        failed=$((failed + 1))
      fi
    done < <(listing git ls-files -z)
    echo "--fix rewrote $changed file(s). Re-run --tracked and read the diff."
    if [ "$failed" -gt 0 ]; then
      echo "--fix could not redact $failed file(s); they still hold what it found" >&2
      exit 2
    fi
    exit 0
    ;;
  history)
    while IFS= read -r obj; do
      scan_text "blob:$obj" < <(filtered -n git cat-file -p "$obj")
    done < <(listing history_blobs)
    ;;
esac

if [ "$COUNTED" = 1 ]; then
  if history_verdict "$findings" "$EXPECTED"; then
    echo "check-sensitive-metadata: clean (history) — $findings known historic finding(s), as SENSITIVE_HISTORY_EXPECTED says"
    exit 0
  fi
  printf '%s' "$held"
  printf '\n%s findings in the whole history; SENSITIVE_HISTORY_EXPECTED is %s.\n' "$findings" "$EXPECTED"
  if [ "$findings" -gt "$EXPECTED" ]; then
    printf 'Something new reached history. Find it among the findings above, and redact it before it is pushed.\n'
  else
    printf 'History lost findings, so it was rewritten or the denylist changed. Check that, then set the new count.\n'
  fi
  exit 1
fi

if [ "$findings" -gt 0 ]; then
  printf '\n%s findings. Nothing was committed.\n' "$findings"
  printf 'Redact the value — do NOT base64 it; rule 4 decodes that.\n'
  exit 1
fi
echo "check-sensitive-metadata: clean (${MODE})"
