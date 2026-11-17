# The lint gate

`[workspace.lints.clippy]` in the root `Cargo.toml` denies clippy's
`pedantic` and `nursery` groups plus the panic set — `unwrap_used`,
`expect_used`, `indexing_slicing`, `arithmetic_side_effects`, `panic`,
`todo`, `unimplemented`, `unreachable`, `exit`, `string_slice`,
`panic_in_result_fn`, `unchecked_time_subtraction`, `as_conversions`.
`clippy.toml` lifts the panic lints inside `#[cfg(test)]`.

Every crate that is ours takes the table with:

```toml
[lints]
workspace = true
```

The five `crates/*-vendored` forks never do. They are upstream code
carrying one deliberate hunk each, and policing their style would mean
editing them for reasons their `NOTICE.md` cannot justify — the
`vendored :=` list in the `Justfile` is the same exclusion from the
other side.

`just lint` is the gate.

## Why deny, and why the panic set

`deny`, not `warn`: this tree went from 0 to 55 warnings without anyone
noticing, and a gate nothing has to pass is not a gate.

The panic set is the load-bearing half. A desk that unwraps its way into
a `None` at bar 43 takes the room dark, and there is no operator recovery
from a process that is gone — no fader to pull, no cue to take, nothing
on stage but the house lights someone has to find. Every `unwrap` removed
from `src/` is one fewer way for that to happen. In `#[cfg(test)]` the
same call is an assertion about a fixture the test just built, which is
why `clippy.toml` allows it there and only there.

## Idioms

These are the shapes the first cleaned crate (`ignition-proto`) settled
on. Prefer them to a fresh invention.

**A lossy conversion happens once, in a named function.** `as_conversions`
is denied so that `as` cannot appear inline in domain code, not so that
floats can never become bytes. The answer is one small helper whose
contract makes the cast total, with the reason on it:

```rust
/// The one place in the tree a float becomes a DMX byte.
/// … NaN lands on 0, ≤0 on 0, ≥255 on 255. The cast that follows
/// cannot lose a sign or truncate anything the clamp has not decided.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the clamp above makes this cast total; see the doc comment"
)]
const fn byte(value: f32) -> u8 {
    if value.is_nan() { return 0; }
    value.round().clamp(0.0, 255.0) as u8
}
```

Use `f32::from(b)` / `usize::from(n)` / `i64::from(n)` wherever a
conversion is infallible — most of them are.

**`#[expect]`, not `#[allow]`, and always with a `reason`.** `expect`
fails the build when the lint stops firing, so a suppression cannot
outlive the thing it was suppressing. Scope it to the smallest item that
needs it. A crate-root `#![expect(...)]` is a last resort and needs a
paragraph saying why (the precedent is Bevy's two unavoidable lints at
the top of `crates/ignition-viz/src/lib.rs`).

**`get` over `[]`.** Anything read out of a venue file, a show file, a
GDTF profile or a DMX frame is *data*, and data is not a place to panic.
`let Some(x) = slice.get(i) else { … }` with a stated fallback. Where the
index is a loop counter over the same slice, iterate instead.

**`saturating_*` / `checked_*` over bare arithmetic** on lengths and
indices. On floats, `arithmetic_side_effects` does not fire — it is
integer overflow the lint is about.

**`mul_add`** where `nursery` asks for it: `(b - a).mul_add(t, a)` is one
rounding instead of two, which in a fade is the difference between a
value that lands on its target and one that stops a byte short.

**Prose names go in `clippy.toml`'s `doc-valid-idents`**, not in
backticks. `FastTrackStudio`, `grandMA3`, `ChamSys` and friends are
names, not items, and a doc comment that ticks them reads as though they
were types.

## What is not a fix

- Deleting a lint from the workspace table.
- `#[allow]` without a reason, or reaching for one before trying the real
  fix.
- Changing behaviour to satisfy a lint. An `arithmetic_side_effects` fix
  that turns a wrap into a saturate has changed what the code does; if
  that is right, say so in the commit, and if it is not, use
  `wrapping_*`.

## Integration tests and examples

`clippy.toml`'s `allow-*-in-tests` only reaches `#[cfg(test)]` modules.
A file under `tests/` or `examples/` is a separate crate, so the panic
lints fire there in full — and a `#[test]` that unwraps the fixture it
just built is the same assertion whether it lives in `src/` or in
`tests/`. Those files carry one blanket at the top, and it is the only
sanctioned blanket in the tree:

```rust
// Integration test: `clippy.toml`'s test allowances only reach
// `#[cfg(test)]` modules, so the panic set is lifted here instead.
// See docs/ops/clippy.md.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "integration test — see docs/ops/clippy.md"
)]
```

`allow` rather than `expect` here: `expect` fails the build when a listed
lint stops firing, and a test file that happens not to index anything
today should not break when someone adds a case that does.

Everything else — `pedantic` and `nursery` — still applies to test code.
`sort_unstable` on a `Vec<u32>`, a redundant closure, a doc comment
missing backticks: fix those in a test exactly as in `src/`.
