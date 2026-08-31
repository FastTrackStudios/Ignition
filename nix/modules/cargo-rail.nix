# cargo-rail — graph-aware workspace maintenance (unify / plan / run /
# change / release / split). Built from source (nixpkgs carries 0.7.0,
# which predates the plan/change/release surface) with one patch:
#
#   nix/patches/cargo-rail-resolve-graph.patch — build dependency edges
#   from cargo's RESOLVE graph (package-ID-keyed) instead of manifest
#   dep names. Upstream v0.17.3 keys nodes by bare name over ALL
#   packages: crates.io's iroh dev-depends on a crate literally named
#   `patchbay`, which aliased our workspace crate and welded the whole
#   graph together (a leaf patchbay change "affected" 214 members).
#   Verified fixed 2026-07-16: patchbay .rs change → exactly
#   {patchbay, patchbay-ui, fts-patchbay}. Candidate for upstreaming.
#
# rust-version is relaxed 1.95 → 1.94: the crate compiles on the FTS
# toolchain era; the floor was a policy bump, not a feature need.
{ ... }:
{
  perSystem = { pkgs, lib, config, ... }:
    let
      pkgsDx = config.fts.pkgsDx;
      cargo-rail = pkgsDx.rustPlatform.buildRustPackage rec {
        pname = "cargo-rail";
        version = "0.17.3";
        src = pkgsDx.fetchFromGitHub {
          owner = "loadingalias";
          repo = "cargo-rail";
          rev = "v${version}";
          hash = "sha256-ROro3gyTGo38yzRi7c8UUsCZ1HqJVUD8pCYauh1H10s=";
        };
        cargoHash = "sha256-OuSc0bmXmdh2ZeENz1Ecbw0ALTX+HTv7XMiMy3iFcko=";
        patches = [ ../patches/cargo-rail-resolve-graph.patch ];
        postPatch = ''
          substituteInPlace Cargo.toml \
            --replace-fail 'rust-version = "1.95.0"' 'rust-version = "1.94.0"'
        '';
        doCheck = false;
        meta = {
          description = "Cargo-native monorepo control plane: unify, plan/run, change/release, split/sync";
          homepage = "https://github.com/loadingalias/cargo-rail";
          license = lib.licenses.mit;
          mainProgram = "cargo-rail";
        };
      };
    in
    {
      fts.cargoRail = cargo-rail;
    };
}
