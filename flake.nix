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

    # Shared Dioxus toolchain hub — every FTS Dioxus repo follows its
    # nixpkgs / rust-overlay pins so `dx` and the Rust toolchain stay
    # in lockstep.
    dioxus-flake.url = "github:FastTrackStudios/Dioxus-Flake";
    nixpkgs.follows = "dioxus-flake/nixpkgs";

    # rust-overlay does NOT follow the Dioxus hub, unlike every other
    # FTS Dioxus repo. Bevy 0.19 raises its MSRV to Rust 1.95 (and the
    # workspace `rust-version` says so), while the hub's overlay is
    # pinned old enough that `rust-bin.stable."1.95.0"` does not exist
    # in it — the toolchain simply is not there to select. So Ignition
    # takes the overlay directly and keeps the hub's nixpkgs, which is
    # what `dx` and the wasm story actually depend on.
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
