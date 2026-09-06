# The dx web bundle for the public site (ignition-web) + its static-site
# OCI image.
#
# `dx bundle` → $out/www, brotli pre-compressed, wrapped by
# `fts.mkStaticSite` into a static-web-server image the cluster runs.
# Mirrors keyflow's and task's — same dx, same crane, same serving shape
# — because the three sites are deployed by the same pipeline and a
# second way of doing it is a second thing to debug at 3am.
#
# This is the FIRST image this repo ships. crane was already carried "for
# parity with the sibling repos even though Ignition ships no image yet"
# (nix/modules/crane.nix); this is the yet.
#
# Only `apps/ignition-web` is built, not the workspace: the site takes
# dioxus, view-knowledge-graph and a markdown parser, and none of Bevy,
# wgpu, Blitz or reaper-rs. The vendor dir is still the whole workspace's
# — there is one Cargo.lock — but vendoring is a download, not a build.
{ ... }:
{
  perSystem =
    { pkgs, lib, config, ... }:
    let
      inherit (config.fts) craneLib commonArgs mkStaticSite;

      # `ring` compiles C to wasm32, and the nix cc-wrapper injects
      # host-only hardening flags clang rejects for that target. Get it
      # wrong and NOTHING FAILS: the C symbols stay in the module as
      # unresolved `(import "env" "ring_core_…")` entries, wasm-bindgen
      # emits `import * as import1 from "env"` for them, and the browser
      # dies on "Failed to resolve module specifier env" — a white page,
      # from a build that exited 0.
      #
      # The dev shell already solves this (`fts.shellEnv`, and the
      # comment there is about exactly this bundle). Taking those three
      # keys from it rather than restating them is what stops the shell
      # and the hermetic build from drifting into disagreement — the
      # local build worked while this one white-screened precisely
      # because they had.
      dxWebEnv = {
        # Hermetic dx: the sandbox has no network, so `NO_DOWNLOADS` makes
        # dx resolve wasm-opt / wasm-bindgen from PATH rather than
        # fetching them from GitHub.
        NO_DOWNLOADS = "1";
      }
      // lib.getAttrs [
        "CC_wasm32_unknown_unknown"
        "AR_wasm32_unknown_unknown"
        "CFLAGS_wasm32_unknown_unknown"
      ] config.fts.shellEnv;

      dxWebNativeInputs =
        commonArgs.nativeBuildInputs
        ++ [
          config.fts.dx.cli
          config.fts.dx.wasmBindgen
          config.fts.dx.binaryen
        ]
        ++ (with pkgs; [
          brotli
          # The compiler and archiver `dxWebEnv` names above.
          llvmPackages_18.clang-unwrapped
          llvmPackages_18.bintools-unwrapped
        ]);

      ignition-webapp = craneLib.buildPackage (
        commonArgs
        // dxWebEnv
        // {
          pname = "ignition-webapp";
          version = "0.1.0";
          cargoArtifacts = null;
          cargoExtraArgs = "--manifest-path apps/ignition-web/Cargo.toml";
          nativeBuildInputs = dxWebNativeInputs;
          doNotPostBuildInstallCargoBinaries = true;

          # No tailwind step here, deliberately. `assets/tailwind.css` is
          # COMMITTED (see apps/ignition-web/tailwind.css for why) and so
          # travels with the source; regenerating it would need the
          # git-dep checkouts resolved inside the sandbox for no gain.
          # `just tailwind` is what refreshes it, and CI runs that.
          buildPhaseCargoCommand = ''
            export HOME="$TMPDIR/dx-home"
            mkdir -p "$HOME"

            # No demo data, and no `--features demo`: the shipped image
            # is the landing page and the guide. `/demo` links Bevy and
            # wgpu — 37 MB of wasm against 1.1 MB without — which is both
            # a heavy download and a heavy build. See the `demo` feature
            # in apps/ignition-web/Cargo.toml for what wants doing before
            # it comes back.
            # --debug-symbols false: drops DWARF. Not only a size win —
            # with DWARF present wasm-opt ABORTS on a version mismatch,
            # and dx logs the SIGABRT and ships the unoptimised module
            # anyway. That is 2.7 MB of wasm instead of 903 KB, and it
            # fails quietly.
            # `--ssg --fullstack --force-sequential`, exactly as
            # `just site` does, and NOT optional: this crate's `web`
            # feature turns on `dioxus-web/hydrate`, so the client that
            # ships expects a pre-rendered DOM and the hydration data
            # that comes with it. Build it without `--ssg` and the wasm
            # loads, calls `atob` on hydration data that was never
            # written, throws `InvalidCharacterError`, and mounts
            # nothing — A WHITE PAGE, from an image that built and
            # deployed and passed its own liveness check, because
            # `/version.json` is a static file and answers either way.
            #
            # That is exactly what shipped between the SSG change and
            # this comment. `--fullstack` is what makes dx build a
            # server at all (this crate keeps `dioxus/fullstack` on its
            # `server` feature alone), and without it `--ssg` silently
            # does nothing. `--force-sequential` because the pre-render
            # borrows `public/index.html` while the client build is
            # writing it (dioxus#3518).
            dx build -p ignition-web --platform web --release --debug-symbols false \
                --ssg --fullstack --force-sequential
          '';

          installPhaseCommand = ''
            mkdir -p $out/www
            cp -R target/dx/ignition-web/release/web/public/. $out/www/
            # `sitemap.xml` and `rss.xml` are written by build.rs into a
            # gitignored directory. They cannot go through `asset!` —
            # that content-hashes what it touches, and `/sitemap.xml` is
            # the fixed URL every crawler looks for — so they are copied
            # in at the top level, the same as `just site` does.
            cp apps/ignition-web/generated/*.xml $out/www/ 2>/dev/null || true
            # Pre-compress so static-web-server's `compression-static`
            # can serve .br — the wasm is the bulk of the bundle.
            find $out/www -type f \( -name '*.wasm' -o -name '*.js' \
              -o -name '*.css' -o -name '*.html' -o -name '*.json' \
              -o -name '*.svg' \) -exec brotli --keep --quality=9 {} +
          '';

          doCheck = false;
        }
      );
    in
    {
      packages =
        { inherit ignition-webapp; }
        // lib.optionalAttrs pkgs.stdenv.isLinux {
          ignition-web-image = mkStaticSite {
            name = "ignition-web";
            siteRoot = "${ignition-webapp}/www";
          };
        };
    };
}
