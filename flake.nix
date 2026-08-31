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
    rust-overlay.follows = "dioxus-flake/rust-overlay";
    flake-parts.url = "github:hercules-ci/flake-parts";

    # crane — cargo-in-nix builds. Carried for parity with the sibling
    # repos even though Ignition ships no image yet.
    crane.url = "github:ipetkov/crane";

    # Dedicated, current-unstable nixpkgs used ONLY to source `dx`
    # (dioxus-cli) at the version the workspace Cargo.lock pins (0.7.9)
    # plus binaryen 129 — see nix/modules/dx.nix.
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
