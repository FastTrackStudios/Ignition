{
  description = "Ignition — the lighting console, DMX visualizer, and the Ignition iOS app";

  # Dendritic layout (den): every .nix under nix/modules/ is a
  # flake-parts module, auto-loaded by import-tree — one file per
  # concern, no central wiring. Shared values flow through the typed
  # `fts.*` perSystem options (nix/modules/options.nix); den's aspect
  # system (nix/modules/den.nix) is the sharing surface with the
  # system flake.
  outputs = inputs: inputs.flake-parts.lib.mkFlake { inherit inputs; }
    (inputs.import-tree ./nix/modules);

  inputs = {
    den.url = "github:denful/den";
    import-tree.url = "github:vic/import-tree";

    # The Rust toolchain: devenv-fleet at the revision devenv.yaml imports,
    # evaluated on ITS pinned nixpkgs and rust-overlay
    # (nix/modules/toolchain.nix), so `nix develop` (and `.#ci`) has the
    # devenv shell's rustc store path and kache shares builds with every
    # fleet repo. Move it with devenv.yaml, never alone.
    devenv-fleet.url = "github:FastTrackStudios/devenv-fleet/c4ddec850ad97b5da74c746bff39a988f02b13a4";

    # Shared Dioxus hub: the native libraries' nixpkgs (5e11f7ac), as
    # before. Not the fleet's newer one: on it, the shell's buildInputs
    # (GTK/WebKit, ffmpeg, onnxruntime and their closures) put more
    # -isystem/-L/-rpath flags on every gcc call than Linux passes to
    # collect2 (COLLECT_GCC_OPTIONS is one string, 128 KiB at most), so
    # every link fails with "Argument list too long" (measured on
    # patchbay's flake, the same set). The devenv shell has no such limit.
    dioxus-flake.url = "github:FastTrackStudios/Dioxus-Flake";
    nixpkgs.follows = "dioxus-flake/nixpkgs";

    # rust-overlay: only the phon-jit / wasm-threads nightlies
    # (nix/modules/toolchain.nix) read it now; the stable toolchain is
    # devenv-fleet's.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-parts.url = "github:hercules-ci/flake-parts";

    # crane — cargo-in-nix builds. Carried for parity with the sibling
    # repos even though Ignition ships no image yet.
    crane.url = "github:ipetkov/crane";

    # Dedicated, current-unstable nixpkgs used ONLY to source `dx`
    # (dioxus-cli, overridden there onto 0.8.0-alpha.0) and binaryen 129
    # — see nix/modules/dx.nix.
    nixpkgs-dx.url = "github:NixOS/nixpkgs/d99b013d5d1931ad77fe3912ed218170dec5d9a4";
  };

  nixConfig = {
    extra-trusted-public-keys = [
      "fasttrackstudio.cachix.org-1:r7v7WXBeSZ7m5meL6w0wttnvsOltRvTpXeVNItcy9f4="
    ];
    extra-substituters = [
      "https://fasttrackstudio.cachix.org"
    ];
  };
}
