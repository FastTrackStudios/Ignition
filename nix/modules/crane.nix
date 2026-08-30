# crane plumbing shared by every deployable package (fills fts.craneLib,
# fts.src, fts.commonArgs, fts.buildRev).
{ inputs, self, ... }:
{
  perSystem = { pkgs, lib, config, ... }:
    let
      # The whole monorepo is the build source: ONE workspace, so the
      # root Cargo.lock drives vendoring, and the [patch.crates-io]
      # path patches (libs/vendor/styx-format, libs/editor/vendor/
      # mermaid-rs-renderer) resolve in-tree. Keep the filter minimal
      # — the flake source is already the tracked git tree (no
      # target/, no untracked junk); we only strip obvious non-build
      # dirs to cut store-copy churn.
      ftsSrc = lib.cleanSourceWith {
        src = ../..;
        filter = path: type:
          let name = builtins.baseNameOf (toString path); in
          !(builtins.elem name [ "target" "node_modules" ".git" "result" "web-dist" ]);
      };

      craneLib = (inputs.crane.mkLib pkgs).overrideToolchain config.fts.rustToolchain;

      # reaper-low (git dep on codeberg.org/FastTrackStudios/reaper-rs)
      # mounts justinfrankel/WDL as a git submodule at main/low/lib/WDL.
      # crane's git-dep vendoring lists files via `cargo package -l`,
      # which walks git-TRACKED files from the superproject's index —
      # a submodule's content isn't in that index (only its gitlink is),
      # so the vendored reaper-low ends up with an empty/partial lib/WDL
      # and its build.rs's own live `git submodule update` then fails
      # silently in the network-off nix sandbox ("no such file or
      # directory: lib/WDL/WDL/projectcontext.cpp"). Fetch the exact
      # submodule commit (pinned via reaper-rs's .gitmodules) and inject
      # it post-vendor instead.
      wdlSrc = pkgs.fetchFromGitHub {
        owner = "justinfrankel";
        repo = "WDL";
        rev = "40df69c0b9124ac433347f31c3f467054a8bfde8";
        hash = "sha256-sOVDXTNydgpetvC1D+DBF1XmR6k6KZbQ9mXKPOBPuTg=";
      };

      # Vendor against the root Cargo.lock. (The old baseview fetch
      # override is gone: the dead Codys-Wright/baseview.git dep was
      # vendored into libs/vendor/baseview as a path dep 2026-07-16.)
      cargoVendorDir = craneLib.vendorCargoDeps {
        src = ftsSrc;

        # libspa-sys needs bindgen's `clang_macro_fallback()` to evaluate
        # cast-expression C macros its normal cexpr parser can't fold —
        # notably SPA_ID_INVALID (`((uint32_t)0xffffffff)`). That fallback
        # writes a scratch `.macro_eval.c` + `*-precompile.h.pch` into
        # `clang_macro_fallback_build_dir`, which DEFAULTS TO THE PROCESS
        # CWD — and cargo runs a build script with CWD set to the crate's
        # own source dir. Under nix that dir is the vendored copy in the
        # read-only store (dr-xr-xr-x), so the write fails; bindgen then
        # swallows it (`tu.save(&pch).ok()?` in ir/context.rs) and
        # SILENTLY disables macro fallback, emitting bindings with
        # SPA_ID_INVALID simply absent. The failure only surfaces much
        # later, as `libspa` failing to compile with a baffling
        # "cannot find value SPA_ID_INVALID in crate spa_sys".
        #
        # Ordinary `cargo build` never hits this: ~/.cargo/registry/src
        # is user-writable, so the scratch files land next to the crate
        # and everything works — which is why this reproduced ONLY inside
        # nix and looked, misleadingly, like a feature-unification bug.
        #
        # Point the fallback at OUT_DIR (where scratch build artifacts
        # belong) instead. Upstream should arguably do this itself.
        #
        # Both pipewire-rs `-sys` crates have it (libspa-sys drops
        # SPA_ID_INVALID, pipewire-sys drops PW_ID_ANY). The injected
        # expression is deliberately self-contained rather than reusing
        # each crate's `out_path` local: in pipewire-sys the
        # `.clang_macro_fallback()` call site comes *before* `out_path`
        # is even declared, so referencing it wouldn't compile.
        overrideVendorCargoPackage = p: drv:
          if p.name == "libspa-sys" || p.name == "pipewire-sys" then
            drv.overrideAttrs (old: {
              postInstall = (old.postInstall or "") + ''
                substituteInPlace $out/build.rs \
                  --replace-fail \
                    '.clang_macro_fallback()' \
                    '.clang_macro_fallback().clang_macro_fallback_build_dir(::std::path::PathBuf::from(::std::env::var("OUT_DIR").unwrap()))'
              '';
            })
          else
            drv;

        overrideVendorGitCheckout = ps: drv:
          if lib.any (p: p.name == "reaper-low") ps then
            drv.overrideAttrs (old: {
              postInstall = (old.postInstall or "") + ''
                for d in $out/reaper-low-*; do
                  rm -rf "$d/lib/WDL"
                  cp -r ${wdlSrc} "$d/lib/WDL"
                  chmod -R u+w "$d/lib/WDL"
                done
              '';
            })
          else
            drv;
      };
    in
    {
      fts.craneLib = craneLib;
      fts.src = ftsSrc;

      fts.commonArgs = {
        src = ftsSrc;
        inherit cargoVendorDir;
        strictDeps = true;
        # mold: the repo's `.cargo/config.toml` pins
        # `-C link-arg=-fuse-ld=mold` for x86_64-linux (see the comment
        # there). That flag applies to nix builds too, and the sandbox has
        # no devshell — without mold on PATH, gcc's collect2 dies with
        # "cannot find 'ld'" on the very first build script. Ship the
        # linker the flag asks for rather than fighting the flag.
        nativeBuildInputs = with pkgs; [ pkg-config ]
          ++ lib.optional stdenv.isLinux mold;
        buildInputs = with pkgs; [ openssl ];
      };

      # Git rev baked into the deployable images so a running
      # deployment can say WHICH commit it serves (version.json /
      # TASK_BUILD_REV). Only the cheap wrapper layers depend on it —
      # the expensive cargo/wasm derivations stay rev-free and cached.
      fts.buildRev = self.rev or self.dirtyRev or "unknown";
    };
}
