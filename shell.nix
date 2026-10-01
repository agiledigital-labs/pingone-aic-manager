# Build environment for pingone-aic-manager.
#
# The yubikey USB-HID dependency chain (ctap-hid-fido2 → hidapi-rs) needs
# `libudev` discoverable via pkg-config at build time. Stock NixOS doesn't
# put it in PKG_CONFIG_PATH, so this shell brings it in along with the
# Rust toolchain.
#
# Usage:
#   nix-shell           # one-shot subshell
#   direnv allow .      # automatic if you `use nix` from .envrc

let
  # The Rust toolchain comes from rust-toolchain.toml, not from nixpkgs, so the
  # dev shell builds with exactly what CI builds with. rust-overlay is pinned;
  # bump it when rust-toolchain.toml names a version its manifests lack.
  rust-overlay = import (builtins.fetchTarball {
    url = "https://github.com/oxalica/rust-overlay/archive/ed3a19fd0439ed618ec5fe1e12f0ba69a8be38b5.tar.gz";
    sha256 = "15fg8h0q986jklrvvwni9igydg7xxppg0qrbvil9h2pisaf3iyrl";
  });
in
{ pkgs ? import <nixpkgs> { overlays = [ rust-overlay ]; } }:

let
  rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
in
pkgs.mkShell {
  nativeBuildInputs = [
    pkgs.pkg-config
    # rustc, cargo, clippy, rustfmt and rust-src at the pinned version.
    rust
    pkgs.rust-analyzer
    # The rhino-local harness's JVM. AM runs Temurin 25; the harness refuses
    # any other Java feature release (packages/rhino-local/src/jvm.ts).
    pkgs.temurin-bin-25
    pkgs.nodejs_24
  ];

  buildInputs = [
    # libudev for hidapi-rs USB device enumeration. `systemd` provides both
    # the runtime .so and the .pc file pkg-config looks for.
    pkgs.systemd
  ];

  # rust-analyzer needs the rust source tree to surface std-lib docs.
  RUST_SRC_PATH = "${rust}/lib/rustlib/src/rust/library";

  shellHook = ''
    echo "pingone-aic-manager dev shell ready — pkg-config can see: $(pkg-config --list-all | grep -i udev | head -1 | cut -d' ' -f1)"
  '';
}
