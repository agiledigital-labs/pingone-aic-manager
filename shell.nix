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

{ pkgs ? import <nixpkgs> { } }:

pkgs.mkShell {
  nativeBuildInputs = [
    pkgs.pkg-config
    pkgs.rustc
    pkgs.cargo
    pkgs.clippy
    pkgs.rustfmt
    pkgs.rust-analyzer
  ];

  buildInputs = [
    # libudev for hidapi-rs USB device enumeration. `systemd` provides both
    # the runtime .so and the .pc file pkg-config looks for.
    pkgs.systemd
  ];

  # rust-analyzer needs the rust source tree to surface std-lib docs.
  RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";

  shellHook = ''
    echo "pingone-aic-manager dev shell ready — pkg-config can see: $(pkg-config --list-all | grep -i udev | head -1 | cut -d' ' -f1)"
  '';
}
