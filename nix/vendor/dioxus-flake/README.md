# dioxus-flake

A reusable Nix flake that provides a complete development environment for Dioxus applications across all platforms — web, desktop, mobile, and native.

## Usage

Add this flake as an input in your project's `flake.nix`:

```nix
{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    dioxus-flake.url = "github:your-org/dioxus-flake";
  };

  outputs = { nixpkgs, dioxus-flake, ... }:
    # Use dioxus-flake's devShell, packages, and checks
    # in your own flake outputs.
}
```

## What's included

- Rust toolchain with targets for web (WASM), desktop, mobile (Android), and native
- `dioxus-cli`, `wasm-bindgen-cli`, and `tailwindcss`
- All native dependencies for Linux (GTK/WebView, X11, Wayland, Vulkan) and macOS
- Crane-based Nix builds for web and desktop packages
- Reusable OCI image helpers for Dioxus web/fullstack deployments
- CI checks for clippy, formatting, and tests

## OCI Web Deployment

For Dioxus web/fullstack sites that produce a `dx bundle --platform web --release`
server bundle, use the container helpers from this flake instead of copying a
custom `nix2container` expression into every project.

Example project `flake.nix`:

```nix
{
  inputs = {
    dioxus-flake.url = "github:FastTrackStudios/Dioxus-Flake";
    nixpkgs.follows = "dioxus-flake/nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.follows = "dioxus-flake/rust-overlay";
  };

  outputs =
    { dioxus-flake, nixpkgs, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachSystem
      [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ]
      (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };

          appName = "fasttrackstudio";
          dxPackageName = "fasttrackstudio-web";
          deploy = dioxus-flake.lib.${system};

          bundleEnv = builtins.getEnv "DX_BUNDLE_DIR";
          bundlePath =
            if bundleEnv != ""
            then /. + bundleEnv
            else ./bundle;

          image = deploy.mkDioxusWebImage {
            inherit appName bundlePath;
            imageName = "registry.fly.io/${appName}";
          };
        in
        {
          devShells.default = pkgs.mkShell {
            inputsFrom = [ dioxus-flake.devShells.${system}.default ];
            shellHook = deploy.mkDioxusWebBundleShellHook {
              inherit appName dxPackageName;
            };
          };

          packages = {
            inherit image;
            default = image;
          };

          apps = deploy.mkDioxusWebDeployApps {
            inherit appName dxPackageName;
            imagePackage = "image";
            registry = "registry.fly.io";
          };
        });
}
```

Standard workflow:

```sh
nix develop
site-bundle
site-image
nix run .#push
nix run .#deploy
```

The generated image expects Dioxus' bundled server at `/app/server` and exposes
`PORT=8080` with `IP=0.0.0.0`, matching Fly.io and most OCI runtimes. Use
`DX_BUNDLE_DIR=$PWD/bundle nix build --impure .#image` directly, or point
`DX_BUNDLE_DIR` at another absolute web bundle path, when not using the
`site-image` alias.
