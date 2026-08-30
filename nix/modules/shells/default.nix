# Dev Shell — one shell for the whole workspace. Toolchain + native/
# audio/wasm env so `cargo` / `dx` behave identically for the rig,
# the web remotes, and THE app.
#
# dx comes from the store (fts.dx — the nixpkgs-dx 0.7.9 pin, same trio
# the hermetic web bundles use). The old `cargo install dioxus-cli`
# shellHook is gone — that was the "reinstalling all of dioxus every
# time" churn: the shell shipped nixpkgs' 0.7.4, the hook detected the
# mismatch and rebuilt 0.7.9 from source into ~/.cargo/bin.
{ ... }:
{
  perSystem = { pkgs, lib, config, ... }: {
    devShells.default = pkgs.mkShell ({
      packages = (with pkgs; [
        # The command runner the whole repo drives through (just web-stage,
        # just rig-install, …). NixOS hosts have it system-wide, which masked
        # its absence here; nix-darwin (airlock) does not, so builds that shell
        # out to `just` failed there.
        just
        cargo-watch
        cargo-nextest
        bacon
        flac
        # uv — vehicle for the graphify bootstrap below (graphify is a
        # PyPI tool, not in nixpkgs; python3 comes from buildInputs).
        uv
      ])
      ++ [
        config.fts.rustToolchain
        config.fts.dx.cli          # dx 0.7.9 — prebuilt, no cargo install
        config.fts.dx.wasmBindgen  # wasm-bindgen-cli 0.2.126 (lock match)
        config.fts.dx.binaryen     # wasm-opt 129 — what dx 0.7.9 pins
      ]
      # cargo-rail — graph-aware workspace maintenance (unify / change /
      # release / split). Pinned upstream binary, NOT nixpkgs' stale
      # 0.7.0 — see nix/modules/cargo-rail.nix (incl. the plan/affected
      # caveat on this tree).
      ++ lib.optionals (config.fts.cargoRail != null) [ config.fts.cargoRail ]
      ++ lib.optionals pkgs.stdenv.isLinux [
        # pw-jack — run the live rigs through PipeWire's JACK shim.
        pkgs.pipewire.jack
        # jack_iodelay / jack_lsp — measure real round-trip latency.
        pkgs.jack-example-tools
        # Xvfb + xvfb-run — virtual display for the REAPER
        # integration-test harness (`just reaper-integration-test`).
        pkgs.xorg.xorgserver
        pkgs.xvfb-run
        # linuxdeploy — dx's AppImage bundler (`dx bundle --package-types
        # appimage`) shells out to it; without it packaging aborts. Not in the
        # flake's (older) nixpkgs pin, so take it from the current-unstable
        # nixpkgs-dx set that already provides the dx toolchain.
        config.fts.pkgsDx.linuxdeploy
      ]
      ++ config.fts.buildInputs
      ++ config.fts.nativeBuildInputs;

      # Playwright browsers for the editor web suite (libs/editor/tests).
      # pkgsDx's playwright-driver (1.59.1) matches the @playwright/test
      # pin in tests/package.json — keep them in lockstep. Without this
      # Playwright falls back to its own downloaded chromium, which can't
      # load shared libs on NixOS (libnspr4.so).
      PLAYWRIGHT_BROWSERS_PATH = config.fts.pkgsDx.playwright-driver.browsers;
      PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS = "true";

      shellHook = ''
        [ -f .env ] && { set -a; source .env; set +a; }

        # No RUSTC_WRAPPER. sccache used to be set here; it was removed
        # because what it bought did not cover what it cost. Measured on
        # this tree it gave 0% hits across worktrees (the target-dir path
        # is part of sccache's Rust cache key, and SCCACHE_BASEDIR makes
        # paths relative, not equal), so it never deduped the thing that
        # actually costs disk — its only real win was making a `just
        # sweep` cheap to recover from. Against that it breaks `dx`,
        # which drives rustc through its own wrapper binary that sccache
        # refuses ("Compiler not supported"), so every dx build and
        # `dx serve` had to unset it by hand.
        # Append (not prepend): store-provided tools (dx, wasm-bindgen)
        # must win over stale cargo-installed copies; cargo/uv bins
        # (tracey, graphify) only need to be reachable.
        export PATH="$PATH:$HOME/.cargo/bin:$HOME/.local/bin"

        # Dev-convenience installers (graphify, tracey) are for
        # interactive shells ONLY. In CI they're dead weight — and
        # worse: `cargo install tracey` compiled from source (or hung
        # on flaky crates.io egress, stderr silenced) on EVERY job,
        # stalling `nix develop -c true` for hours before the first
        # cargo step. Forgejo/GitHub runners set CI=true.
        if [ -z "''${CI:-}" ]; then
          # graphify — whole-repo knowledge-graph tool (safishamsi/graphify)
          # so any agent/assistant can understand this 160-crate tree without
          # grepping it cold. Not in nixpkgs; bootstrapped via uv, pinned for
          # reproducibility. Code extraction is 100% local (tree-sitter, no
          # API calls); the [mcp] extra lets `graphify serve` expose the
          # graph over MCP. uv tools shim into ~/.local/bin.
          GRAPHIFY_VERSION="0.9.15"
          if ! uv tool list 2>/dev/null | grep -q "graphifyy v$GRAPHIFY_VERSION"; then
            echo "  Installing graphify $GRAPHIFY_VERSION..."
            UV_PYTHON_DOWNLOADS=never uv tool install --force \
              --python "${pkgs.python3}/bin/python3" \
              "graphifyy[mcp]==$GRAPHIFY_VERSION" >/dev/null 2>&1 || \
              echo "  (graphify install failed — run: uv tool install 'graphifyy[mcp]==$GRAPHIFY_VERSION')"
          fi

          # tracey — spec-coverage CLI (crates.io). Version-pinned so the
          # requirement traceability in docs/spec/** is reproducible across
          # machines. Config: .config/tracey/config.styx.
          TRACEY_VERSION=$(tracey --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || echo "0")
          if [ "$TRACEY_VERSION" != "1.3.0" ]; then
            echo "  Installing tracey 1.3.0..."
            cargo install tracey --locked --version "=1.3.0" 2>/dev/null || true
          fi
        fi

        echo ""
        echo "  Ignition dev shell"
        echo "  ─────────────────────────────────────────────"
        echo "  just check / just test / just ci"
        echo "  cargo run -p ignition-engine — the lighting engine"
        echo "  cd apps/mobile && dx serve   — the Ignition app"
        echo "  (cd apps/mobile && ./ios/build-ios.sh)   — the iOS app"
        echo ""
        echo "  Rust: $(rustc --version)"
        echo "  dx:   $(dx --version 2>/dev/null || echo 'not available')"
        echo "  graphify: $(graphify --version 2>/dev/null || echo 'not available') — 'just graph' to (re)build the repo knowledge graph"
        echo ""
      '';
    }
    // config.fts.shellEnv);
  };
}
