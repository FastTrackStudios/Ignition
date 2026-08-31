# The dx toolchain trio (fills fts.dx.* and fts.pkgsDx).
#
# Dedicated, current-unstable nixpkgs used ONLY to source `dx`
# (dioxus-cli) at the version the workspace Cargo.lock pins (0.7.9)
# plus binaryen 129 (the wasm-opt dx 0.7.9 expects). The main
# `nixpkgs` (dioxus-flake's pin) carries dioxus-cli 0.7.4 / binaryen
# 126, which dx rejects / SIGABRTs with.
#
# The SAME trio serves the dev shell and the hermetic web bundles —
# previously the shell carried nixpkgs' 0.7.4 and cargo-installed 0.7.9
# into ~/.cargo/bin on every version drift (the "reinstalling dioxus"
# churn). Now dx comes from the store, prebuilt and cached.
{ inputs, ... }:
{
  perSystem = { system, lib, ... }:
    let
      pkgsDx = import inputs.nixpkgs-dx { inherit system; };
    in
    {
      fts.pkgsDx = pkgsDx;
      # dx at the version the workspace tracks (dioxus 0.8 line). nixpkgs
      # carries 0.7.9; override src onto the published 0.8.0-alpha.0
      # crate. Bump together with the dioxus git rev in root Cargo.toml.
      fts.dx.cli = pkgsDx.dioxus-cli.overrideAttrs (old: rec {
        version = "0.8.0-alpha.0";
        # static.crates.io, NOT fetchCrate: nixpkgs' fetchCrate builds the
        # legacy `crates.io/api/v1/crates/<c>/<v>/download` URL, which now
        # answers 403 to the fetcher. The crate is fine and unyanked — only
        # that endpoint is gone. static.crates.io serves the identical
        # tarball, so the hash below is unchanged. This 403 is what broke
        # EVERY iOS build (the devshell can't even evaluate without dx).
        src = pkgsDx.fetchzip {
          name = "dioxus-cli-${version}";
          url = "https://static.crates.io/crates/dioxus-cli/dioxus-cli-${version}.crate";
          hash = "sha256-gEC5MtvkTBAhv2ChvWPQIx4u/OJ5Qx2sN2+epdcXwSA=";
          extension = "tar.gz";
        };
        cargoDeps = pkgsDx.rustPlatform.fetchCargoVendor {
          inherit src;
          name = "dioxus-cli-${version}-vendor";
          hash = "sha256-znRYZFhWP5PzS6ftcShzNBvRqJXRjnM10OZ+KzUOOsg=";
        };
        # 0.7.9-era patches/checks don't apply to the alpha.
        patches = [ ];
        doCheck = false;
        doInstallCheck = false;
      });
      fts.dx.binaryen = pkgsDx.binaryen;

      # wasm-bindgen-cli at the EXACT version the workspace Cargo.lock
      # pins for the `wasm-bindgen` crate (0.2.127): `dx build --platform
      # web` checks the pair at startup and refuses a CLI one patch off.
      # This stood at 0.2.126 while the lock said 0.2.127, which is a
      # refusal, not a warning. Bump together with the `wasm-bindgen`
      # line in Cargo.lock.
      #
      # `buildWasmBindgenCli` rather than a bare `buildRustPackage`:
      # nixpkgs' own builder carries the crate's quirks (its workspace
      # layout, the tests it cannot run sandboxed). Fetched from
      # static.crates.io for the same reason `dx` above is — nixpkgs'
      # `fetchCrate` builds the legacy download URL, which now 403s.
      # `fetchCrate` is itself a `fetchzip` of that tarball, so the
      # hash is the same either way.
      fts.dx.wasmBindgen = pkgsDx.buildWasmBindgenCli rec {
        # Explicit: the builder otherwise defaults `version` to
        # `src.version`, which `fetchCrate` sets and a plain `fetchzip`
        # does not.
        version = "0.2.127";
        src = pkgsDx.fetchzip {
          name = "wasm-bindgen-cli-${version}";
          url = "https://static.crates.io/crates/wasm-bindgen-cli/wasm-bindgen-cli-${version}.crate";
          hash = "sha256-di+qBAdd7pENLiIB9CoZoab+W5xeDoByMREcCGTSzWo=";
          extension = "tar.gz";
        };
        cargoDeps = pkgsDx.rustPlatform.fetchCargoVendor {
          inherit src;
          pname = "wasm-bindgen-cli";
          inherit version;
          hash = "sha256-FTv2GZIAQs0ePdIZXIXil7JbZ6kIT05VG6vqC1qNFxQ=";
        };
      };
    };
}
