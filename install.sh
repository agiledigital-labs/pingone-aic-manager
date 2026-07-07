#!/usr/bin/env bash
# install.sh — install or update `aic` (pingone-aic-manager).
#
# Primary path: download the prebuilt Linux binary from the latest GitHub
# Release and drop it in ~/.local/bin. Falls back to `cargo install` when a
# prebuilt binary isn't available for your platform (or when you ask for it).
#
# Quick start (Ubuntu / WSL):
#   curl -fsSL https://raw.githubusercontent.com/agiledigital-labs/pingone-aic-manager/main/install.sh | bash
#
# Re-running installs the newest release (i.e. it doubles as an updater).
#
# Environment / flags:
#   AIC_VERSION=0.2.0     install a specific version instead of the latest
#   AIC_INSTALL_DIR=DIR   install location (default: ~/.local/bin)
#   GITHUB_TOKEN=...       used for the GitHub API to avoid rate limits (optional)
#   --from-source          skip the prebuilt binary; build with `cargo install`
#   --help                 show this help
set -euo pipefail

REPO="agiledigital-labs/pingone-aic-manager"
CRATE="pingone-aic-manager"
BIN="aic"
TARGET="x86_64-unknown-linux-gnu"
INSTALL_DIR="${AIC_INSTALL_DIR:-$HOME/.local/bin}"
FROM_SOURCE=0

for arg in "$@"; do
  case "$arg" in
    --from-source) FROM_SOURCE=1 ;;
    -h | --help)
      sed -n '2,26p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "install.sh: unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

info() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
err() {
  printf '\033[1;31merror:\033[0m %s\n' "$*" >&2
  exit 1
}
have() { command -v "$1" >/dev/null 2>&1; }

# --- source build fallback ---------------------------------------------------
install_from_source() {
  info "Installing $CRATE from source with cargo…"
  if ! have cargo; then
    err "cargo not found. Install Rust from https://rustup.rs then re-run, \
or install a prebuilt release manually from https://github.com/$REPO/releases"
  fi
  local args=(install "$CRATE" --force)
  if [ -n "${AIC_VERSION:-}" ]; then
    args+=(--version "$AIC_VERSION")
  fi
  cargo "${args[@]}"
  info "Installed via cargo to $(dirname "$(command -v "$BIN" || echo "$HOME/.cargo/bin/$BIN")")"
  exit 0
}

if [ "$FROM_SOURCE" -eq 1 ]; then
  install_from_source
fi

# --- platform check ----------------------------------------------------------
os="$(uname -s)"
arch="$(uname -m)"
if [ "$os" != "Linux" ] || { [ "$arch" != "x86_64" ] && [ "$arch" != "amd64" ]; }; then
  warn "No prebuilt binary for ${os}/${arch}; falling back to a source build."
  install_from_source
fi

for tool in curl tar sha256sum; do
  have "$tool" || err "required tool '$tool' not found on PATH"
done

# --- resolve the release + asset URL -----------------------------------------
api="https://api.github.com/repos/$REPO/releases"
if [ -n "${AIC_VERSION:-}" ]; then
  api="$api/tags/v${AIC_VERSION#v}"
else
  api="$api/latest"
fi

curl_gh() {
  if [ -n "${GITHUB_TOKEN:-}" ]; then
    curl -fsSL -H "Authorization: Bearer $GITHUB_TOKEN" "$@"
  else
    curl -fsSL "$@"
  fi
}

info "Looking up release ($REPO)…"
release_json="$(curl_gh "$api")" ||
  err "could not fetch release metadata from $api"

# Pull the browser_download_url for the tarball matching our target triple.
# Portable extraction (no jq dependency).
asset_url="$(printf '%s\n' "$release_json" |
  grep -o '"browser_download_url": *"[^"]*"' |
  sed 's/.*"browser_download_url": *"//;s/"$//' |
  grep -E "aic-[^/]*-${TARGET}\.tar\.gz$" |
  head -n1 || true)"

if [ -z "$asset_url" ]; then
  warn "no prebuilt asset for $TARGET in that release; falling back to source."
  install_from_source
fi

# --- download + verify + install ---------------------------------------------
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
tarball="$tmp/$(basename "$asset_url")"

info "Downloading $(basename "$asset_url")…"
curl_gh -o "$tarball" "$asset_url"

# Checksum is best-effort: verify when the .sha256 sidecar is present.
if curl_gh -o "$tarball.sha256" "$asset_url.sha256" 2>/dev/null; then
  info "Verifying checksum…"
  ( cd "$tmp" && sha256sum -c "$(basename "$tarball").sha256" >/dev/null ) ||
    err "checksum verification failed — refusing to install"
else
  warn "no checksum sidecar found; skipping verification"
fi

info "Extracting…"
tar -xzf "$tarball" -C "$tmp"
[ -f "$tmp/$BIN" ] || err "archive did not contain a '$BIN' binary"

mkdir -p "$INSTALL_DIR"
install -m 0755 "$tmp/$BIN" "$INSTALL_DIR/$BIN"
info "Installed $BIN to $INSTALL_DIR/$BIN"

# --- PATH hint + version -----------------------------------------------------
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    warn "$INSTALL_DIR is not on your PATH. Add this to your shell profile:"
    printf '    export PATH="%s:$PATH"\n' "$INSTALL_DIR" >&2
    ;;
esac

if ver="$("$INSTALL_DIR/$BIN" --version 2>/dev/null)"; then
  info "Done — $ver"
else
  info "Done. Run '$BIN --help' to get started."
fi
