//! Tests that span two layers of the domain, and so live in the upper one.
//!
//! Each of these was a unit test inside `ignition-show` (or
//! `ignition-rig`) that reached *up* the layering for something it
//! asserted about — the shipped effect library, the desk macros, the
//! recipe type. Once those layers became separate crates the reach up
//! stopped being possible: a `#[cfg(test)]` module compiles its own copy
//! of its crate, so the types it saw through a dev-dependency were a
//! second, incompatible `ignition_show`.
//!
//! An integration test links the real library instead, which is why
//! these belong here. The layering did not change; where the test is
//! allowed to sit did.

use ignition_rig::Trick;

/// A recipe carries its tricks in the same object as its selection
/// and its values.
///
/// Tricks are not a stage a value passes through on its way
/// somewhere. In grandMA3 they are columns on the recipe line, and
/// that is why one recipe there covers what would otherwise need a
/// dozen — so they have to travel with the recipe, including
/// through a file.
///
/// r[verify tricks.on-the-recipe]
#[test]
fn a_recipe_carries_its_tricks_and_they_survive_a_file() {
    use ignition_rig::selection::Selection;
    use ignition_show::recipe::{Recipe, RecipeApply};

    let mut recipe = Recipe::new(Selection::Group("Pars".into()), RecipeApply::Dimmer(1.0));
    recipe.tricks = vec![Trick::Block(2), Trick::Wings(2)];

    let json = serde_json::to_string(&recipe).expect("a recipe serialises");
    let back: Recipe = serde_json::from_str(&json).expect("and parses back");
    assert_eq!(
        back.tricks, recipe.tricks,
        "the tricks did not travel with the recipe"
    );
}
