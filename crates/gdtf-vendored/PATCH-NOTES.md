# Vendored fork of `gdtf` 0.3.0

Source: https://github.com/cpdt/gdtf-rs, MIT-licensed (see `LICENSE`).
Vendored (not a plain crates.io dependency) because the upstream crate has
no public way to read a parsed `Matrix`'s values at all — `Matrix`'s inner
`[[f64; 4]; 4]` field is private and there is no accessor method, only
`Serialize`/`Deserialize`. Every `<Geometry>` node's placement (`Position`
attribute) is a `Matrix`, so this blocks reading GDTF geometry trees
entirely through the public API, not just an inconvenience.

## Patch

`src/description/values.rs`, `impl Matrix`: added

```rust
/// The matrix's rows, as a plain array — vendored accessor, not present
/// upstream. See PATCH-NOTES.md.
pub const fn rows(&self) -> [[f64; 4]; 4] {
    self.0
}
```

`src/description/geometry.rs`, test module: replaced 4 uses of
`std::assert_matches!` (the unstable nightly-only macro) with
`assert!(matches!(...))`. Not a functional change — vendoring this crate
as a workspace member means `cargo test --workspace` now builds gdtf's
own tests, which failed to compile on stable rust without this.

`src/lib.rs`, crate-doc example: changed the fenced block from ```` ```rust ```` to ```` ```no_run ````. It opens `Generic@RGBW8@test.gdtf` by a relative path that only existed in the upstream repo's root (test fixtures weren't vendored, only `src/`); `no_run` keeps the example compiling as a doctest without needing that file at runtime.

Re-sync with upstream by re-vendoring `src/` from a newer `gdtf` release
and re-applying these three patches (the `Matrix::rows()` accessor, the
`assert_matches!` stable-rust fix, and the doctest `no_run` fix).

Used by `crates/ignition-viz/src/gdtf_geometry.rs` (Ignition's GDTF
3D-geometry importer) — see
`docs/research/lighting-console-landscape.md`'s GDTF-geometry slice for
what it's for.
