{
  description = "Ignition — DMX/Art-Net/sACN console + Bevy 3D visualizer + projection mapping";

  # Deliberately a plain flake, not the dendritic `den` layout the
  # FastTrackStudio repo uses. That layout exists there to share typed
  # `fts.*` options across ~250 workspace members and a system flake;
  # Ignition is four crates and one dev shell, and a single readable file
  # beats a module tree it would not use.
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # A dedicated nixpkgs used ONLY to source `dx` (dioxus-cli). The
    # main pin carries an older one, and dx refuses to build a tree
    # whose dioxus is a different minor — the version pair is checked at
    # startup, not discovered at link time. Same pin the FastTrackStudio
    # tree uses, for the same reason.
    nixpkgs-dx.url = "github:NixOS/nixpkgs/d99b013d5d1931ad77fe3912ed218170dec5d9a4";
  };

  outputs = { self, nixpkgs, nixpkgs-dx, rust-overlay }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      }));

      # `dx` at the version this tree's dioxus rev pins (0.8.0-alpha.0).
      # nixpkgs carries 0.7.9, which refuses to serve a 0.8 tree, so the
      # source is overridden onto the published alpha crate. Bump this
      # together with the dioxus rev in `apps/ignition-studio/Cargo.toml`
      # — they are checked against each other at `dx serve` startup.
      dxFor = system:
        let pkgsDx = import nixpkgs-dx { inherit system; };
        in pkgsDx.dioxus-cli.overrideAttrs (old: rec {
          version = "0.8.0-alpha.0";
          src = pkgsDx.fetchCrate {
            pname = "dioxus-cli";
            inherit version;
            hash = "sha256-gEC5MtvkTBAhv2ChvWPQIx4u/OJ5Qx2sN2+epdcXwSA=";
          };
          cargoDeps = pkgsDx.rustPlatform.fetchCargoVendor {
            inherit src;
            name = "dioxus-cli-${version}-vendor";
            hash = "sha256-znRYZFhWP5PzS6ftcShzNBvRqJXRjnM10OZ+KzUOOsg=";
          };
          # The 0.7.9-era patches and checks do not apply to the alpha.
          patches = [ ];
          doCheck = false;
          doInstallCheck = false;
        });
    in
    {
      devShells = forAllSystems (pkgs:
        let
          inherit (pkgs) lib stdenv;

          # Bevy 0.19 raises its MSRV to Rust 1.95, which is ahead of the
          # 1.94 the FastTrackStudio toolchain pins — the reason this repo
          # has a flake of its own at all. `latest` rather than a hard pin
          # so a Bevy MSRV bump is a `nix flake update`, not an edit here;
          # the lockfile is what actually makes it reproducible.
          rust = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
            targets = [ "wasm32-unknown-unknown" ];
          };

          # Bevy's Linux build/run dependencies, per its own
          # docs/linux_dependencies.md: ALSA and udev are build-time
          # (bevy_audio, gilrs), the rest are loaded at runtime by wgpu
          # and winit and so have to be on the library path too.
          linuxBuildInputs = with pkgs; [
            alsa-lib
            udev
            vulkan-loader
            libxkbcommon
            wayland
            libx11
            libxcursor
            libxi
            libxrandr
            # Blitz's text stack (yeslogic-fontconfig-sys) links
            # fontconfig, and fontconfig wants freetype. Only the studio
            # app needs these — the visualizer alone does not — but the
            # dev shell is shared.
            fontconfig
            freetype
            # The standalone DAW backend's audio output. PipeWire is its
            # default engine on Linux, and `libspa-sys` builds against
            # the headers rather than dlopening — so this is needed to
            # compile, not only to run.
            pipewire
            # `dx serve` turns on the full `dioxus/native` feature set,
            # which brings Blitz's networking (remote images, devtools)
            # and therefore `openssl-sys`. A plain `cargo build` of the
            # studio app does not need this — the features it selects are
            # narrower — so this only shows up under dx.
            openssl
          ];
        in
        {
          default = pkgs.mkShell {
            packages = [
              rust
              pkgs.pkg-config
              # mold — same linker choice as the FastTrackStudio tree, for
              # the same reason: Bevy links a lot of object code and the
              # default linker dominates incremental build time.
              pkgs.mold
              pkgs.cargo-nextest
              # `dx serve` for the studio app. Native renderer only —
              # the visualizer is composited through Blitz's wgpu
              # device, which a webview does not have.
              (dxFor pkgs.stdenv.hostPlatform.system)
              # Tailwind, for `dx serve`'s built-in pipeline. It would
              # otherwise download a standalone binary into ~/.cache,
              # which will not run on NixOS — `NO_DOWNLOADS=1` sends it
              # to `which tailwindcss` instead. See the Justfile.
              pkgs.tailwindcss_4
              # The operator overlay's font. Bevy's built-in default is a
              # small subset with no box-drawing or symbol glyphs, so a
              # cue sheet drawn with it renders a column of tofu where
              # the cooked-status markers should be.
              pkgs.dejavu_fonts
            ] ++ lib.optionals stdenv.hostPlatform.isLinux linuxBuildInputs;

            # Read by `crates/ignition-viz/build.rs`, which copies the
            # file into OUT_DIR so it ends up *embedded in the binary*
            # rather than referenced by store path. A nix store path is
            # machine-specific; a visualizer that only draws its own UI
            # inside the dev shell would be a bad trade for one font.
            IGNITION_OVERLAY_FONT =
              "${pkgs.dejavu_fonts}/share/fonts/truetype/DejaVuSansMono.ttf";

            # wgpu dlopen's the Vulkan loader and winit dlopen's the
            # Wayland/xkb clients, so they must be findable at run time,
            # not just at link time.
            LD_LIBRARY_PATH = lib.optionalString stdenv.hostPlatform.isLinux
              (lib.makeLibraryPath linuxBuildInputs);

            # `libspa-sys` generates its bindings with bindgen, which
            # dlopens libclang and finds it *only* through this variable
            # on NixOS — there is no system path to fall back to. The
            # failure is a build-script panic several hundred lines into
            # pkg-config noise, so it reads as a PipeWire problem and is
            # not.
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

            # ...and finding libclang is only half of it. bindgen drives
            # libclang *directly*, not through the `clang` wrapper script
            # that normally injects the include paths, so it starts with
            # an empty system include path. There is no `/usr/include` on
            # NixOS to fall back to.
            #
            # The symptom is bizarre enough to be worth naming: clang
            # reports that its own `inttypes.h` cannot find `inttypes.h`.
            # It is not confused — that header ends in `#include_next`,
            # handing off to libc's copy of the same name, and it is the
            # handoff that fails. Both halves have to be on the path: the
            # compiler's own resource-dir headers, then libc's.
            BINDGEN_EXTRA_CLANG_ARGS = builtins.concatStringsSep " " [
              "-isystem ${pkgs.llvmPackages.libclang.lib}/lib/clang/${lib.versions.major pkgs.llvmPackages.libclang.version}/include"
              "-isystem ${pkgs.stdenv.cc.libc.dev}/include"
            ];
          };
        });
    };
}
