# CI shell — the default shell minus every interactive convenience.
# No shellHook installers (the dx / graphify / tracey cargo-installs
# stalled CI jobs for HOURS compiling from source on cache misses),
# no dioxus-cli / editor tooling, no .env sourcing. Toolchain + native
# headers + the env the build scripts need, nothing else. Workflows
# enter it via `nix develop .#ci`.
#
# tailwindcss IS here, despite "CI drives plain cargo": the Task apps'
# assets/tailwind.css is gitignored build output that `asset!()` demands
# at compile time. `dx` generates it during a normal dx build, but CI
# drives plain cargo, so the workflow runs `just css` first — which
# needs this binary. It is a single prebuilt
# store path, not a from-source cargo-install, so it does not
# reintroduce the stall described above.
{ ... }:
{
  perSystem = { pkgs, lib, config, ... }: {
    devShells.ci = pkgs.mkShell ({
      packages = [ config.fts.rustToolchain pkgs.cargo-nextest ]
      # cargo-rail — release/unify automation (`cargo rail unify --check`
      # as a hygiene gate). NOT used for CI change-selection yet: its
      # plan/affected walk over-selects on this workspace — see
      # nix/modules/cargo-rail.nix. Store-sourced only (never installed
      # from a hook — see the stall note above).
      ++ lib.optionals (config.fts.cargoRail != null) [ config.fts.cargoRail ]
      ++ config.fts.buildInputs
      # The shared native tool list (pkg-config, bindgen, tailwindcss —
      # and on Linux, mold: .cargo/config.toml selects it for every
      # x86_64-linux link, so a shell without it can't link at all).
      # Consumed from toolchain.nix instead of hand-repeating entries
      # here, so the two shells can't drift again.
      ++ config.fts.nativeBuildInputs
      # `just` so the workflow can invoke `just css` instead of
      # repeating the tailwindcss commands — the recipe stays the one
      # definition of how those sheets get built.
      ++ [
        pkgs.just
      ];

      # Seeded cargo-home bins (dx for the web-bundle steps) resolve
      # from PATH — never installed from here. APPEND, don't prepend:
      # GitHub-hosted runners ship a rustup stable in ~/.cargo/bin that
      # would otherwise shadow the pinned nix rustc (first symptom:
      # "can't find crate for core" on the wasm target).
      shellHook = ''
        export PATH="$PATH:$HOME/.cargo/bin"
      '';
    }
    // config.fts.shellEnv);
  };
}
