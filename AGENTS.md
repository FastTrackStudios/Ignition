# Ignition: agent instructions

## Build and test

- Build inside the fleet's devenv shell: `devenv shell -- <command>`, or
  direnv through `.envrc` (`eval "$(devenv direnvrc)"` then `use devenv`; a
  bare `use devenv` fails silently in an agent shell and leaves the host's
  rustc). devenv-fleet's `rust`, `dioxus` and `darwin` modules (devenv.yaml)
  are the toolchain, dx and the iOS/wasm setup; `devenv.nix` adds what the
  workspace links and the env its build scripts read.
- `devenv shell -- fleet-doctor` shows the rustc store path, the kache
  wrapper and the linker; it fails off the fleet pin.
- Move the pins only with devenv-fleet's `scripts/fleet-pins --write .` (it
  rewrites devenv.yaml and devenv.lock), never by hand. flake.nix takes its
  toolchain from the same devenv-fleet revision: move the two together.
  `fleet-pins .` reports flake.lock's nixpkgs and rust-overlay as off the
  pins; that is deliberate (flake.nix: on the fleet's nixpkgs the CI shell's
  gcc flags pass Linux's 128 KiB limit). Its rustc is still the fleet's.
- Never set or unset `RUSTC_WRAPPER` or `CARGO_HOME`, and never set
  `RUSTFLAGS`: `.cargo/config.toml`'s rustflags tables are the fleet's, flag
  for flag, so kache shares builds across the fleet's repos.
- CI enters the flake's `nix develop .#ci`, which takes the same toolchain
  (devenv-fleet's `toolchainFor`); keep its package list
  (nix/modules/toolchain.nix) in step with devenv.nix.
