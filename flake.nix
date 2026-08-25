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
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      }));
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
          };
        });
    };
}
