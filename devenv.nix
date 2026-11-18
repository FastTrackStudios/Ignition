# Ignition's dev environment (devenv 2.4): `devenv shell`, or direnv through
# .envrc. devenv-fleet's modules (devenv.yaml) give the toolchain (rust 1.99.1
# with wasm32, and on macOS the iOS and Intel targets; the rust-toolchain.toml
# pin), mold, cargo-hakari, cargo-rail, prek, just, `fleet-doctor`, dx,
# binaryen, wasm-bindgen, tailwindcss, the wasm32 C toolchain, the iOS linkers
# and the macOS library path. This file adds what the workspace links against
# and runs: the flake's shell (nix/modules/toolchain.nix) without what the
# fleet provides, as Studio's devenv.nix is Studio's flake shell. The flake's
# `nix develop .#ci` (CI) takes the same toolchain from devenv-fleet: keep the
# two package lists in step.
{ pkgs, lib, ... }:
let
  # phon-jit's copy-and-patch stencils chain via `become`, a nightly feature.
  # Darwin only: the native stencil backend compiles only on macOS-aarch64.
  # Studio's pinned nightly, so the same store path as Studio's shell.
  nightlyDate = "2026-04-05";
  rustNightly = pkgs.rust-bin.nightly.${nightlyDate}.minimal;

  # cargo-nightly — an explicit door to a nightly with rust-src and wasm, for
  # the one build stable cannot do (wasm threads need -Z build-std, `just
  # keys-worklet-wasm-threads`). Reached by name, never on PATH as `cargo`.
  rustNightlyWasm = pkgs.rust-bin.nightly.${nightlyDate}.minimal.override {
    extensions = [ "rust-src" ];
    targets = [ "wasm32-unknown-unknown" ];
  };
  cargoNightly = pkgs.writeShellScriptBin "cargo-nightly" ''
    export RUSTC="${rustNightlyWasm}/bin/rustc"
    exec ${rustNightlyWasm}/bin/cargo "$@"
  '';

  linux = with pkgs; lib.optionals stdenv.isLinux [
    alsa-lib
    # hidapi's build script pkg-configs libudev (kontrol's raw USB HID).
    udev
    # ort (load-dynamic) dlopens libonnxruntime at run time (session-guide/tts).
    onnxruntime
    # cpal's jack and pipewire features.
    libjack2
    pipewire
    glib
    gtk3
    gdk-pixbuf
    pango
    cairo
    atk
    harfbuzz
    libsoup_3
    webkitgtk_4_1
    xdotool
    libx11
    libxcursor
    libxrandr
    libxi
    libxcb
    libxkbcommon
    wayland
    libGL
    vulkan-loader
    # ignition-viz's `ffmpeg` feature (off by default); its major must match
    # the `ffmpeg-next` major in crates/ignition-viz/Cargo.toml.
    ffmpeg
  ];
  # On LD_LIBRARY_PATH / DYLD_LIBRARY_PATH so a test binary runs from this
  # shell. libstdc++ for the NAM C++ core.
  runtimeLibs = with pkgs; [ stdenv.cc.cc.lib fontconfig freetype openssl ] ++ linux;
in
{
  packages = linux ++ (with pkgs; [
    cargo-nextest
    # Native headers and build tools for build scripts.
    openssl
    openssl.dev
    libiconv
    fontconfig
    freetype
    cmake
    python3
    # cargo fetches every git dependency through the git CLI. On macOS this
    # real git shadows the /usr/bin/git xcrun shim.
    git
    # Sets bindgen's clang arguments for crates that run it (pipewire-sys,
    # ffmpeg-sys).
    rustPlatform.bindgenHook
    cargoNightly
  ])
  ++ lib.optionals pkgs.stdenv.isLinux (with pkgs; [
    alsa-lib.dev
    pipewire.dev
    # cargo-sweep reclaims stale target/ artifacts (`just sweep`).
    cargo-sweep
  ]);

  fleet.darwin.libraries = runtimeLibs;

  env = {
    LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
    OPENSSL_DIR = "${pkgs.openssl.dev}";
    OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
    # Read by crates/ignition-viz/build.rs, which embeds the font in the
    # binary (the operator overlay's cue sheet needs box-drawing glyphs).
    IGNITION_OVERLAY_FONT = "${pkgs.dejavu_fonts}/share/fonts/truetype/DejaVuSansMono.ttf";
  }
  // lib.optionalAttrs pkgs.stdenv.isLinux {
    LD_LIBRARY_PATH = lib.makeLibraryPath runtimeLibs;
    ORT_DYLIB_PATH = "${pkgs.onnxruntime}/lib/libonnxruntime.so";
    XDG_DATA_DIRS = "${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}";
    # WebKitGTK's accelerated compositing fails on NixOS (GBM buffer error,
    # white window). https://github.com/NixOS/nixpkgs/issues/32580
    WEBKIT_DISABLE_COMPOSITING_MODE = "1";
  }
  // lib.optionalAttrs pkgs.stdenv.isDarwin {
    PHON_JIT_NIGHTLY_RUSTC = "${rustNightly}/bin/rustc";
  };

  # kache is not set here: the rust module exports RUSTC_WRAPPER from PATH.
  # Store-provided tools (dx, wasm-bindgen) win over stale cargo-installed
  # copies: cargo's bin is appended, not prepended.
  enterShell = ''
    export PATH="$PATH:$HOME/.cargo/bin"
  '';

  fleet.banner = ''
    Ignition dev shell
      just lint / cargo test --workspace
      just tailwind
  '';
}
